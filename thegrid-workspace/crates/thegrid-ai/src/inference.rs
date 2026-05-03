//! Text inference provider abstraction.
//!
//! Priority chain (mirrors ai_policy):
//!   1. Local Ollama  (`ai_provider_url` + `ai_model`)
//!   2. Google Gemini (`google_gemini_api_key` + `google_gemini_model`) — preferred cloud
//!   3. Claude        (`claude_api_key` + `claude_model`)              — last resort
//!
//! `build_inference_provider()` returns the highest-priority provider that is
//! reachable/configured given the current policy.  Under "local_only" only tier 1
//! is returned; a failing probe returns `None` (caller decides how to degrade).

use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Duration;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Capability trait for any text-inference backend.
pub trait InferenceProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    /// Single-turn completion.  `system` is an optional system prompt.
    fn complete(&self, system: Option<&str>, prompt: &str) -> Result<String>;
}

// ── 1. Local Ollama ───────────────────────────────────────────────────────────

pub struct OllamaInferenceProvider {
    model:    String,
    base_url: String,
    client:   reqwest::blocking::Client,
}

impl OllamaInferenceProvider {
    /// Connect to an Ollama instance and verify the model exists.
    pub fn new(model: String, base_url: String) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        // Probe: list models, confirm ours is present.
        let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
        let resp = client.get(&url).send()
            .map_err(|e| anyhow!("Ollama unreachable at {}: {}", base_url, e))?;
        if !resp.status().is_success() {
            return Err(anyhow!("Ollama /api/tags returned {}", resp.status()));
        }

        #[derive(serde::Deserialize)]
        struct M { name: String }
        #[derive(serde::Deserialize)]
        struct R { models: Vec<M> }
        let data: R = resp.json()
            .map_err(|e| anyhow!("Ollama tags parse error: {}", e))?;

        let available = data.models.iter().any(|m| m.name.starts_with(&model));
        if !available {
            return Err(anyhow!(
                "Ollama: model '{}' not found. Pull it with: ollama pull {}",
                model, model
            ));
        }

        log::info!("[Inference] Ollama ready: model={} url={}", model, base_url);
        Ok(Self { model, base_url, client })
    }
}

impl InferenceProvider for OllamaInferenceProvider {
    fn provider_id(&self) -> &str { "ollama-local" }

    fn complete(&self, system: Option<&str>, prompt: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url.trim_end_matches('/'));

        let mut body = serde_json::json!({
            "model":  self.model,
            "prompt": prompt,
            "stream": false,
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }

        let resp = self.client.post(&url).json(&body).send()
            .map_err(|e| anyhow!("Ollama generate request failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(anyhow!("Ollama generate HTTP {}", resp.status()));
        }

        #[derive(serde::Deserialize)]
        struct R { response: String }
        let data: R = resp.json()
            .map_err(|e| anyhow!("Ollama generate parse error: {}", e))?;
        Ok(data.response.trim().to_string())
    }
}

// ── 2. Google Gemini (preferred cloud tier) ───────────────────────────────────

pub struct GeminiInferenceProvider {
    api_key: String,
    model:   String,
    client:  reqwest::blocking::Client,
}

impl GeminiInferenceProvider {
    pub fn new(api_key: String, model: String) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("Gemini API key is empty"));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        log::info!("[Inference] Gemini configured: model={}", model);
        Ok(Self { api_key, model, client })
    }
}

impl InferenceProvider for GeminiInferenceProvider {
    fn provider_id(&self) -> &str { "gemini-cloud" }

    fn complete(&self, system: Option<&str>, prompt: &str) -> Result<String> {
        // Gemini generateContent endpoint
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        // Build contents array; prepend system as first user turn if present.
        let mut parts: Vec<serde_json::Value> = Vec::new();
        if let Some(sys) = system {
            parts.push(serde_json::json!({
                "role": "user",
                "parts": [{ "text": sys }]
            }));
            parts.push(serde_json::json!({
                "role": "model",
                "parts": [{ "text": "Understood." }]
            }));
        }
        parts.push(serde_json::json!({
            "role": "user",
            "parts": [{ "text": prompt }]
        }));

        let body = serde_json::json!({ "contents": parts });

        let resp = self.client.post(&url).json(&body).send()
            .map_err(|e| anyhow!("Gemini request failed: {}", e))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("Gemini HTTP {}: {}", status, body));
        }

        #[derive(serde::Deserialize)]
        struct Part { text: String }
        #[derive(serde::Deserialize)]
        struct Content { parts: Vec<Part> }
        #[derive(serde::Deserialize)]
        struct Candidate { content: Content }
        #[derive(serde::Deserialize)]
        struct Resp { candidates: Vec<Candidate> }

        let data: Resp = resp.json()
            .map_err(|e| anyhow!("Gemini response parse error: {}", e))?;

        data.candidates
            .into_iter()
            .next()
            .and_then(|c| c.content.parts.into_iter().next())
            .map(|p| p.text.trim().to_string())
            .ok_or_else(|| anyhow!("Gemini returned empty response"))
    }
}

// ── 3. Claude (last resort) ───────────────────────────────────────────────────

pub struct ClaudeInferenceProvider {
    api_key: String,
    model:   String,
    client:  reqwest::blocking::Client,
}

impl ClaudeInferenceProvider {
    pub fn new(api_key: String, model: String) -> Result<Self> {
        if api_key.is_empty() {
            return Err(anyhow!("Claude API key is empty"));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        log::info!("[Inference] Claude configured (last-resort): model={}", model);
        Ok(Self { api_key, model, client })
    }
}

impl InferenceProvider for ClaudeInferenceProvider {
    fn provider_id(&self) -> &str { "claude-cloud" }

    fn complete(&self, system: Option<&str>, prompt: &str) -> Result<String> {
        let url = "https://api.anthropic.com/v1/messages";

        let mut body = serde_json::json!({
            "model":      self.model,
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }],
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }

        let resp = self.client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|e| anyhow!("Claude request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("Claude HTTP {}: {}", status, body));
        }

        #[derive(serde::Deserialize)]
        struct TextBlock { text: String }
        #[derive(serde::Deserialize)]
        struct Resp { content: Vec<TextBlock> }

        let data: Resp = resp.json()
            .map_err(|e| anyhow!("Claude response parse error: {}", e))?;

        data.content
            .into_iter()
            .next()
            .map(|b| b.text.trim().to_string())
            .ok_or_else(|| anyhow!("Claude returned empty response"))
    }
}

// ── Priority chain builder ─────────────────────────────────────────────────────

/// Build the highest-priority available `InferenceProvider` respecting `ai_policy`.
///
/// Provider selection order:
///   1. Local Ollama  — always tried when `ai_provider_url` + `ai_model` are set.
///   2. Google Gemini — tried when policy allows cloud AND `google_gemini_api_key` is set.
///   3. Claude        — tried as absolute last resort when policy allows cloud AND key is set.
///
/// Returns `None` when `ai_policy = "local_only"` and Ollama is unavailable,
/// signalling the caller to degrade gracefully without any inference.
pub fn build_inference_provider(
    ai_policy: &str,
    ai_provider_url: Option<&str>,
    ai_model: Option<&str>,
    google_gemini_api_key: Option<&str>,
    google_gemini_model: &str,
    claude_api_key: Option<&str>,
    claude_model: &str,
) -> Option<Arc<dyn InferenceProvider>> {
    // ── Tier 1: local Ollama ──────────────────────────────────────────────
    if let (Some(url), Some(model)) = (ai_provider_url, ai_model) {
        match OllamaInferenceProvider::new(model.to_string(), url.to_string()) {
            Ok(p) => {
                log::info!("[Inference] Using local Ollama (policy={})", ai_policy);
                return Some(Arc::new(p));
            }
            Err(e) => {
                log::warn!("[Inference] Local Ollama unavailable: {}", e);
            }
        }
    }

    // ── Tier 2 & 3: cloud — only if policy allows ─────────────────────────
    if !crate::cloud_allowed(ai_policy) {
        log::info!("[Inference] No local inference available and policy={} — degrading gracefully.", ai_policy);
        return None;
    }

    // ── Tier 2: Google Gemini ─────────────────────────────────────────────
    if let Some(key) = google_gemini_api_key.filter(|k| !k.is_empty()) {
        match GeminiInferenceProvider::new(key.to_string(), google_gemini_model.to_string()) {
            Ok(p) => {
                log::info!("[Inference] Falling back to Google Gemini (policy={})", ai_policy);
                return Some(Arc::new(p));
            }
            Err(e) => {
                log::warn!("[Inference] Gemini init failed: {}", e);
            }
        }
    }

    // ── Tier 3: Claude ────────────────────────────────────────────────────
    if let Some(key) = claude_api_key.filter(|k| !k.is_empty()) {
        match ClaudeInferenceProvider::new(key.to_string(), claude_model.to_string()) {
            Ok(p) => {
                log::warn!(
                    "[Inference] Last-resort fallback: Claude (policy={}). \
                     Consider pulling a local model to avoid this.",
                    ai_policy
                );
                return Some(Arc::new(p));
            }
            Err(e) => {
                log::error!("[Inference] Claude init also failed: {}", e);
            }
        }
    }

    log::error!("[Inference] No inference provider available (all tiers exhausted).");
    None
}
