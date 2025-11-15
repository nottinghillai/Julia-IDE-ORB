//! Agent Memory - Vector embeddings and memory management for agent sessions
//!
//! This crate provides:
//! - Embedding generation for chat messages and sessions
//! - Vector storage and retrieval
//! - Session memory management
//! - Global agent memory aggregation

pub mod embedding;
pub use embedding::PlaceholderEmbeddingGenerator;
pub mod vector_store;
pub mod session_memory;
pub mod agent_memory;
pub mod sqlite_vector_store;

#[cfg(feature = "embeddings")]
pub mod bge_generator;

pub use embedding::{EmbeddingModel, EmbeddingGenerator};
pub use vector_store::{VectorStore, VectorStoreError};
pub use session_memory::SessionMemory;
pub use agent_memory::AgentMemory;
pub use sqlite_vector_store::SQLiteVectorStore;

#[cfg(feature = "embeddings")]
pub use bge_generator::BgeEmbeddingGenerator;

