//! Session memory management

use crate::embedding::{content_hash, normalize_text_for_embedding, Embedding, EmbeddingGenerator, EmbeddingModel};
use crate::vector_store::VectorStore;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// Manages memory and embeddings for a single session
pub struct SessionMemory {
    session_id: String,
    embedding_generator: Arc<dyn EmbeddingGenerator>,
    vector_store: Arc<dyn VectorStore>,
    model: EmbeddingModel,
}

impl SessionMemory {
    pub fn new(
        session_id: String,
        embedding_generator: Arc<dyn EmbeddingGenerator>,
        vector_store: Arc<dyn VectorStore>,
        model: Option<EmbeddingModel>,
    ) -> Self {
        Self {
            session_id,
            embedding_generator,
            vector_store,
            model: model.unwrap_or_default(),
        }
    }

    /// Add a message to the session and update embedding
    pub async fn add_message(&self, text: &str) -> Result<()> {
        let normalized = normalize_text_for_embedding(text);
        let hash = content_hash(&normalized);
        
        // Check cache first
        if let Some(cached) = self.vector_store.get_message_embedding(&hash).await? {
            // Use cached embedding
            // TODO: Aggregate with session embedding
        } else {
            // Generate new embedding
            let embedding = self.embedding_generator.generate(&normalized, self.model.clone()).await?;
            self.vector_store.store_message_embedding(&hash, &embedding).await?;
            // TODO: Update session embedding
        }
        
        Ok(())
    }

    /// Get current session embedding
    pub async fn get_embedding(&self) -> Result<Option<Embedding>> {
        self.vector_store.get_session_embedding(&self.session_id).await
    }

    /// Update global agent embedding with this session's contribution
    pub async fn update_global_agent_embedding(
        &self,
        agent_id: &str,
        agent_type: &str,
    ) -> Result<()> {
        let session_embedding = match self.get_embedding().await? {
            Some(emb) => emb,
            None => return Ok(()), // No embedding yet
        };

        // Get current global embedding
        let global_embedding = self.vector_store.get_agent_embedding(agent_id).await?;
        
        // TODO: Implement mean pooling aggregation
        // For now, just store the session embedding as global
        // In the future, this should aggregate multiple session embeddings
        
        self.vector_store
            .store_agent_embedding(
                agent_id,
                agent_type,
                &session_embedding,
                1, // session_count - will be updated properly later
                "mean",
            )
            .await?;

        Ok(())
    }
}

