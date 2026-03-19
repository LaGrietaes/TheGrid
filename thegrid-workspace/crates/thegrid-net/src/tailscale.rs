use anyhow::{Context, Result};
use reqwest::blocking::Client;
use thegrid_core::{TailscaleDevice, models::TailscaleDevicesResponse};

const API_BASE: &str = "https://api.tailscale.com/api/v2";

pub struct TailscaleClient {
    http: Client,
    api_key: String,
}

impl TailscaleClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .user_agent(concat!("THEGRID/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Building HTTP client")?;
        Ok(Self { http, api_key: api_key.into() })
    }

    /// Fetch all devices in the default tailnet ("-" means the tailnet of
    /// the authenticated key).
    pub fn fetch_devices(&self) -> Result<Vec<TailscaleDevice>> {
        let url = format!("{}/tailnet/-/devices", API_BASE);
        log::debug!("GET {}", url);

        let resp = self.http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .context("Sending devices request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Tailscale API returned {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }

        let mut data: TailscaleDevicesResponse = resp.json()
            .context("Parsing devices JSON")?;

        data.devices.sort_by(|a, b| {
            a.hostname.to_lowercase().cmp(&b.hostname.to_lowercase())
        });

        log::info!("Fetched {} devices from Tailscale", data.devices.len());
        Ok(data.devices)
    }
}
