use anyhow::{Context, Result};
use reqwest::blocking::Client;
use thegrid_core::{TailscaleDevice, models::TailscaleDevicesResponse};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const API_BASE: &str = "https://api.tailscale.com/api/v2";
const CACHE_DURATION: Duration = Duration::from_secs(300); // 5 minutes

pub struct TailscaleClient {
    http: Client,
    api_key: String,
    cache: Arc<Mutex<Option<(Vec<TailscaleDevice>, Instant)>>>,
}

impl TailscaleClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let mut key: String = api_key.into();
        key = key.trim().to_string();
        
        // Ensure standard lowercase prefix if it matches
        if key.to_lowercase().starts_with("tskey-api-") {
            let suffix = &key[10..];
            key = format!("tskey-api-{}", suffix);
        }

        let http = Client::builder()
            .user_agent(concat!("THEGRID/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10))
            .build()
            .context("Building HTTP client")?;
        Ok(Self { 
            http, 
            api_key: key,
            cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Fetch all devices in the default tailnet ("-" means the tailnet of
    /// the authenticated key). Uses cache if available and not expired.
    pub fn fetch_devices(&self) -> Result<Vec<TailscaleDevice>> {
        if let Ok(guard) = self.cache.lock() {
            if let Some((devices, ts)) = &*guard {
                if ts.elapsed() < CACHE_DURATION {
                    return Ok(devices.clone());
                }
            }
        }

        let url = format!("{}/tailnet/-/devices", API_BASE);
        log::debug!("GET {}", url);

        let resp_result = self.http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send();

        let resp = match resp_result {
            Ok(r) => r,
            Err(e) => return self.stale_fallback(anyhow::anyhow!("Request failed: {}", e)),
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            log::error!("Tailscale API Error {}: {}", status, body);
            return self.stale_fallback(anyhow::anyhow!(
                "Tailscale API returned {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }

        let mut data: TailscaleDevicesResponse = match resp.json() {
            Ok(d) => d,
            Err(e) => return self.stale_fallback(anyhow::anyhow!("JSON Parsing failed: {}", e)),
        };

        data.devices.sort_by(|a, b| {
            a.hostname.to_lowercase().cmp(&b.hostname.to_lowercase())
        });

        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some((data.devices.clone(), Instant::now()));
        }

        log::info!("Fetched {} devices from Tailscale", data.devices.len());
        Ok(data.devices)
    }

    fn stale_fallback(&self, original_err: anyhow::Error) -> Result<Vec<TailscaleDevice>> {
        if let Ok(guard) = self.cache.lock() {
            if let Some((devices, _)) = &*guard {
                log::warn!("Fetch failed ({}), using stale tailnet cache", original_err);
                return Ok(devices.clone());
            }
        }
        Err(original_err)
    }

    /// Checks if a given IP address belongs to any device in the tailnet.
    pub fn is_ip_in_tailnet(&self, ip: &str) -> bool {
        let devices = match self.fetch_devices() {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to fetch tailnet devices for trust check: {}", e);
                return false;
            }
        };

        for dev in devices {
            for addr in dev.addresses {
                if addr == ip || addr.split('/').next() == Some(ip) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thegrid_core::TailscaleDevice;

    #[test]
    fn test_ip_matching() {
        let device = TailscaleDevice {
            id: "1".into(),
            hostname: "test".into(),
            name: "test".into(),
            addresses: vec!["100.64.0.1/32".into(), "fd7a:1111:1111:1111:1111:1111:1111:1111".into()],
            os: "linux".into(),
            client_version: "1.0".into(),
            last_seen: None,
            blocks_incoming: false,
            authorized: true,
            user: "user".into(),
        };

        // We can't easily mock fetch_devices without a trait, but we can test the logic if we extract it.
        // For now, let's just verify the string matching logic I used.
        let addr = &device.addresses[0];
        let ip = "100.64.0.1";
        assert!(addr == ip || addr.split('/').next() == Some(ip));

        let ip_wrong = "100.64.0.2";
        assert!(!(addr == ip_wrong || addr.split('/').next() == Some(ip_wrong)));
    }
}
