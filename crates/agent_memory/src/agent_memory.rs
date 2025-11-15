//! Global agent memory management

use crate::embedding::Embedding;
use crate::vector_store::VectorStore;
use anyhow::Result;
use std::sync::Arc;

/// Manages global memory for an agent across all sessions
pub struct AgentMemory {
    agent_id: String,
    agent_type: String,
    vector_store: Arc<dyn VectorStore>,
}

impl AgentMemory {
    pub fn new(
        agent_id: String,
        agent_type: String,
        vector_store: Arc<dyn VectorStore>,
    ) -> Self {
        Self {
            agent_id,
            agent_type,
            vector_store,
        }
    }

    /// Add a session embedding to global memory using incremental mean pooling
    /// 
    /// This method requires the current session_count to be passed in, as it needs
    /// to be read from the database by the caller.
    pub async fn add_session_embedding_with_count(
        &self,
        embedding: &Embedding,
        current_count: i32,
    ) -> Result<()> {
        // Get current global embedding
        let current_global = self.vector_store.get_agent_embedding(&self.agent_id).await?;
        
        let (new_embedding, new_count) = if let Some(current) = current_global {
            // Incremental mean pooling: new_mean = (old_mean * old_count + new_embedding) / (old_count + 1)
            let old_count = current_count as f32;
            let old_mean = current.vector.clone();
            let new_vec = embedding.vector.clone();
            
            // Compute: new_mean = (old_mean * old_count + new_vec) / (old_count + 1)
            let mut new_mean = Vec::with_capacity(old_mean.len());
            for (old_val, new_val) in old_mean.iter().zip(new_vec.iter()) {
                new_mean.push((old_val * old_count + new_val) / (old_count + 1.0));
            }
            
            let mut aggregated = Embedding::new(new_mean, embedding.model.clone())?;
            aggregated.normalize();
            (aggregated, current_count + 1)
        } else {
            // First embedding - just use it directly
            (embedding.clone(), 1)
        };
        
        self.vector_store
            .store_agent_embedding(
                &self.agent_id,
                &self.agent_type,
                &new_embedding,
                new_count as usize,
                "mean",
            )
            .await?;

        Ok(())
    }

    /// Get global embedding for this agent
    pub async fn get_global_embedding(&self) -> Result<Option<Embedding>> {
        self.vector_store.get_agent_embedding(&self.agent_id).await
    }

    /// Search for similar sessions using global agent embedding
    pub async fn search_similar_sessions(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<(String, f32)>> {
        self.vector_store
            .search_similar_sessions(query_embedding, limit, threshold)
            .await
    }
}

/// Mean pooling aggregation for embeddings
/// 
/// Formula: mean = (sum of all embeddings) / count
/// For incremental updates: new_mean = (old_mean * old_count + new_embedding) / (old_count + 1)
pub fn aggregate_embeddings_mean(
    embeddings: &[Embedding],
) -> Result<Embedding> {
    if embeddings.is_empty() {
        anyhow::bail!("Cannot aggregate empty embedding list");
    }

    let dimension = embeddings[0].dimension;
    let mut sum = vec![0.0f32; dimension];

    for emb in embeddings {
        if emb.dimension != dimension {
            anyhow::bail!("Dimension mismatch in aggregation");
        }
        for (i, v) in emb.vector.iter().enumerate() {
            sum[i] += v;
        }
    }

    let count = embeddings.len() as f32;
    for v in &mut sum {
        *v /= count;
    }

    // Normalize the result
    let mut aggregated = Embedding::new(sum, embeddings[0].model.clone())?;
    aggregated.normalize();

    Ok(aggregated)
}

