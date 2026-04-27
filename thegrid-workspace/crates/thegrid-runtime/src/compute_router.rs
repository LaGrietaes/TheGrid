use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use thegrid_core::{
    AppEvent, Config,
    models::{
        ComputeCapabilities, ComputeStatus, ComputeTaskProgress,
        ComputeTaskReceipt, ComputeTaskRequest, ComputeTaskState, ComputeTaskType,
        TailscaleDevice,
    },
};
use thegrid_net::AgentClient;

// ── Route target ──────────────────────────────────────────────────────────────

pub enum RouteTarget {
    /// Delegate to a remote peer; includes a fresh client and its device id.
    Remote {
        device_id: String,
        ip:        String,
        client:    Arc<AgentClient>,
    },
    /// Execute locally — no suitable peer found.
    Local,
}

impl std::fmt::Debug for RouteTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "RouteTarget::Local"),
            Self::Remote { device_id, ip, .. } =>
                write!(f, "RouteTarget::Remote {{ device_id: {:?}, ip: {:?} }}", device_id, ip),
        }
    }
}

// ── Peer snapshot used during selection ──────────────────────────────────────

#[derive(Debug)]
struct PeerSnapshot {
    device_id: String,
    ip:        String,
    status:    ComputeStatus,
    caps:      ComputeCapabilities,
}

// ── ComputeRouter ─────────────────────────────────────────────────────────────

pub struct ComputeRouter {
    config:   Arc<Mutex<Config>>,
    event_tx: std::sync::mpsc::Sender<AppEvent>,

    /// device_id → in-flight task_ids owned by this borrower
    active_borrows: Mutex<HashMap<String, String>>,
}

impl ComputeRouter {
    pub fn new(config: Arc<Mutex<Config>>, event_tx: std::sync::mpsc::Sender<AppEvent>) -> Self {
        Self {
            config,
            event_tx,
            active_borrows: Mutex::new(HashMap::new()),
        }
    }

    /// Choose the best peer for `task_type` given the current Tailscale device list.
    /// Returns `RouteTarget::Local` when no peer is available or willing.
    pub fn route_task(
        &self,
        task_type: &ComputeTaskType,
        peers: &[TailscaleDevice],
    ) -> RouteTarget {
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };

        // Filter to peers that advertise capability for this task type and are reachable.
        let mut candidates: Vec<PeerSnapshot> = peers
            .iter()
            .filter(|d| d.is_likely_online())
            .filter_map(|d| {
                let ip = d.primary_ip()?.to_string();
                let client = AgentClient::new(&ip, port, api_key.clone()).ok()?;
                let status = client.get_compute_status().ok()?;
                let caps = d.capabilities_hint();

                if !caps.supported_task_types.contains(task_type) {
                    return None;
                }
                if !status.available {
                    return None;
                }

                Some(PeerSnapshot {
                    device_id: d.id.clone(),
                    ip,
                    status,
                    caps,
                })
            })
            .collect();

        if candidates.is_empty() {
            return RouteTarget::Local;
        }

        // Sort: prefer GPU-present, then fewer active tasks, then lower eta.
        candidates.sort_by(|a, b| {
            let a_gpu = a.caps.gpu_available as u8;
            let b_gpu = b.caps.gpu_available as u8;
            b_gpu.cmp(&a_gpu)
                .then(a.status.active_tasks.cmp(&b.status.active_tasks))
                .then(
                    a.status.busy_until_estimate_secs.unwrap_or(0)
                        .cmp(&b.status.busy_until_estimate_secs.unwrap_or(0)),
                )
        });

        let best = &candidates[0];
        let client = match AgentClient::new(&best.ip, port, api_key) {
            Ok(c) => Arc::new(c),
            Err(_) => return RouteTarget::Local,
        };

        RouteTarget::Remote {
            device_id: best.device_id.clone(),
            ip:        best.ip.clone(),
            client,
        }
    }

    /// Attempt to delegate `request` to a remote peer.
    /// On acceptance emits `ComputeBorrowOk`; on rejection or error emits `ComputeBorrowFailed`.
    /// Returns the receipt so the caller can poll progress.
    pub fn try_delegate(
        &self,
        request: ComputeTaskRequest,
        peers: &[TailscaleDevice],
    ) -> Result<ComputeTaskReceipt> {
        let target = self.route_task(&request.task_type, peers);

        match target {
            RouteTarget::Local => {
                let _ = self.event_tx.send(AppEvent::ComputeBorrowFailed {
                    task_id: request.task_id.clone(),
                    reason:  "no capable peer available".to_string(),
                });
                Err(anyhow!("no capable peer — execute locally"))
            }

            RouteTarget::Remote { device_id, ip: _, client } => {
                match client.post_compute_request(&request) {
                    Ok(receipt) if receipt.accepted => {
                        self.active_borrows
                            .lock()
                            .unwrap()
                            .insert(device_id.clone(), request.task_id.clone());

                        let _ = self.event_tx.send(AppEvent::ComputeBorrowOk {
                            task_id:            request.task_id.clone(),
                            provider_device_id: device_id,
                            task_type:          request.task_type.clone(),
                        });
                        Ok(receipt)
                    }

                    Ok(receipt) => {
                        let reason = receipt.reason_if_rejected
                            .unwrap_or_else(|| "peer rejected".to_string());
                        let _ = self.event_tx.send(AppEvent::ComputeBorrowFailed {
                            task_id: request.task_id,
                            reason:  reason.clone(),
                        });
                        Err(anyhow!("{}", reason))
                    }

                    Err(e) => {
                        let _ = self.event_tx.send(AppEvent::ComputeBorrowFailed {
                            task_id: request.task_id,
                            reason:  e.to_string(),
                        });
                        Err(e)
                    }
                }
            }
        }
    }

    /// Poll progress for `task_id` from `provider_device_id`.
    /// Emits `ComputeTaskUpdate` and returns the latest progress.
    pub fn poll_progress(
        &self,
        task_id: &str,
        provider_device_id: &str,
        peers: &[TailscaleDevice],
    ) -> Result<ComputeTaskProgress> {
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };

        let peer = peers.iter()
            .find(|d| d.id == provider_device_id)
            .ok_or_else(|| anyhow!("provider device not found in peer list"))?;

        let ip = peer.primary_ip()
            .ok_or_else(|| anyhow!("provider has no IP address"))?;

        let client = AgentClient::new(ip, port, api_key)?;
        let progress = client.get_compute_progress(task_id)?;

        let _ = self.event_tx.send(AppEvent::ComputeTaskUpdate(progress.clone()));

        if matches!(progress.state, ComputeTaskState::Done | ComputeTaskState::Failed | ComputeTaskState::Cancelled) {
            let _ = client.ack_compute_result(task_id);
            self.active_borrows.lock().unwrap().remove(provider_device_id);
        }

        Ok(progress)
    }

    /// Cancel a delegated task.
    pub fn cancel(
        &self,
        task_id: &str,
        provider_device_id: &str,
        peers: &[TailscaleDevice],
    ) -> Result<()> {
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };

        let peer = peers.iter()
            .find(|d| d.id == provider_device_id)
            .ok_or_else(|| anyhow!("provider device not found"))?;

        let ip = peer.primary_ip()
            .ok_or_else(|| anyhow!("provider has no IP address"))?;

        let client = AgentClient::new(ip, port, api_key)?;
        client.cancel_compute_task(task_id)?;
        self.active_borrows.lock().unwrap().remove(provider_device_id);
        Ok(())
    }

    pub fn active_borrow_count(&self) -> usize {
        self.active_borrows.lock().unwrap().len()
    }
}

// ── TailscaleDevice capability hint ──────────────────────────────────────────

trait CapabilityHint {
    /// Best-effort compute caps from what we know statically about the device.
    /// The real caps come from the ping response; this is used only during routing
    /// before a full handshake has occurred.
    fn capabilities_hint(&self) -> ComputeCapabilities;
}

impl CapabilityHint for TailscaleDevice {
    fn capabilities_hint(&self) -> ComputeCapabilities {
        // We don't store caps directly on TailscaleDevice — return a permissive
        // default so that the status check is the real gate.
        ComputeCapabilities {
            gpu_available: false,
            gpu_models:    vec![],
            gpu_vram_mb:   0,
            cpu_cores:     0,
            ram_available_mb: 0,
            max_parallel_tasks: 1,
            supported_task_types: vec![
                ComputeTaskType::TextEmbedding,
                ComputeTaskType::ImageEmbedding,
                ComputeTaskType::FullHash,
            ],
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn make_router() -> ComputeRouter {
        let cfg = Arc::new(Mutex::new(Config::default()));
        let (tx, _rx) = mpsc::channel();
        ComputeRouter::new(cfg, tx)
    }

    #[test]
    fn routes_to_local_when_no_peers() {
        let router = make_router();
        let target = router.route_task(&ComputeTaskType::TextEmbedding, &[]);
        assert!(matches!(target, RouteTarget::Local));
    }

    #[test]
    fn routes_to_local_when_peers_offline() {
        let router = make_router();
        let mut peer = TailscaleDevice {
            id:               "node-a".to_string(),
            hostname:         "node-a".to_string(),
            name:             "node-a".to_string(),
            addresses:        vec!["100.0.0.1/32".to_string()],
            os:               "linux".to_string(),
            client_version:   String::new(),
            last_seen:        None,
            blocks_incoming:  true,
            authorized:       true,
            user:             String::new(),
        };
        let target = router.route_task(&ComputeTaskType::TextEmbedding, &[peer]);
        assert!(matches!(target, RouteTarget::Local));
    }

    #[test]
    fn active_borrow_count_starts_at_zero() {
        let router = make_router();
        assert_eq!(router.active_borrow_count(), 0);
    }
}
