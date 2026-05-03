use anyhow::Result;
use std::process::{Command, Stdio};
use std::io::Write;
use std::path::PathBuf;
use thegrid_core::models::FabricIntent;

/// The Interpreter bridge that talks to the Fabric CLI.
pub struct Interpreter;

impl Interpreter {
    /// Pipes raw human input through a Fabric pattern to get structured intent.
    ///
    /// `ai_policy` controls whether cloud AI calls are permitted:
    /// - "local_only"    → Fabric must be configured with a local model or this errors.
    /// - "local_first"   → Fabric runs; if it uses cloud, that's on Fabric's own config.
    /// - "cloud_allowed" → No restriction.
    ///
    /// Pass `fabric_model` (e.g. `"ollama/llama3:8b"`) to force a local model via
    /// Fabric's `--model` flag, overriding whatever Fabric's own default is.
    pub fn interpret(
        input: &str,
        pattern: &str,
        fabric_path: Option<PathBuf>,
        ai_policy: &str,
        fabric_model: Option<&str>,
    ) -> Result<FabricIntent> {
        if ai_policy == "local_only" && fabric_model.is_none() {
            log::warn!(
                "[Symbiosis] ai_policy=local_only but no fabric_model set. \
                 Fabric may call cloud. Set fabric_model in config to enforce local."
            );
        }

        let binary = fabric_path
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "fabric".to_string());

        log::info!(
            "[Symbiosis] Interpreting intent with {} (pattern: {}, policy: {}, model: {:?})",
            binary, pattern, ai_policy, fabric_model
        );

        let mut cmd = Command::new(&binary);
        cmd.arg("--pattern").arg(pattern);
        if let Some(model) = fabric_model {
            cmd.arg("--model").arg(model);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn {}: {}. Ensure path is correct.", binary, e))?;

        let mut stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?;
        stdin.write_all(input.as_bytes())?;
        drop(stdin);

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Fabric interpretation failed: {}", err);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Attempt to parse JSON. Fabric patterns like 'refine_intent' should ideally output JSON.
        let intent: FabricIntent = serde_json::from_str(&stdout).unwrap_or_else(|_| {
            log::warn!("[Symbiosis] Fabric output was not valid JSON, using fallback parsing.");
            FabricIntent {
                intent: stdout.trim().to_string(),
                target_files: None,
                destination_node: None,
                action_type: "unstructured_task".into(),
                urgency: None,
            }
        });

        Ok(intent)
    }

    /// Convenience method for the 'refine_intent' common pattern.
    /// Uses "local_only" policy by default — no cloud unless you explicitly pass "local_first".
    pub fn refine(
        input: &str,
        fabric_path: Option<PathBuf>,
        ai_policy: &str,
        fabric_model: Option<&str>,
    ) -> Result<FabricIntent> {
        Self::interpret(input, "refine_intent", fabric_path, ai_policy, fabric_model)
    }
}

/// Client for the Ruflo Agent Swarm.
pub struct SwarmClient {
    pub base_url: String,
    pub binary_path: Option<PathBuf>,
    pub client: reqwest::blocking::Client,
}

impl SwarmClient {
    pub fn new(url: &str, binary_path: Option<PathBuf>) -> Self {
        Self {
            base_url: url.trim_end_matches('/').to_string(),
            binary_path,
            client: reqwest::blocking::Client::new(),
        }
    }

    /// Ensures the Ruflo orchestrator is running.
    pub fn ensure_running(&self) -> Result<()> {
        let url = format!("{}/health", self.base_url);
        if self.client.get(&url).send().is_ok() {
            return Ok(());
        }

        let binary = self.binary_path.as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "ruflo".to_string());

        log::info!("[Symbiosis] Swarm not found at {}. Attempting to start {}", self.base_url, binary);

        Command::new(&binary)
            .arg("start")
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start Ruflo swarm: {}", e))?;

        Ok(())
    }

    /// Dispatches a FabricIntent to the Ruflo swarm for execution.
    pub fn dispatch_task(&self, intent: &FabricIntent) -> Result<String> {
        let _ = self.ensure_running();
        log::info!("[Symbiosis] Dispatching task to Ruflo swarm: {:?}", intent.intent);
        
        let url = format!("{}/api/task", self.base_url);
        let resp = self.client.post(&url)
            .json(intent)
            .send()
            .map_err(|e| anyhow::anyhow!("Failed to reach Ruflo swarm at {}: {}", url, e))?;

        if !resp.status().is_success() {
            anyhow::bail!("Ruflo swarm rejected task with status: {}", resp.status());
        }

        let task_id = resp.text()?;
        log::info!("[Symbiosis] Task accepted by Ruflo. ID: {}", task_id);
        Ok(task_id)
    }

    /// Requests a distributed search via Ruflo's Librarian agent.
    pub fn distributed_search(&self, query: &str) -> Result<Vec<String>> {
        let _ = self.ensure_running();
        let url = format!("{}/api/search/distributed", self.base_url);
        let resp = self.client.get(&url)
            .query(&[("q", query)])
            .send()?;

        if !resp.status().is_success() {
            anyhow::bail!("Ruflo distributed search failed: {}", resp.status());
        }

        Ok(resp.json()?)
    }
}
