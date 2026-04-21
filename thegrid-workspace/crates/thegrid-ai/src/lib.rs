use anyhow::Result;
use sysinfo::System;
use std::sync::Arc;

/// Capability trait for any embedding model.
pub trait EmbeddingProvider: Send + Sync {
    fn dimensions(&self) -> usize;
    fn model_id(&self) -> &str;
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}

/// Mock implementation of EmbeddingProvider.
pub struct FastEmbedProvider {
    model_name: String,
}

impl FastEmbedProvider {
    pub fn new() -> Result<Self> {
        log::info!("[AI] Using local FastEmbed (stubbed for now).");
        Ok(Self {
            model_name: "mock-MiniLM-L12-v2".to_string(),
        })
    }
}

impl EmbeddingProvider for FastEmbedProvider {
    fn dimensions(&self) -> usize { 384 }
    fn model_id(&self) -> &str { &self.model_name }
    fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 384])
    }
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(vec![vec![0.0; 384]; texts.len()])
    }
}

/// Provider that talks to an external HTTP AI server (Ollama, llama.cpp, etc.)
pub struct HttpEmbeddingProvider {
    model_name: String,
    base_url: String,
    client: reqwest::blocking::Client,
    dims: usize,
}

impl HttpEmbeddingProvider {
    /// Connect to the Ollama endpoint, confirm reachability, and probe actual vector dimensions.
    pub fn new(model_name: String, base_url: String) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let probe_url = format!("{}/api/embeddings", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": model_name,
            "prompt": "probe",
        });

        let resp = client.post(&probe_url)
            .json(&body)
            .send()
            .map_err(|e| anyhow::anyhow!("Ollama not reachable at {}: {}", base_url, e))?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Ollama returned {} for model '{}'. Run: ollama pull {}",
                resp.status(), model_name, model_name
            ));
        }

        #[derive(serde::Deserialize)]
        struct OllamaResp { embedding: Vec<f32> }
        let data: OllamaResp = resp.json()
            .map_err(|e| anyhow::anyhow!("Bad Ollama probe response: {}", e))?;

        let dims = data.embedding.len();
        if dims == 0 {
            return Err(anyhow::anyhow!("Ollama returned empty embedding for '{}'", model_name));
        }

        log::info!("[AI] Ollama ready: model={} dims={} url={}", model_name, dims, base_url);
        Ok(Self { model_name, base_url, client, dims })
    }
}

impl EmbeddingProvider for HttpEmbeddingProvider {
    fn dimensions(&self) -> usize { self.dims }
    fn model_id(&self) -> &str { &self.model_name }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model_name,
            "prompt": text,
        });

        let resp = self.client.post(&url)
            .json(&body)
            .send()
            .map_err(|e| anyhow::anyhow!("HTTP AI request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("HTTP AI returned status {}", resp.status()));
        }

        #[derive(serde::Deserialize)]
        struct OllamaResp { embedding: Vec<f32> }
        let data: OllamaResp = resp.json()
            .map_err(|e| anyhow::anyhow!("Failed to parse AI response: {}", e))?;
        Ok(data.embedding)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// Probe a local Ollama instance and return the list of available model names.
/// Returns `None` if Ollama is not running or not reachable.
pub fn probe_ollama_models(base_url: &str) -> Option<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().ok()?;

    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let resp = client.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }

    #[derive(serde::Deserialize)]
    struct Model { name: String }
    #[derive(serde::Deserialize)]
    struct TagsResp { models: Vec<Model> }

    let data: TagsResp = resp.json().ok()?;
    Some(data.models.into_iter().map(|m| m.name).collect())
}

/// Pick the best embedding-capable model from an available Ollama model list.
/// Preference order: nomic-embed-text > mxbai-embed-large > all-minilm > any *embed* > first.
pub fn pick_best_embed_model(models: &[String]) -> Option<String> {
    let preferred = ["nomic-embed-text", "mxbai-embed-large", "all-minilm"];
    for p in &preferred {
        if let Some(m) = models.iter().find(|m| m.starts_with(p)) {
            return Some(m.clone());
        }
    }
    models.iter().find(|m| m.contains("embed")).cloned()
        .or_else(|| models.first().cloned())
}

/// Pure-Rust in-memory vector index using pre-normalized cosine similarity.
/// Suitable for local file indexes (up to ~1M vectors on a modern CPU).
pub struct VectorIndex {
    dims: usize,
    entries: Vec<(i64, Vec<f32>)>, // (file_id, L2-normalized vector)
}

impl VectorIndex {
    pub fn new(dims: usize) -> Result<Self> {
        Ok(Self { dims, entries: Vec::new() })
    }

    /// Upsert a vector for a file. Normalizes to unit length for cosine similarity.
    pub fn add(&mut self, file_id: i64, vector: &[f32]) -> Result<()> {
        if vector.is_empty() { return Ok(()); }
        // Accept dimension mismatch on first add (model may differ from init probe)
        if self.entries.is_empty() {
            self.dims = vector.len();
        } else if vector.len() != self.dims {
            // Skip silently — model change mid-session; caller should reinit
            return Ok(());
        }

        let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let normalized: Vec<f32> = if norm > 1e-9 {
            vector.iter().map(|x| x / norm).collect()
        } else {
            vector.to_vec()
        };

        if let Some(entry) = self.entries.iter_mut().find(|(id, _)| *id == file_id) {
            entry.1 = normalized;
        } else {
            self.entries.push((file_id, normalized));
        }
        Ok(())
    }

    /// Return top-k (file_id, cosine_score) pairs, sorted descending.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        if self.entries.is_empty() || k == 0 { return Ok(Vec::new()); }

        let norm: f32 = query.iter().map(|x| x * x).sum::<f32>().sqrt();
        let q: Vec<f32> = if norm > 1e-9 {
            query.iter().map(|x| x / norm).collect()
        } else {
            query.to_vec()
        };

        let mut scores: Vec<(i64, f32)> = self.entries.iter()
            .map(|(id, v)| {
                let score: f32 = v.iter().zip(q.iter()).map(|(a, b)| a * b).sum();
                (*id, score)
            })
            .collect();

        scores.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(k);
        Ok(scores)
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

pub struct AiNodeDetector {
    sys: System,
}

impl AiNodeDetector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self { sys }
    }

    pub fn is_ai_node(&self) -> bool {
        let total_ram_gb = self.sys.total_memory() / (1024 * 1024 * 1024);
        total_ram_gb >= 8 || self.has_nvidia_gpu()
    }

    pub fn has_nvidia_gpu(&self) -> bool {
        // Simple check for nvidia-smi presence and output
        let output = std::process::Command::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output();

        match output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout);
                s.contains("RTX") || s.contains("GTX") || s.contains("NVIDIA")
            }
            Err(_) => false,
        }
    }

    pub fn gpu_info(&self) -> Option<String> {
        let output = std::process::Command::new("nvidia-smi")
            .arg("--query-gpu=name,memory.total")
            .arg("--format=csv,noheader")
            .output();

        match output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            }
            Err(_) => None,
        }
    }

    pub fn capability_label(&self) -> String {
        let mut label = if self.is_ai_node() { "AI NODE" } else { "STANDARD" }.to_string();
        if let Some(gpu) = self.gpu_info() {
            label = format!("{} ({})", label, gpu);
        }
        label
    }
}

impl Default for AiNodeDetector {
    fn default() -> Self { Self::new() }
}

pub struct SemanticSearch {
    provider: Arc<dyn EmbeddingProvider>,
    index: VectorIndex,
}

impl SemanticSearch {
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        let dims = provider.dimensions();
        let index = VectorIndex::new(dims)?;
        Ok(Self { provider, index })
    }
    pub fn model_id(&self) -> &str {
        self.provider.model_id()
    }
    /// Embed `text`, store the vector in the index, and return the raw vector.
    pub fn index_file(&mut self, file_id: i64, text: &str) -> Result<Vec<f32>> {
        let vector = self.provider.embed(text)?;
        self.index.add(file_id, &vector)?;
        Ok(vector)
    }
    /// Insert a pre-computed vector (e.g. loaded from DB on startup).
    pub fn add_vector(&mut self, file_id: i64, vector: &[f32]) -> Result<()> {
        self.index.add(file_id, vector)
    }
    /// Embed a query string and return the raw vector.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.provider.embed(text)
    }
    /// Semantic search: embed query, return top-k (file_id, score) pairs.
    pub fn search(&self, query: &str, k: usize) -> Result<Vec<(i64, f32)>> {
        let q_vec = self.provider.embed(query)?;
        self.index.search(&q_vec, k)
    }
    pub fn vector_count(&self) -> usize { self.index.len() }
}

/// Metadata extracted from media files (images, video) using AI models.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct MediaMetadata {
    pub description: String,
    pub tags: Vec<String>,
    pub dominant_colors: Vec<String>,
    pub face_count: usize,
    pub ocr_text: Option<String>,
}

/// Trait for analyzing media files.
pub trait MediaAnalyzer: Send + Sync {
    fn analyze(&self, path: &std::path::Path) -> Result<MediaMetadata>;
}

/// GPU-accelerated media analyzer (placeholder for actual CUDA/cuDNN logic).
pub struct CudaMediaAnalyzer {
    gpu_name: String,
}

impl CudaMediaAnalyzer {
    pub fn new() -> Result<Self> {
        let detector = AiNodeDetector::new();
        if let Some(gpu) = detector.gpu_info() {
            log::info!("[AI] Initializing CUDA Media Analyzer on {}", gpu);
            Ok(Self { gpu_name: gpu })
        } else {
            Err(anyhow::anyhow!("No CUDA-capable GPU found for high-performance indexing."))
        }
    }
}

impl MediaAnalyzer for CudaMediaAnalyzer {
    fn analyze(&self, path: &std::path::Path) -> Result<MediaMetadata> {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut meta = MediaMetadata::default();

        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "webp" => {
                // Real image metadata via the `image` crate — no GPU needed for this layer.
                match image::image_dimensions(path) {
                    Ok((w, h)) => {
                        let mp = (w as f64 * h as f64) / 1_000_000.0;
                        meta.description = format!("{}x{} ({:.1}MP)", w, h, mp);
                        meta.tags = vec![
                            ext.to_string(),
                            format!("{}x{}", w, h),
                            if mp >= 8.0 { "high-res".into() } else if mp >= 2.0 { "mid-res".into() } else { "low-res".into() },
                        ];
                    }
                    Err(e) => {
                        log::warn!("[AI] Could not read image dimensions for {:?}: {}", path, e);
                        meta.description = format!("{} (unreadable)", ext);
                        meta.tags = vec![ext.to_string()];
                    }
                }
            }
            "mp4" | "mkv" | "mov" | "avi" => {
                // No GPU video decode yet — record container format and file size.
                let size_mb = path.metadata().map(|m| m.len() / 1_048_576).unwrap_or(0);
                meta.description = format!("{} container, ~{}MB", ext.to_ascii_uppercase(), size_mb);
                meta.tags = vec![ext.to_string(), "video".into()];
            }
            _ => {
                meta.description = format!("Unsupported media type: {}", ext);
            }
        }

        log::debug!("[AI] [{}] Analyzed {:?}: {}", self.gpu_name, path, meta.description);
        Ok(meta)
    }
}

