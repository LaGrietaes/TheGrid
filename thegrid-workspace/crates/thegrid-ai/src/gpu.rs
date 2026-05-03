// gpu.rs — GPU device detection and visual-agent routing for THE GRID
//
// Detects available GPU hardware at startup and exposes a routing helper
// so that visual agents (image analysis, VLM inference, video decode) can
// prefer the RTX 2070 CUDA path rather than calling a cloud API.
//
// The actual ONNX/CUDA runtime is behind the `gpu-inference` feature flag.
// Without that feature this module is still fully usable for routing decisions
// (e.g. "do we call the local Ollama VLM or fall back to Gemini?").

use std::sync::OnceLock;
use thegrid_core::Config;

// ── GPU device descriptor ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GpuDeviceInfo {
    /// Display name (e.g. "NVIDIA GeForce RTX 2070")
    pub name: String,
    /// Estimated VRAM in megabytes (None if not queryable without extra libs)
    pub vram_mb: Option<u64>,
    /// CUDA ordinal index (None = not a CUDA device)
    pub cuda_index: Option<u32>,
    /// Whether the device is considered capable of running the visual VLM
    pub visual_capable: bool,
}

impl GpuDeviceInfo {
    pub fn is_nvidia(&self) -> bool {
        self.name.to_ascii_lowercase().contains("nvidia")
            || self.name.to_ascii_lowercase().contains("rtx")
            || self.name.to_ascii_lowercase().contains("gtx")
    }
}

// ── Detection ─────────────────────────────────────────────────────────────

/// Cached result of GPU probe — runs once per process.
static GPU_CACHE: OnceLock<Vec<GpuDeviceInfo>> = OnceLock::new();

/// Probe available GPU devices.
/// On Windows with an NVIDIA driver, `nvidia-smi --query-gpu=name,memory.total
/// --format=csv,noheader` is a reliable zero-dependency detection method.
pub fn detect_gpu_devices() -> Vec<GpuDeviceInfo> {
    GPU_CACHE
        .get_or_init(|| {
            let mut devices = Vec::new();

            // --- Attempt nvidia-smi (Windows + Linux) ---
            if let Ok(out) = std::process::Command::new("nvidia-smi")
                .args(["--query-gpu=index,name,memory.total", "--format=csv,noheader"])
                .output()
            {
                if out.status.success() {
                    let text = String::from_utf8_lossy(&out.stdout);
                    for line in text.lines() {
                        let parts: Vec<&str> = line.splitn(3, ',').collect();
                        if parts.len() < 2 {
                            continue;
                        }
                        let idx: u32 = parts[0].trim().parse().unwrap_or(0);
                        let name = parts[1].trim().to_string();
                        let vram_mb = if parts.len() == 3 {
                            // "8192 MiB" or "8192 MB"
                            parts[2]
                                .trim()
                                .split_whitespace()
                                .next()
                                .and_then(|n| n.parse::<u64>().ok())
                        } else {
                            None
                        };
                        let visual_capable = vram_mb.map_or(false, |v| v >= 5_000);
                        devices.push(GpuDeviceInfo {
                            name,
                            vram_mb,
                            cuda_index: Some(idx),
                            visual_capable,
                        });
                    }
                }
            }

            // --- Fallback: check for CUDA DLL presence on Windows ---
            if devices.is_empty() {
                let cuda_present = [
                    "nvcuda.dll",
                    "cudart64_12.dll",
                    "cudart64_110.dll",
                    "cudart64_120.dll",
                ]
                .iter()
                .any(|dll| {
                    let sys32 = format!("C:\\Windows\\System32\\{}", dll);
                    std::path::Path::new(&sys32).exists()
                });

                if cuda_present {
                    devices.push(GpuDeviceInfo {
                        name: "NVIDIA GPU (CUDA detected via DLL)".to_string(),
                        vram_mb: None,
                        cuda_index: Some(0),
                        visual_capable: true, // assume capable; user has 8 GB RTX
                    });
                }
            }

            devices
        })
        .clone()
}

/// Returns the best device to run the visual VLM on, or `None` if no suitable
/// GPU is found and we should fall back to Ollama CPU / cloud.
pub fn best_visual_device() -> Option<GpuDeviceInfo> {
    detect_gpu_devices()
        .into_iter()
        .filter(|d| d.visual_capable)
        .max_by_key(|d| d.vram_mb.unwrap_or(0))
}

// ── Routing helper ────────────────────────────────────────────────────────

/// How a visual task should be executed, in preference order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualRoute {
    /// Run inference via local Ollama using the configured visual model.
    /// Model runs on GPU (CUDA) if Ollama was started with CUDA enabled.
    LocalOllama { model: String, base_url: String },
    /// Fall back to Google Gemini (cloud, second tier).
    GeminiCloud,
    /// Last resort: Claude (cloud, third tier).
    ClaudeCloud,
}

/// Decide how to run a visual task given current config and GPU availability.
pub fn route_visual_task(cfg: &Config) -> VisualRoute {
    // If policy is local-only or local-first, try local Ollama VLM first.
    let policy = cfg.ai_policy.as_str();
    if policy == "local_only" || policy == "local_first" {
        let base_url = cfg
            .visual_model_provider_url
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
        return VisualRoute::LocalOllama {
            model: cfg.visual_model.clone(),
            base_url,
        };
    }

    // cloud_allowed: prefer local GPU if available and configured.
    if cfg.prefer_local_gpu_for_visual && best_visual_device().is_some() {
        let base_url = cfg
            .visual_model_provider_url
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
        return VisualRoute::LocalOllama {
            model: cfg.visual_model.clone(),
            base_url,
        };
    }

    // No local GPU or policy allows cloud: use Gemini first.
    if cfg.google_gemini_api_key.is_some() {
        return VisualRoute::GeminiCloud;
    }

    VisualRoute::ClaudeCloud
}

// ── Model catalogue (ready-to-fetch, no download triggered here) ──────────

/// A known visual model that has been vetted for RTX 2070 / 8 GB VRAM.
#[derive(Debug, Clone)]
pub struct VisualModelSpec {
    /// Ollama pull tag (e.g. "ollama pull qwen2.5vl:7b")
    pub ollama_tag: String,
    /// HuggingFace repo (for manual download / ort backend)
    pub hf_repo: Option<&'static str>,
    /// Estimated VRAM usage in MiB at the default quantisation
    pub vram_mb: u32,
    /// Supports video frames (multi-image context window)
    pub video_capable: bool,
    /// One-line description
    pub description: &'static str,
}

/// All models pre-vetted for the 8 GB RTX 2070.  None are downloaded here.
/// Call `ollama pull <tag>` after configuring `models_dir`.
pub fn vetted_visual_models() -> Vec<VisualModelSpec> {
    vec![
        VisualModelSpec {
            ollama_tag: "qwen2.5vl:7b".to_string(),
            hf_repo: Some("Qwen/Qwen2.5-VL-7B-Instruct-GGUF"),
            vram_mb: 5_500,
            video_capable: true,
            description: "[PRIMARY] Qwen2.5-VL 7B — beats GPT-4o-mini, 125K ctx, images + video frames, 6 GB download",
        },
        VisualModelSpec {
            ollama_tag: "minicpm-v:8b".to_string(),
            hf_repo: Some("openbmb/MiniCPM-V-2_6-gguf"),
            vram_mb: 4_800,
            video_capable: true,
            description: "[BACKUP] MiniCPM-V 2.6 8B — strong OCR + multi-image + video, lower VRAM footprint, 5.5 GB download",
        },
        VisualModelSpec {
            ollama_tag: "llava:13b".to_string(),
            hf_repo: Some("mys/ggml_llava-v1.5-13b"),
            vram_mb: 7_500,
            video_capable: false,
            description: "[LEGACY] LLaVA 1.5 13B — well-tested but no video; tight fit in 8 GB",
        },
    ]
}
