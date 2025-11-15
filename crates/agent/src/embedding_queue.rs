//! Embedding job queue for background processing

use agent_memory::{AgentMemory, EmbeddingGenerator, VectorStore};
use anyhow::{Context, Result};
use gpui::BackgroundExecutor;
use indoc::indoc;
use parking_lot::Mutex;
use sqlez::connection::Connection;
use std::sync::Arc;
use zstd;

/// Status of an embedding job
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingJobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl EmbeddingJobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "processing" => Self::Processing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Embedding job
pub struct EmbeddingJob {
    pub job_id: String,
    pub session_id: String,
    pub content_hash: String,
    pub status: EmbeddingJobStatus,
    pub retry_count: u32,
    pub error_message: Option<String>,
}

/// Embedding queue for processing embeddings in the background
pub struct EmbeddingQueue {
    executor: BackgroundExecutor,
    connection: Arc<Mutex<Connection>>,
    generator: Arc<dyn EmbeddingGenerator>,
    vector_store: Arc<dyn VectorStore>,
    _worker_task: Arc<Mutex<Option<gpui::Task<()>>>>,
}

impl EmbeddingQueue {
    /// Create a new embedding queue
    pub fn new(
        executor: BackgroundExecutor,
        connection: Arc<Mutex<Connection>>,
        generator: Arc<dyn EmbeddingGenerator>,
        vector_store: Arc<dyn VectorStore>,
    ) -> Self {
        Self {
            executor,
            connection,
            generator,
            vector_store,
            _worker_task: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the background worker
    pub fn start_worker(&self) {
        let executor = self.executor.clone();
        let connection = self.connection.clone();
        let generator = self.generator.clone();
        let vector_store = self.vector_store.clone();

        let task = executor.clone().spawn(async move {
            Self::worker_loop(executor, connection, generator, vector_store).await;
        });

        *self._worker_task.lock() = Some(task);
    }

    /// Queue an embedding job
    pub fn queue_job(
        &self,
        session_id: &str,
        content_hash: &str,
    ) -> Result<()> {
        // Generate unique job ID using session_id and content_hash
        let job_id = format!("{}-{}", session_id, content_hash);
        let session_id = session_id.to_string();
        let content_hash = content_hash.to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let connection = self.connection.clone();
        self.executor.spawn(async move {
            let connection = connection.lock();
            let mut insert = connection.exec_bound::<(&str, &str, &str, &str, &str)>(indoc! {"
                INSERT INTO embedding_jobs
                (job_id, session_id, content_hash, status, created_at, updated_at)
                VALUES (?, ?, ?, 'pending', ?, ?)
            "})?;

            insert((&job_id, &session_id, &content_hash, &now, &now))?;
            Ok::<(), anyhow::Error>(())
        }).detach();

        Ok(())
    }

    /// Background worker loop
    async fn worker_loop(
        executor: BackgroundExecutor,
        connection: Arc<Mutex<Connection>>,
        generator: Arc<dyn EmbeddingGenerator>,
        vector_store: Arc<dyn VectorStore>,
    ) {
        const BATCH_SIZE: usize = 10;
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

        loop {
            // Fetch pending jobs
            let jobs = Self::fetch_pending_jobs(&executor, &connection, BATCH_SIZE).await;
            
            if jobs.is_empty() {
                // No jobs, wait a bit before checking again
                executor.timer(std::time::Duration::from_secs(1)).await;
                continue;
            }

            // Process each job
            for job in jobs {
                // Mark as processing
                Self::update_job_status(&executor, &connection, &job.job_id, EmbeddingJobStatus::Processing, None).await;

                // Generate embedding
                let result = Self::process_job(&executor, &connection, &generator, &vector_store, &job).await;

                match result {
                    Ok(()) => {
                        // Mark as completed
                        Self::update_job_status(&executor, &connection, &job.job_id, EmbeddingJobStatus::Completed, None).await;
                        
                        // Update chat_sessions to mark embedding as complete
                        Self::mark_embedding_complete(&executor, &connection, &job.session_id).await;
                        
                        // Update global agent embedding
                        Self::update_global_embedding(
                            &executor,
                            &connection,
                            &vector_store,
                            &job.session_id,
                        ).await;
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        let retry_count = job.retry_count + 1;

                        if retry_count < MAX_RETRIES {
                            // Retry - mark as pending again
                            Self::update_job_retry(&executor, &connection, &job.job_id, retry_count, Some(&error_msg)).await;
                            // Wait before retry
                            executor.timer(RETRY_DELAY).await;
                        } else {
                            // Max retries reached - mark as failed
                            Self::update_job_status(&executor, &connection, &job.job_id, EmbeddingJobStatus::Failed, Some(&error_msg)).await;
                        }
                    }
                }
            }
        }
    }

    /// Fetch pending jobs from database
    async fn fetch_pending_jobs(
        executor: &BackgroundExecutor,
        connection: &Arc<Mutex<Connection>>,
        limit: usize,
    ) -> Vec<EmbeddingJob> {
        let db_connection = connection.clone();
        match executor.spawn(async move {
            let connection = db_connection.lock();
            let mut select = connection.select_bound::<i32, (String, String, String, String, i32, Option<String>)>(
                indoc! {"
                    SELECT job_id, session_id, content_hash, status, retry_count, error_message
                    FROM embedding_jobs
                    WHERE status = 'pending'
                    ORDER BY created_at ASC
                    LIMIT ?
                "},
            )?;

            let rows = select(limit as i32)?;
            let mut jobs = Vec::new();

            for (job_id, session_id, content_hash, status, retry_count, error_message) in rows {
                jobs.push(EmbeddingJob {
                    job_id,
                    session_id,
                    content_hash,
                    status: EmbeddingJobStatus::from_str(&status),
                    retry_count: retry_count as u32,
                    error_message,
                });
            }

            Ok::<Vec<EmbeddingJob>, anyhow::Error>(jobs)
        })
        .await
        {
            Ok(jobs) => jobs,
            Err(_) => Vec::new(),
        }
    }

    /// Process a single embedding job
    async fn process_job(
        executor: &BackgroundExecutor,
        connection: &Arc<Mutex<Connection>>,
        generator: &Arc<dyn EmbeddingGenerator>,
        vector_store: &Arc<dyn VectorStore>,
        job: &EmbeddingJob,
    ) -> Result<()> {
        // Load thread messages from database
        let db_connection = connection.clone();
        let executor = executor.clone();
        let session_id = job.session_id.clone();

        let messages = executor
            .spawn({
                let db_connection = db_connection.clone();
                let session_id = session_id.clone();
                async move {
                    let connection = db_connection.lock();
                    let mut select =
                        connection.select_bound::<&str, (crate::db::DataType, Vec<u8>)>(indoc! {"
                            SELECT data_type, data FROM threads WHERE id = ? LIMIT 1
                        "})?;

                    let rows = select(&session_id)?;
                    if let Some((data_type, data)) = rows.into_iter().next() {
                        let json_data = match data_type {
                            crate::db::DataType::Zstd => {
                                let decompressed = zstd::decode_all(&data[..])?;
                                String::from_utf8(decompressed)?
                            }
                            crate::db::DataType::Json => String::from_utf8(data)?,
                        };
                        let db_thread = crate::DbThread::from_json(json_data.as_bytes())?;
                        Ok::<Vec<crate::Message>, anyhow::Error>(db_thread.messages)
                    } else {
                        anyhow::bail!("Thread not found: {}", session_id)
                    }
                }
            })
            .await?;

        // Extract session text
        let session_text = crate::extract_session_text(&messages);
        if session_text.is_empty() {
            anyhow::bail!("No text content in session");
        }

        // Verify content hash matches
        let computed_hash = agent_memory::embedding::content_hash(&session_text);
        if computed_hash != job.content_hash {
            anyhow::bail!(
                "Content hash mismatch: expected {}, got {}",
                job.content_hash,
                computed_hash
            );
        }

        // Generate embedding
        let embedding = generator
            .generate(&session_text, agent_memory::EmbeddingModel::default())
            .await
            .context("Failed to generate embedding")?;

        // Store session embedding
        vector_store
            .store_session_embedding(&job.session_id, &embedding, Some(&job.content_hash))
            .await
            .context("Failed to store embedding")?;

        Ok(())
    }

    /// Update job status in database
    async fn update_job_status(
        executor: &BackgroundExecutor,
        connection: &Arc<Mutex<Connection>>,
        job_id: &str,
        status: EmbeddingJobStatus,
        error_message: Option<&str>,
    ) {
        let connection = connection.clone();
        let executor = executor.clone();
        let job_id = job_id.to_string();
        let status_str = status.as_str().to_string();
        let error_message = error_message.map(|s| s.to_string());
        let now = chrono::Utc::now().to_rfc3339();

        executor.spawn(async move {
            let connection = connection.lock();
            let mut update = connection.exec_bound::<(&str, Option<&str>, &str, &str)>(indoc! {"
                UPDATE embedding_jobs
                SET status = ?, error_message = ?, updated_at = ?
                WHERE job_id = ?
            "})?;

            update((status_str.as_str(), error_message.as_deref(), &now, &job_id))?;
            Ok::<(), anyhow::Error>(())
        }).detach();
    }

    /// Update job retry count
    async fn update_job_retry(
        executor: &BackgroundExecutor,
        connection: &Arc<Mutex<Connection>>,
        job_id: &str,
        retry_count: u32,
        error_message: Option<&str>,
    ) {
        let connection = connection.clone();
        let executor = executor.clone();
        let job_id = job_id.to_string();
        let retry_count = retry_count as i32;
        let error_message = error_message.map(|s| s.to_string());
        let now = chrono::Utc::now().to_rfc3339();

        executor.spawn(async move {
            let connection = connection.lock();
            let mut update = connection.exec_bound::<(i32, Option<&str>, &str, &str)>(indoc! {"
                UPDATE embedding_jobs
                SET status = 'pending', retry_count = ?, error_message = ?, updated_at = ?
                WHERE job_id = ?
            "})?;

            update((retry_count, error_message.as_deref(), &now, &job_id))?;
            Ok::<(), anyhow::Error>(())
        }).detach();
    }

    /// Mark embedding as complete in chat_sessions
    async fn mark_embedding_complete(
        executor: &BackgroundExecutor,
        connection: &Arc<Mutex<Connection>>,
        session_id: &str,
    ) {
        let connection = connection.clone();
        let executor = executor.clone();
        let session_id = session_id.to_string();

        executor.spawn(async move {
            let connection = connection.lock();
            let mut update = connection.exec_bound::<&str>(indoc! {"
                UPDATE chat_sessions
                SET pending_embedding = 0
                WHERE session_id = ?
            "})?;

            update(&session_id)?;
            Ok::<(), anyhow::Error>(())
        }).detach();
    }

    /// Update global agent embedding after session embedding is complete
    async fn update_global_embedding(
        executor: &BackgroundExecutor,
        connection: &Arc<Mutex<Connection>>,
        vector_store: &Arc<dyn VectorStore>,
        session_id: &str,
    ) {
        let connection = connection.clone();
        let executor = executor.clone();
        let vector_store = vector_store.clone();
        let session_id = session_id.to_string();

        executor.spawn(async move {
            // Get agent_id and agent_type from chat_sessions
            let db_connection = connection.clone();
            let (agent_id, agent_type) = {
                let connection_guard = db_connection.lock();
                let mut select = connection_guard.select_bound::<&str, (String, String)>(indoc! {"
                    SELECT agent_id, agent_type
                    FROM chat_sessions
                    WHERE session_id = ? LIMIT 1
                "})?;

                match select(&session_id)?.into_iter().next() {
                    Some((id, ty)) => (id, ty),
                    None => return Ok(()), // Session not found
                }
            };

            // Load session embedding
            let session_embedding = match vector_store.get_session_embedding(&session_id).await {
                Ok(Some(emb)) => emb,
                Ok(None) => return Ok(()), // No embedding yet
                Err(_) => return Ok(()), // Error loading embedding
            };

            // Get current session count from database for accurate mean pooling
            let current_count = {
                let connection_guard = db_connection.lock();
                let mut select_count = connection_guard.select_bound::<&str, i32>(indoc! {"
                    SELECT session_count FROM agent_global_embeddings WHERE agent_id = ? LIMIT 1
                "})?;
                
                select_count(&agent_id)?.into_iter().next().unwrap_or(0)
            };

            // Update global agent embedding using incremental mean pooling
            let agent_memory = AgentMemory::new(agent_id, agent_type, vector_store);
            agent_memory.add_session_embedding_with_count(&session_embedding, current_count).await?;

            Ok::<(), anyhow::Error>(())
        }).detach();
    }

    /// Resume pending jobs on startup
    pub fn resume_pending_jobs(
        executor: BackgroundExecutor,
        connection: Arc<Mutex<Connection>>,
    ) -> gpui::Task<Result<()>> {
        executor.spawn(async move {
            let connection = connection.lock();
            
            // Reset any "processing" jobs back to "pending" (assume crash)
            connection.exec_bound::<()>(indoc! {"
                UPDATE embedding_jobs
                SET status = 'pending'
                WHERE status = 'processing'
            "})?(())?;

            // Query sessions with pending embeddings
            let mut select = connection.select_bound::<(), String>(indoc! {"
                SELECT session_id
                FROM chat_sessions
                WHERE pending_embedding = 1
            "})?;

            let _rows = select(())?;
            // Jobs will be queued by the worker when it processes them
            // For now, we just reset the processing jobs

            Ok(())
        })
    }
}
