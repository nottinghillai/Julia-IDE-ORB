//! Embedding generation and management

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Supported embedding models
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingModel {
    /// BGE small English v1.5 (default, 384 dimensions)
    BgeSmallEnV15,
    /// OpenAI text-embedding-3-small (1536 dimensions)
    OpenAiSmall,
    /// OpenAI text-embedding-3-large (3072 dimensions)
    OpenAiLarge,
}

impl EmbeddingModel {
    pub fn default() -> Self {
        Self::BgeSmallEnV15
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::BgeSmallEnV15 => "bge-small-en-v1.5",
            Self::OpenAiSmall => "text-embedding-3-small",
            Self::OpenAiLarge => "text-embedding-3-large",
        }
    }

    pub fn version(&self) -> &'static str {
        match self {
            Self::BgeSmallEnV15 => "1.0",
            Self::OpenAiSmall => "1.0",
            Self::OpenAiLarge => "1.0",
        }
    }

    pub fn dimension(&self) -> usize {
        match self {
            Self::BgeSmallEnV15 => 384,
            Self::OpenAiSmall => 1536,
            Self::OpenAiLarge => 3072,
        }
    }
}

impl Default for EmbeddingModel {
    fn default() -> Self {
        Self::BgeSmallEnV15
    }
}

/// Embedding vector (normalized float array)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub vector: Vec<f32>,
    pub model: EmbeddingModel,
    pub dimension: usize,
}

impl Embedding {
    pub fn new(vector: Vec<f32>, model: EmbeddingModel) -> Result<Self> {
        let dimension = model.dimension();
        if vector.len() != dimension {
            anyhow::bail!(
                "Embedding dimension mismatch: expected {}, got {}",
                dimension,
                vector.len()
            );
        }
        Ok(Self {
            vector,
            model,
            dimension,
        })
    }

    /// Normalize the embedding vector to unit length
    pub fn normalize(&mut self) {
        let norm: f32 = self.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut self.vector {
                *v /= norm;
            }
        }
    }

    /// Compute cosine similarity with another embedding
    pub fn cosine_similarity(&self, other: &Embedding) -> Result<f32> {
        if self.dimension != other.dimension {
            anyhow::bail!("Dimension mismatch for cosine similarity");
        }
        let dot_product: f32 = self
            .vector
            .iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();
        Ok(dot_product) // Assumes both embeddings are normalized
    }
}

/// Content hash for caching embeddings
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

/// Normalize text for embedding (remove timestamps, normalize whitespace)
pub fn normalize_text_for_embedding(text: &str) -> String {
    // Remove common timestamp patterns
    let text = regex::Regex::new(r"\d{4}-\d{2}-\d{2}[\sT]\d{2}:\d{2}:\d{2}")
        .unwrap()
        .replace_all(text, "");
    
    // Normalize whitespace
    let text = regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(&text, " ");
    
    text.trim().to_string()
}

/// Trait for generating embeddings
#[async_trait::async_trait]
pub trait EmbeddingGenerator: Send + Sync {
    /// Generate embedding for text
    async fn generate(&self, text: &str, model: EmbeddingModel) -> Result<Embedding>;

    /// Generate embeddings for multiple texts (batch processing)
    async fn generate_batch(
        &self,
        texts: &[String],
        model: EmbeddingModel,
    ) -> Result<Vec<Embedding>>;
}

/// Placeholder implementation - will be replaced with actual model loading
pub struct PlaceholderEmbeddingGenerator;

#[async_trait::async_trait]
impl EmbeddingGenerator for PlaceholderEmbeddingGenerator {
    async fn generate(&self, _text: &str, model: EmbeddingModel) -> Result<Embedding> {
        // Placeholder: return zero vector
        let dimension = model.dimension();
        let vector = vec![0.0f32; dimension];
        Embedding::new(vector, model)
    }

    async fn generate_batch(
        &self,
        texts: &[String],
        model: EmbeddingModel,
    ) -> Result<Vec<Embedding>> {
        let dimension = model.dimension();
        Ok(texts
            .iter()
            .map(|_| Embedding::new(vec![0.0f32; dimension], model.clone()))
            .collect::<Result<Vec<_>>>()?)
    }
}

impl fmt::Display for EmbeddingModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

