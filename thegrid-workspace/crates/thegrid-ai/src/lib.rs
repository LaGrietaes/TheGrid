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
}

impl HttpEmbeddingProvider {
    pub fn new(model_name: String, base_url: String) -> Self {
        log::info!("[AI] Initializing HTTP AI provider: {} at {}", model_name, base_url);
        Self {
            model_name,
            base_url,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl EmbeddingProvider for HttpEmbeddingProvider {
    fn dimensions(&self) -> usize { 384 } // Common for many models, should be configurable
    fn model_id(&self) -> &str { &self.model_name }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Simple Ollama-style embedding request
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
        let data: OllamaResp = resp.json().map_err(|e| anyhow::anyhow!("Failed to parse AI response: {}", e))?;
        Ok(data.embedding)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::new();
        for t in texts {
            results.push(self.embed(t)?);
        }
        Ok(results)
    }
}

/// Mock implementation of VectorIndex.
pub struct USearchIndex {
    _dims: usize,
}

impl USearchIndex {
    pub fn new(dims: usize) -> Result<Self> {
        Ok(Self { _dims: dims })
    }
    pub fn add(&mut self, _file_id: i64, _vector: &[f32]) -> Result<()> {
        Ok(())
    }
    pub fn search(&self, _query: &[f32], _k: usize) -> Result<Vec<(i64, f32)>> {
        Ok(Vec::new())
    }
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
    _index: USearchIndex,
}

impl SemanticSearch {
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        let dims = provider.dimensions();
        let index = USearchIndex::new(dims)?;
        Ok(Self { provider, _index: index })
    }
    pub fn model_id(&self) -> &str {
        self.provider.model_id()
    }
    pub fn index_file(&mut self, _file_id: i64, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 384])
    }
    pub fn add_vector(&mut self, _file_id: i64, _vector: &[f32]) -> Result<()> {
        Ok(())
    }
    pub fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 384])
    }
    pub fn search(&self, _query: &str, _k: usize) -> Result<Vec<(i64, f32)>> {
        Ok(Vec::new())
    }
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

