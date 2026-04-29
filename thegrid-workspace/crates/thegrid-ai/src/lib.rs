use anyhow::Result;
use sysinfo::System;
use std::sync::Arc;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::BufReader;

/// Capability trait for any embedding model.
pub trait EmbeddingProvider: Send + Sync {
    fn dimensions(&self) -> usize;
    fn model_id(&self) -> &str;
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}

/// Local deterministic embedding provider.
///
/// This is not a transformer model, but it generates stable semantic-ish vectors
/// using token hashing, weighting, and normalization so semantic search remains
/// functional even without a remote AI server.
pub struct FastEmbedProvider {
    model_name: String,
}

impl FastEmbedProvider {
    pub fn new() -> Result<Self> {
        log::info!("[AI] Using local deterministic embedding provider.");
        Ok(Self {
            model_name: "local-hash-embed-v1".to_string(),
        })
    }

    fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
            .collect()
    }

    fn hash_to_index(token: &str, salt: u64, dims: usize) -> (usize, f32) {
        let mut h = DefaultHasher::new();
        token.hash(&mut h);
        salt.hash(&mut h);
        let value = h.finish();
        let idx = (value as usize) % dims;
        let sign = if (value & (1 << 63)) != 0 { -1.0 } else { 1.0 };
        (idx, sign)
    }

    fn normalize(v: &mut [f32]) {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-8 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
    }

    fn embed_one(text: &str, dims: usize) -> Vec<f32> {
        let tokens = Self::tokenize(text);
        let mut vec = vec![0.0f32; dims];

        if tokens.is_empty() {
            return vec;
        }

        // Multi-hash feature projection with position decay and token-length boost.
        for (i, tok) in tokens.iter().enumerate() {
            let pos_weight = 1.0 / ((i + 1) as f32).sqrt();
            let len_weight = ((tok.len().min(12) as f32) / 12.0).max(0.25);
            let w = pos_weight * len_weight;

            let (a, sa) = Self::hash_to_index(tok, 0x9E3779B185EBCA87, dims);
            let (b, sb) = Self::hash_to_index(tok, 0xC2B2AE3D27D4EB4F, dims);
            let (c, sc) = Self::hash_to_index(tok, 0x165667B19E3779F9, dims);

            vec[a] += sa * w;
            vec[b] += sb * w * 0.7;
            vec[c] += sc * w * 0.4;

            // Character bigram projection to improve partial-context matching.
            let chars: Vec<char> = tok.chars().collect();
            for bg in chars.windows(2) {
                let bg_s: String = bg.iter().collect();
                let (idx, sgn) = Self::hash_to_index(&bg_s, 0xA24BAED4963EE407, dims);
                vec[idx] += sgn * 0.08;
            }
        }

        Self::normalize(&mut vec);
        vec
    }
}

impl EmbeddingProvider for FastEmbedProvider {
    fn dimensions(&self) -> usize { 384 }
    fn model_id(&self) -> &str { &self.model_name }
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(Self::embed_one(text, self.dimensions()))
    }
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| Self::embed_one(t, self.dimensions())).collect())
    }
}

/// Wire format used by the remote embedding server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFormat {
    /// Ollama: POST /api/embeddings {model, prompt} → {embedding: [...]}
    Ollama,
    /// OpenAI-compatible: POST /v1/embeddings {model, input} → {data:[{embedding:[...]}]}
    OpenAi,
}

/// Provider that talks to an external HTTP AI server (Ollama, LocalAI, llama.cpp, etc.)
pub struct HttpEmbeddingProvider {
    model_name: String,
    base_url: String,
    client: reqwest::blocking::Client,
    dims: usize,
    format: ApiFormat,
}

impl HttpEmbeddingProvider {
    /// Connect to an embedding endpoint, auto-detect format, and probe actual vector dimensions.
    /// Tries OpenAI-compat first, then Ollama format.
    pub fn new(model_name: String, base_url: String) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        // Try OpenAI-compat format first (/v1/embeddings)
        if let Ok((dims, format)) = Self::probe_format(&client, &model_name, &base_url, ApiFormat::OpenAi) {
            if dims > 0 {
                log::info!("[AI] LocalAI/OpenAI-compat ready: model={} dims={} url={}", model_name, dims, base_url);
                return Ok(Self { model_name, base_url, client, dims, format });
            }
        }

        // Fall back to Ollama format (/api/embeddings)
        let (dims, format) = Self::probe_format(&client, &model_name, &base_url, ApiFormat::Ollama)
            .map_err(|e| anyhow::anyhow!("AI server not reachable at {}: {}", base_url, e))?;

        if dims == 0 {
            return Err(anyhow::anyhow!(
                "AI server returned empty embedding for '{}'. Check model config.",
                model_name
            ));
        }

        log::info!("[AI] Ollama ready: model={} dims={} url={}", model_name, dims, base_url);
        Ok(Self { model_name, base_url, client, dims, format })
    }

    fn probe_format(client: &reqwest::blocking::Client, model_name: &str, base_url: &str, format: ApiFormat) -> Result<(usize, ApiFormat)> {
        let vec = Self::do_embed_with(client, model_name, base_url, "probe", format)?;
        Ok((vec.len(), format))
    }

    fn do_embed_with(
        client: &reqwest::blocking::Client,
        model_name: &str,
        base_url: &str,
        text: &str,
        format: ApiFormat,
    ) -> Result<Vec<f32>> {
        let base = base_url.trim_end_matches('/');
        match format {
            ApiFormat::OpenAi => {
                let url = format!("{}/v1/embeddings", base);
                let body = serde_json::json!({ "model": model_name, "input": text });
                let resp = client.post(&url).json(&body).send()
                    .map_err(|e| anyhow::anyhow!("OpenAI embed request failed: {}", e))?;
                if !resp.status().is_success() {
                    return Err(anyhow::anyhow!("OpenAI embed HTTP {}", resp.status()));
                }
                #[derive(serde::Deserialize)]
                struct EmbObj { embedding: Vec<f32> }
                #[derive(serde::Deserialize)]
                struct OpenAiResp { data: Vec<EmbObj> }
                let data: OpenAiResp = resp.json()
                    .map_err(|e| anyhow::anyhow!("Bad OpenAI embed response: {}", e))?;
                Ok(data.data.into_iter().next().map(|e| e.embedding).unwrap_or_default())
            }
            ApiFormat::Ollama => {
                let url = format!("{}/api/embeddings", base);
                let body = serde_json::json!({ "model": model_name, "prompt": text });
                let resp = client.post(&url).json(&body).send()
                    .map_err(|e| anyhow::anyhow!("Ollama embed request failed: {}", e))?;
                if !resp.status().is_success() {
                    return Err(anyhow::anyhow!("Ollama embed HTTP {}", resp.status()));
                }
                #[derive(serde::Deserialize)]
                struct OllamaResp { embedding: Vec<f32> }
                let data: OllamaResp = resp.json()
                    .map_err(|e| anyhow::anyhow!("Bad Ollama embed response: {}", e))?;
                Ok(data.embedding)
            }
        }
    }
}

impl EmbeddingProvider for HttpEmbeddingProvider {
    fn dimensions(&self) -> usize { self.dims }
    fn model_id(&self) -> &str { &self.model_name }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Self::do_embed_with(&self.client, &self.model_name, &self.base_url, text, self.format)
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

/// Probe a LocalAI / OpenAI-compatible instance and return the list of model ids.
/// Returns `None` if the server is not running or not reachable.
pub fn probe_localai_models(base_url: &str) -> Option<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build().ok()?;

    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = client.get(&url).send().ok()?;
    if !resp.status().is_success() { return None; }

    #[derive(serde::Deserialize)]
    struct ModelObj { id: String }
    #[derive(serde::Deserialize)]
    struct ModelsResp { data: Vec<ModelObj> }

    let data: ModelsResp = resp.json().ok()?;
    let ids: Vec<String> = data.data.into_iter().map(|m| m.id).collect();
    if ids.is_empty() { None } else { Some(ids) }
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

/// Pick an embedding model suitable for tablet/edge nodes where thermal and
/// sustained power limits matter. Prefer compact models first.
pub fn pick_tablet_embed_model(models: &[String]) -> Option<String> {
    let lower: Vec<(String, String)> = models
        .iter()
        .map(|m| (m.clone(), m.to_ascii_lowercase()))
        .collect();

    // Preference order for tablet-class hardware.
    let preferred_substrings = [
        "all-minilm",
        "bge-small",
        "e5-small",
        "nomic-embed-text",
        "mxbai-embed-large",
    ];

    for p in preferred_substrings {
        if let Some((orig, _)) = lower.iter().find(|(_, m)| m.contains(p)) {
            return Some(orig.clone());
        }
    }

    // Generic fallback: any embedding model id.
    lower
        .iter()
        .find(|(_, m)| m.contains("embed"))
        .map(|(orig, _)| orig.clone())
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

    pub fn provider(&self) -> Arc<dyn EmbeddingProvider> {
        Arc::clone(&self.provider)
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
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub megapixels: Option<f32>,
    pub focus_score: Option<f32>,
    pub quality_score: Option<f32>,
    pub brightness: Option<f32>,
    pub contrast: Option<f32>,
    pub in_focus: Option<bool>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<u32>,
    pub aperture: Option<f32>,
    pub shutter_seconds: Option<f32>,
    pub focal_length_mm: Option<f32>,
    pub captured_at: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    /// Duration in seconds (MP4/MOV containers)
    pub duration_secs: Option<f64>,
    /// Video codec or container short name
    pub video_codec: Option<String>,
}

/// Trait for analyzing media files.
pub trait MediaAnalyzer: Send + Sync {
    fn analyze(&self, path: &std::path::Path) -> Result<MediaMetadata>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaProcessingMode {
    Auto,
    Cpu,
    DedicatedGpu,
}

impl MediaProcessingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
            Self::DedicatedGpu => "dedicated_gpu",
        }
    }
}

/// Media analyzer that runs locally. If NVIDIA is present it reports the GPU
/// in telemetry, but analysis itself is CPU-based for portability.
pub struct CudaMediaAnalyzer {
    gpu_name: String,
    mode: MediaProcessingMode,
}

impl CudaMediaAnalyzer {
    pub fn new() -> Result<Self> {
        Self::new_with_mode(MediaProcessingMode::Auto)
    }

    pub fn new_with_mode(mode: MediaProcessingMode) -> Result<Self> {
        let detector = AiNodeDetector::new();
        let gpu_info = detector.gpu_info();
        let effective_mode = match mode {
            MediaProcessingMode::Auto => {
                if detector.has_nvidia_gpu() {
                    MediaProcessingMode::DedicatedGpu
                } else {
                    MediaProcessingMode::Cpu
                }
            }
            forced => forced,
        };

        let gpu = gpu_info.unwrap_or_else(|| "cpu".to_string());
        log::info!(
            "[AI] Initializing Media Analyzer backend={} mode={}",
            gpu,
            effective_mode.as_str()
        );
        Ok(Self { gpu_name: gpu, mode: effective_mode })
    }

    pub fn processing_mode(&self) -> MediaProcessingMode {
        self.mode
    }

    fn try_cuda_python(path: &std::path::Path) -> Option<MediaMetadata> {
        let script_path = std::env::var("THEGRID_CUDA_ANALYZER_SCRIPT")
            .ok()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join("scripts")
                    .join("cuda_media_analyze.py")
            });

        if !script_path.exists() {
            return None;
        }

        let output = std::process::Command::new("python")
            .arg(script_path)
            .arg(path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str::<MediaMetadata>(&stdout).ok()
    }

    fn dominant_colors(img: &image::RgbImage, top_k: usize) -> Vec<String> {
        let mut bins = std::collections::HashMap::<u16, u32>::new();
        for p in img.pixels() {
            // 4-bit/channel quantization => 4096 bins
            let r = (p[0] >> 4) as u16;
            let g = (p[1] >> 4) as u16;
            let b = (p[2] >> 4) as u16;
            let key = (r << 8) | (g << 4) | b;
            *bins.entry(key).or_insert(0) += 1;
        }

        let mut sorted: Vec<(u16, u32)> = bins.into_iter().collect();
        sorted.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(top_k);

        sorted
            .into_iter()
            .map(|(k, _)| {
                let r = (((k >> 8) & 0x0F) as u8) * 17;
                let g = (((k >> 4) & 0x0F) as u8) * 17;
                let b = ((k & 0x0F) as u8) * 17;
                format!("#{:02X}{:02X}{:02X}", r, g, b)
            })
            .collect()
    }

    fn grayscale_stats(gray: &image::GrayImage) -> (f32, f32) {
        let n = (gray.width() as f32) * (gray.height() as f32);
        if n <= 1.0 {
            return (0.0, 0.0);
        }
        let mean = gray.pixels().map(|p| p[0] as f32).sum::<f32>() / n;
        let var = gray
            .pixels()
            .map(|p| {
                let d = (p[0] as f32) - mean;
                d * d
            })
            .sum::<f32>()
            / (n - 1.0);
        (mean / 255.0, var.sqrt() / 255.0)
    }

    fn focus_score(gray: &image::GrayImage) -> f32 {
        let (w, h) = gray.dimensions();
        if w < 3 || h < 3 {
            return 0.0;
        }

        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut count = 0.0f64;

        for y in 1..(h - 1) {
            for x in 1..(w - 1) {
                let c = gray.get_pixel(x, y)[0] as f64;
                let l = gray.get_pixel(x - 1, y)[0] as f64;
                let r = gray.get_pixel(x + 1, y)[0] as f64;
                let u = gray.get_pixel(x, y - 1)[0] as f64;
                let d = gray.get_pixel(x, y + 1)[0] as f64;
                let lap = 4.0 * c - l - r - u - d;
                sum += lap;
                sum_sq += lap * lap;
                count += 1.0;
            }
        }

        if count <= 1.0 {
            return 0.0;
        }

        let mean = sum / count;
        let var = (sum_sq / count) - (mean * mean);
        // Normalize empirically; clamp to [0, 1].
        (var as f32 / 1200.0).clamp(0.0, 1.0)
    }

    fn estimate_noise(gray: &image::GrayImage) -> f32 {
        let (w, h) = gray.dimensions();
        if w < 2 || h < 2 {
            return 0.0;
        }

        let mut acc = 0.0f32;
        let mut count = 0.0f32;
        for y in 0..(h - 1) {
            for x in 0..(w - 1) {
                let p = gray.get_pixel(x, y)[0] as f32;
                let px = gray.get_pixel(x + 1, y)[0] as f32;
                let py = gray.get_pixel(x, y + 1)[0] as f32;
                acc += (p - px).abs() + (p - py).abs();
                count += 2.0;
            }
        }
        if count <= 0.0 {
            0.0
        } else {
            (acc / count / 255.0).clamp(0.0, 1.0)
        }
    }

    fn classify_exposure(brightness: f32) -> &'static str {
        if brightness < 0.25 {
            "underexposed"
        } else if brightness > 0.82 {
            "overexposed"
        } else {
            "well-exposed"
        }
    }

    fn exif_ascii(field: &exif::Field) -> Option<String> {
        if let exif::Value::Ascii(parts) = &field.value {
            let mut out = String::new();
            for part in parts {
                if let Ok(s) = std::str::from_utf8(part) {
                    out.push_str(s);
                }
            }
            let trimmed = out.trim_matches('\0').trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        } else {
            None
        }
    }

    fn exif_u32(field: &exif::Field) -> Option<u32> {
        match &field.value {
            exif::Value::Short(v) => v.first().copied().map(|n| n as u32),
            exif::Value::Long(v) => v.first().copied(),
            exif::Value::SLong(v) => v.first().copied().and_then(|n| u32::try_from(n).ok()),
            exif::Value::Rational(v) => v.first().and_then(|r| {
                if r.denom == 0 {
                    None
                } else {
                    Some((r.num as f64 / r.denom as f64).round() as u32)
                }
            }),
            _ => None,
        }
    }

    fn exif_f32(field: &exif::Field) -> Option<f32> {
        match &field.value {
            exif::Value::Rational(v) => v.first().and_then(|r| {
                if r.denom == 0 {
                    None
                } else {
                    Some(r.num as f32 / r.denom as f32)
                }
            }),
            exif::Value::SRational(v) => v.first().and_then(|r| {
                if r.denom == 0 {
                    None
                } else {
                    Some(r.num as f32 / r.denom as f32)
                }
            }),
            exif::Value::Short(v) => v.first().copied().map(|n| n as f32),
            exif::Value::Long(v) => v.first().copied().map(|n| n as f32),
            _ => None,
        }
    }

    fn parse_exif_datetime(raw: &str) -> String {
        // EXIF typical: YYYY:MM:DD HH:MM:SS -> normalize to ISO-like string for lexical filtering.
        let trimmed = raw.trim();
        if trimmed.len() >= 19 {
            let mut chars: Vec<char> = trimmed.chars().collect();
            if chars.len() >= 19 {
                chars[4] = '-';
                chars[7] = '-';
                chars[10] = 'T';
                return chars.into_iter().collect();
            }
        }
        trimmed.to_string()
    }

    fn gps_to_decimal(coord: &exif::Field, dir: Option<&exif::Field>) -> Option<f64> {
        let values = match &coord.value {
            exif::Value::Rational(v) => v,
            _ => return None,
        };

        if values.len() < 3 {
            return None;
        }

        let to_f64 = |r: exif::Rational| -> Option<f64> {
            if r.denom == 0 { None } else { Some(r.num as f64 / r.denom as f64) }
        };

        let deg = to_f64(values[0])?;
        let min = to_f64(values[1])?;
        let sec = to_f64(values[2])?;
        let mut dec = deg + (min / 60.0) + (sec / 3600.0);

        if let Some(d) = dir.and_then(Self::exif_ascii) {
            let d = d.to_ascii_uppercase();
            if d == "S" || d == "W" {
                dec = -dec;
            }
        }

        Some(dec)
    }

    fn inject_exif(meta: &mut MediaMetadata, path: &std::path::Path) {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let mut reader = BufReader::new(file);
        let exif = match exif::Reader::new().read_from_container(&mut reader) {
            Ok(v) => v,
            Err(_) => return,
        };

        let make = exif
            .get_field(exif::Tag::Make, exif::In::PRIMARY)
            .and_then(Self::exif_ascii);
        let model = exif
            .get_field(exif::Tag::Model, exif::In::PRIMARY)
            .and_then(Self::exif_ascii);
        let lens = exif
            .get_field(exif::Tag::LensModel, exif::In::PRIMARY)
            .and_then(Self::exif_ascii);
        let iso = exif
            .get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
            .and_then(Self::exif_u32)
            .or_else(|| {
                exif.get_field(exif::Tag::ISOSpeed, exif::In::PRIMARY)
                    .and_then(Self::exif_u32)
            });
        let aperture = exif
            .get_field(exif::Tag::FNumber, exif::In::PRIMARY)
            .and_then(Self::exif_f32);
        let shutter = exif
            .get_field(exif::Tag::ExposureTime, exif::In::PRIMARY)
            .and_then(Self::exif_f32);
        let focal = exif
            .get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
            .and_then(Self::exif_f32);
        let captured = exif
            .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
            .and_then(Self::exif_ascii)
            .map(|s| Self::parse_exif_datetime(&s));

        let gps_lat = exif
            .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
            .and_then(|lat| {
                Self::gps_to_decimal(
                    lat,
                    exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY),
                )
            });
        let gps_lon = exif
            .get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)
            .and_then(|lon| {
                Self::gps_to_decimal(
                    lon,
                    exif.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY),
                )
            });

        meta.camera_make = make;
        meta.camera_model = model;
        meta.lens_model = lens;
        meta.iso = iso;
        meta.aperture = aperture;
        meta.shutter_seconds = shutter;
        meta.focal_length_mm = focal;
        meta.captured_at = captured;
        meta.gps_lat = gps_lat;
        meta.gps_lon = gps_lon;

        if let Some(make) = &meta.camera_make {
            meta.tags.push(format!("make:{}", make.to_ascii_lowercase()));
        }
        if let Some(model) = &meta.camera_model {
            meta.tags.push(format!("camera:{}", model.to_ascii_lowercase()));
        }
        if let Some(lens) = &meta.lens_model {
            meta.tags.push(format!("lens:{}", lens.to_ascii_lowercase()));
        }
        if let Some(iso) = meta.iso {
            meta.tags.push(format!("iso:{}", iso));
        }
        if let Some(ap) = meta.aperture {
            meta.tags.push(format!("f/{:.1}", ap));
        }
        if let Some(fl) = meta.focal_length_mm {
            meta.tags.push(format!("{}mm", fl.round() as i32));
        }
        if meta.gps_lat.is_some() && meta.gps_lon.is_some() {
            meta.tags.push("geotagged".into());
        }
    }
}

impl MediaAnalyzer for CudaMediaAnalyzer {
    fn analyze(&self, path: &std::path::Path) -> Result<MediaMetadata> {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut meta = MediaMetadata::default();

        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "webp" => {
                if matches!(self.mode, MediaProcessingMode::DedicatedGpu) {
                    if let Some(mut gpu_meta) = Self::try_cuda_python(path) {
                        if gpu_meta.description.is_empty() {
                            gpu_meta.description = "CUDA analyzer result".to_string();
                        }
                        if !gpu_meta.tags.iter().any(|t| t == "cuda") {
                            gpu_meta.tags.push("cuda".to_string());
                        }
                        Self::inject_exif(&mut gpu_meta, path);
                        return Ok(gpu_meta);
                    }
                }

                let dyn_img = image::open(path)?;
                let rgb = dyn_img.to_rgb8();
                let (w, h) = rgb.dimensions();
                let mp = (w as f32 * h as f32) / 1_000_000.0;

                // Downsample profile tuned by processing mode.
                let thumb_edge = match self.mode {
                    MediaProcessingMode::Cpu => 720,
                    MediaProcessingMode::DedicatedGpu => 1280,
                    MediaProcessingMode::Auto => 960,
                };
                let down = image::imageops::thumbnail(&rgb, thumb_edge, thumb_edge);
                let gray = image::DynamicImage::ImageRgb8(down).to_luma8();

                let (brightness, contrast) = Self::grayscale_stats(&gray);
                let focus_score = Self::focus_score(&gray);
                let noise_score = Self::estimate_noise(&gray);
                let in_focus = focus_score >= 0.30;
                let quality_score = (
                    (focus_score * 0.50)
                    + (contrast * 0.20)
                    + ((1.0 - (brightness - 0.50).abs() * 2.0).clamp(0.0, 1.0) * 0.20)
                    + ((1.0 - noise_score).clamp(0.0, 1.0) * 0.10)
                ).clamp(0.0, 1.0);

                meta.width = Some(w);
                meta.height = Some(h);
                meta.megapixels = Some(mp);
                meta.focus_score = Some(focus_score);
                meta.quality_score = Some(quality_score);
                meta.brightness = Some(brightness);
                meta.contrast = Some(contrast);
                meta.in_focus = Some(in_focus);
                meta.dominant_colors = Self::dominant_colors(&rgb, 4);

                meta.tags.push(ext.to_string());
                meta.tags.push(format!("{}x{}", w, h));
                meta.tags.push(if mp >= 24.0 {
                    "ultra-res".into()
                } else if mp >= 8.0 {
                    "high-res".into()
                } else if mp >= 2.0 {
                    "mid-res".into()
                } else {
                    "low-res".into()
                });

                meta.tags.push(if in_focus { "in-focus".into() } else { "out-of-focus".into() });
                meta.tags.push(Self::classify_exposure(brightness).to_string());

                if w >= h {
                    meta.tags.push("landscape".into());
                } else {
                    meta.tags.push("portrait".into());
                }

                if contrast < 0.18 {
                    meta.tags.push("low-contrast".into());
                } else if contrast > 0.40 {
                    meta.tags.push("high-contrast".into());
                }

                if noise_score > 0.30 {
                    meta.tags.push("noisy".into());
                } else {
                    meta.tags.push("clean".into());
                }

                meta.description = format!(
                    "{}x{} ({:.1}MP), focus {:.2}, quality {:.2}, mode {}",
                    w,
                    h,
                    mp,
                    focus_score,
                    quality_score,
                    self.mode.as_str()
                );

                Self::inject_exif(&mut meta, path);
            }
            "mp4" | "mkv" | "mov" | "avi" => {
                let size_mb = path.metadata().map(|m| m.len() / 1_048_576).unwrap_or(0);
                let duration = match ext.as_str() {
                    "mp4" | "mov" => read_mp4_duration(path),
                    _ => None,
                };
                meta.duration_secs = duration;
                meta.video_codec = Some(ext.to_ascii_uppercase());
                if let Some(d) = duration {
                    let mins = (d / 60.0) as u64;
                    let secs = (d % 60.0) as u64;
                    meta.description = format!("{} {}:{:02}  ~{}MB", ext.to_ascii_uppercase(), mins, secs, size_mb);
                    meta.tags.push(format!("duration_{}s", d as u64));
                } else {
                    meta.description = format!("{} container, ~{}MB", ext.to_ascii_uppercase(), size_mb);
                }
                meta.tags.extend([ext.to_string(), "video".into(), "container-only-analysis".into()]);
            }
            _ => {
                meta.description = format!("Unsupported media type: {}", ext);
            }
        }

        log::debug!(
            "[AI] [{}:{}] Analyzed {:?}: {}",
            self.gpu_name,
            self.mode.as_str(),
            path,
            meta.description
        );
        Ok(meta)
    }
}

// ── MP4/MOV duration extraction ───────────────────────────────────────────────
// Pure-Rust box parser: finds the 'mvhd' atom inside 'moov' and reads
// duration/timescale without any external dependency.

fn find_mp4_box<R: std::io::Read + std::io::Seek>(
    f: &mut R,
    box_type: &[u8; 4],
    start: u64,
    end: u64,
) -> Option<u64> {
    use std::io::SeekFrom;
    let mut pos = start;
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut hdr = [0u8; 8];
        f.read_exact(&mut hdr).ok()?;
        let size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        if &hdr[4..8] == box_type { return Some(pos); }
        if size < 8 { break; }
        pos += size;
    }
    None
}

fn read_mp4_duration(path: &std::path::Path) -> Option<f64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let file_len = f.metadata().ok()?.len();
    // Search only the first 64 MB to avoid reading huge video files entirely
    let scan_end = file_len.min(64 * 1024 * 1024);

    let moov_pos = find_mp4_box(&mut f, b"moov", 0, scan_end)?;
    f.seek(SeekFrom::Start(moov_pos)).ok()?;
    let mut sz_buf = [0u8; 4];
    f.read_exact(&mut sz_buf).ok()?;
    let moov_size = u32::from_be_bytes(sz_buf) as u64;

    let mvhd_pos = find_mp4_box(&mut f, b"mvhd", moov_pos + 8, moov_pos + moov_size)?;
    // Skip box size(4) + box type(4) = 8 bytes, then read version(1) + flags(3)
    f.seek(SeekFrom::Start(mvhd_pos + 8)).ok()?;
    let mut ver_flags = [0u8; 4];
    f.read_exact(&mut ver_flags).ok()?;
    let version = ver_flags[0];

    let (timescale, duration): (u64, u64) = if version == 1 {
        // v1: creation(8) + modification(8) + timescale(4) + duration(8)
        f.seek(SeekFrom::Current(16)).ok()?;
        let mut ts = [0u8; 4]; f.read_exact(&mut ts).ok()?;
        let mut d  = [0u8; 8]; f.read_exact(&mut d).ok()?;
        (u32::from_be_bytes(ts) as u64, u64::from_be_bytes(d))
    } else {
        // v0: creation(4) + modification(4) + timescale(4) + duration(4)
        f.seek(SeekFrom::Current(8)).ok()?;
        let mut ts = [0u8; 4]; f.read_exact(&mut ts).ok()?;
        let mut d  = [0u8; 4]; f.read_exact(&mut d).ok()?;
        (u32::from_be_bytes(ts) as u64, u32::from_be_bytes(d) as u64)
    };

    if timescale == 0 { return None; }
    Some(duration as f64 / timescale as f64)
}

