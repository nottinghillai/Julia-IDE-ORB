//! SQLite-backed vector store implementation

use crate::embedding::Embedding;
use crate::vector_store::{VectorStore, VectorStoreError};
use anyhow::{Context, Result};
use gpui::BackgroundExecutor;
use indoc::indoc;
use parking_lot::Mutex;
use sqlez::connection::Connection;
use std::sync::Arc;

/// SQLite-backed vector store using the threads database connection
pub struct SQLiteVectorStore {
    executor: BackgroundExecutor,
    connection: Arc<Mutex<Connection>>,
}

impl SQLiteVectorStore {
    /// Create a new SQLiteVectorStore from a database connection
    pub fn new(executor: BackgroundExecutor, connection: Arc<Mutex<Connection>>) -> Self {
        Self {
            executor,
            connection,
        }
    }

    /// Serialize embedding vector to bytes for BLOB storage
    fn serialize_embedding(embedding: &Embedding) -> Vec<u8> {
        // Convert Vec<f32> to bytes (little-endian)
        let mut bytes = Vec::with_capacity(embedding.vector.len() * 4);
        for &value in &embedding.vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Deserialize embedding vector from BLOB
    fn deserialize_embedding(
        bytes: &[u8],
        model: crate::embedding::EmbeddingModel,
    ) -> Result<Embedding> {
        if bytes.len() % 4 != 0 {
            anyhow::bail!("Invalid embedding blob size: not a multiple of 4");
        }

        let dimension = model.dimension();
        let expected_size = dimension * 4;
        if bytes.len() != expected_size {
            anyhow::bail!(
                "Embedding size mismatch: expected {} bytes, got {}",
                expected_size,
                bytes.len()
            );
        }

        let mut vector = Vec::with_capacity(dimension);
        for chunk in bytes.chunks_exact(4) {
            let value = f32::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
            ]);
            vector.push(value);
        }

        Embedding::new(vector, model)
    }

    /// Compute cosine similarity between two vectors
    /// Assumes both vectors are normalized (unit length)
    fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32> {
        if a.len() != b.len() {
            anyhow::bail!("Vector length mismatch for cosine similarity");
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        Ok(dot_product) // If normalized, this is the cosine similarity
    }
}

#[async_trait::async_trait]
impl VectorStore for SQLiteVectorStore {
    async fn store_session_embedding(
        &self,
        session_id: &str,
        embedding: &Embedding,
        content_hash: Option<&str>,
    ) -> Result<()> {
        let session_id = session_id.to_string();
        let embedding_bytes = Self::serialize_embedding(embedding);
        let model_name = embedding.model.name().to_string();
        let model_version = embedding.model.version().to_string();
        let dimension = embedding.dimension as i32;
        let content_hash = content_hash.map(|s| s.to_string());
        let now = chrono::Utc::now().to_rfc3339();

        let connection = self.connection.clone();
        self.executor
            .spawn(async move {
                let connection = connection.lock();
                let mut insert = connection.exec_bound::<(
                    &str,
                    &[u8],
                    &str,
                    &str,
                    i32,
                    Option<&str>,
                    &str,
                )>(indoc! {"
                    INSERT OR REPLACE INTO session_embeddings 
                    (session_id, embedding, embedding_model, embedding_model_version, 
                     embedding_dimension, content_hash, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                "})?;

                insert((
                    &session_id,
                    &embedding_bytes,
                    &model_name,
                    &model_version,
                    dimension,
                    content_hash.as_deref(),
                    &now,
                ))?;

                Ok(())
            })
            .await
    }

    async fn get_session_embedding(&self, session_id: &str) -> Result<Option<Embedding>> {
        let session_id = session_id.to_string();
        let connection = self.connection.clone();

        self.executor
            .spawn(async move {
                let connection = connection.lock();
                let mut select = connection.select_bound::<&str, (Vec<u8>, String, String, i32)>(
                    indoc! {"
                        SELECT embedding, embedding_model, embedding_model_version, embedding_dimension
                        FROM session_embeddings
                        WHERE session_id = ? LIMIT 1
                    "},
                )?;

                let rows = select(&session_id)?;
                if let Some((embedding_bytes, model_name, _model_version, _dimension)) =
                    rows.into_iter().next()
                {
                    // Parse model from name
                    let model = match model_name.as_str() {
                        "bge-small-en-v1.5" => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                        "text-embedding-3-small" => crate::embedding::EmbeddingModel::OpenAiSmall,
                        "text-embedding-3-large" => crate::embedding::EmbeddingModel::OpenAiLarge,
                        _ => {
                            // Default to BGE if unknown
                            crate::embedding::EmbeddingModel::BgeSmallEnV15
                        }
                    };

                    let embedding = Self::deserialize_embedding(&embedding_bytes, model)
                        .context("Failed to deserialize session embedding")?;
                    Ok(Some(embedding))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn store_message_embedding(
        &self,
        content_hash: &str,
        embedding: &Embedding,
    ) -> Result<()> {
        let content_hash = content_hash.to_string();
        let embedding_bytes = Self::serialize_embedding(embedding);
        let model_name = embedding.model.name().to_string();
        let model_version = embedding.model.version().to_string();
        let dimension = embedding.dimension as i32;
        let now = chrono::Utc::now().to_rfc3339();

        let connection = self.connection.clone();
        self.executor
            .spawn(async move {
                let connection = connection.lock();
                let mut insert = connection.exec_bound::<(&str, &[u8], &str, &str, i32, &str)>(
                    indoc! {"
                        INSERT OR REPLACE INTO message_embeddings
                        (content_hash, embedding, embedding_model, embedding_model_version, 
                         embedding_dimension, created_at)
                        VALUES (?, ?, ?, ?, ?, ?)
                    "},
                )?;

                insert((
                    &content_hash,
                    &embedding_bytes,
                    &model_name,
                    &model_version,
                    dimension,
                    &now,
                ))?;

                Ok(())
            })
            .await
    }

    async fn get_message_embedding(&self, content_hash: &str) -> Result<Option<Embedding>> {
        let content_hash = content_hash.to_string();
        let connection = self.connection.clone();

        self.executor
            .spawn(async move {
                let connection = connection.lock();
                let mut select = connection.select_bound::<&str, (Vec<u8>, String, String, i32)>(
                    indoc! {"
                        SELECT embedding, embedding_model, embedding_model_version, embedding_dimension
                        FROM message_embeddings
                        WHERE content_hash = ? LIMIT 1
                    "},
                )?;

                let rows = select(&content_hash)?;
                if let Some((embedding_bytes, model_name, _model_version, _dimension)) =
                    rows.into_iter().next()
                {
                    let model = match model_name.as_str() {
                        "bge-small-en-v1.5" => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                        "text-embedding-3-small" => crate::embedding::EmbeddingModel::OpenAiSmall,
                        "text-embedding-3-large" => crate::embedding::EmbeddingModel::OpenAiLarge,
                        _ => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                    };

                    let embedding = Self::deserialize_embedding(&embedding_bytes, model)
                        .context("Failed to deserialize message embedding")?;
                    Ok(Some(embedding))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn store_agent_embedding(
        &self,
        agent_id: &str,
        agent_type: &str,
        embedding: &Embedding,
        session_count: usize,
        aggregation_method: &str,
    ) -> Result<()> {
        let agent_id = agent_id.to_string();
        let agent_type = agent_type.to_string();
        let embedding_bytes = Self::serialize_embedding(embedding);
        let model_name = embedding.model.name().to_string();
        let model_version = embedding.model.version().to_string();
        let dimension = embedding.dimension as i32;
        let session_count = session_count as i32;
        let aggregation_method = aggregation_method.to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let connection = self.connection.clone();
        self.executor
            .spawn(async move {
                let connection = connection.lock();
                let mut insert = connection.exec_bound::<(
                    &str,
                    &str,
                    &[u8],
                    &str,
                    &str,
                    i32,
                    i32,
                    &str,
                    &str,
                )>(indoc! {"
                    INSERT OR REPLACE INTO agent_global_embeddings
                    (agent_id, agent_type, embedding, embedding_model, embedding_model_version,
                     embedding_dimension, session_count, aggregation_method, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "})?;

                insert((
                    &agent_id,
                    &agent_type,
                    &embedding_bytes,
                    &model_name,
                    &model_version,
                    dimension,
                    session_count,
                    &aggregation_method,
                    &now,
                ))?;

                Ok(())
            })
            .await
    }

    async fn get_agent_embedding(&self, agent_id: &str) -> Result<Option<Embedding>> {
        let agent_id = agent_id.to_string();
        let connection = self.connection.clone();

        self.executor
            .spawn(async move {
                let connection = connection.lock();
                let mut select = connection.select_bound::<&str, (Vec<u8>, String, String, i32)>(
                    indoc! {"
                        SELECT embedding, embedding_model, embedding_model_version, embedding_dimension
                        FROM agent_global_embeddings
                        WHERE agent_id = ? LIMIT 1
                    "},
                )?;

                let rows = select(&agent_id)?;
                if let Some((embedding_bytes, model_name, _model_version, _dimension)) =
                    rows.into_iter().next()
                {
                    let model = match model_name.as_str() {
                        "bge-small-en-v1.5" => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                        "text-embedding-3-small" => crate::embedding::EmbeddingModel::OpenAiSmall,
                        "text-embedding-3-large" => crate::embedding::EmbeddingModel::OpenAiLarge,
                        _ => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                    };

                    let embedding = Self::deserialize_embedding(&embedding_bytes, model)
                        .context("Failed to deserialize agent embedding")?;
                    Ok(Some(embedding))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn search_similar_sessions(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<(String, f32)>> {
        let query_vector = query_embedding.vector.clone();
        let model_name = query_embedding.model.name().to_string();
        let model_version = query_embedding.model.version().to_string();
        let dimension = query_embedding.dimension;
        let limit = limit as i32;

        let connection = self.connection.clone();
        self.executor
            .spawn(async move {
                let connection = connection.lock();
                
                // Query all session embeddings with matching model
                let mut select = connection.select_bound::<(&str, &str, i32), (String, Vec<u8>)>(indoc! {"
                    SELECT session_id, embedding
                    FROM session_embeddings
                    WHERE embedding_model = ? AND embedding_model_version = ?
                      AND embedding_dimension = ?
                "})?;

                let rows = select((&model_name, &model_version, dimension as i32))?;
                
                let mut results = Vec::new();
                for (session_id, embedding_bytes) in rows {
                    // Deserialize embedding
                    let model = match model_name.as_str() {
                        "bge-small-en-v1.5" => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                        "text-embedding-3-small" => crate::embedding::EmbeddingModel::OpenAiSmall,
                        "text-embedding-3-large" => crate::embedding::EmbeddingModel::OpenAiLarge,
                        _ => crate::embedding::EmbeddingModel::BgeSmallEnV15,
                    };

                    let candidate_embedding = match Self::deserialize_embedding(&embedding_bytes, model) {
                        Ok(emb) => emb,
                        Err(_) => continue, // Skip invalid embeddings
                    };

                    // Compute cosine similarity
                    let similarity = Self::cosine_similarity(&query_vector, &candidate_embedding.vector)
                        .unwrap_or(0.0);

                    if similarity >= threshold {
                        results.push((session_id, similarity));
                    }
                }

                // Sort by similarity (descending) and take top N
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                results.truncate(limit as usize);

                Ok(results)
            })
            .await
    }
}

