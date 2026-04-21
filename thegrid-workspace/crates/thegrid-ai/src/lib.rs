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
        log::warn!("[AI] AI dependencies (fastembed/usearch) are currently stubbed. Semantic search is disabled.");
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
        total_ram_gb >= 8
    }
    pub fn capability_label(&self) -> &'static str {
        if self.is_ai_node() { "AI NODE (STUBBED)" } else { "STANDARD" }
    }
}

impl Default for AiNodeDetector {
    fn default() -> Self { Self::new() }
}

pub struct SemanticSearch {
    provider: Arc<dyn EmbeddingProvider>,
    index: USearchIndex,
}

impl SemanticSearch {
    pub fn new(provider: Arc<dyn EmbeddingProvider>) -> Result<Self> {
        let dims = provider.dimensions();
        let index = USearchIndex::new(dims)?;
        Ok(Self { provider, index })
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
