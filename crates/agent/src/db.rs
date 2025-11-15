use crate::{AgentMessage, AgentMessageContent, UserMessage, UserMessageContent};
use acp_thread::UserMessageId;
use agent_client_protocol as acp;
use agent_settings::{AgentProfileId, CompletionMode};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use collections::{HashMap, IndexMap};
use futures::{FutureExt, future::Shared};
use gpui::{BackgroundExecutor, Global, Task};
use indoc::indoc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sqlez::{
    bindable::{Bind, Column},
    connection::Connection,
    statement::Statement,
};
use std::sync::Arc;
use ui::{App, SharedString};
use zed_env_vars::ZED_STATELESS;

pub type DbMessage = crate::Message;
pub type DbSummary = crate::legacy_thread::DetailedSummaryState;
pub type DbLanguageModel = crate::legacy_thread::SerializedLanguageModel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbThreadMetadata {
    pub id: acp::SessionId,
    #[serde(alias = "summary")]
    pub title: SharedString,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbThread {
    pub title: SharedString,
    pub messages: Vec<DbMessage>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub detailed_summary: Option<SharedString>,
    #[serde(default)]
    pub initial_project_snapshot: Option<Arc<crate::ProjectSnapshot>>,
    #[serde(default)]
    pub cumulative_token_usage: language_model::TokenUsage,
    #[serde(default)]
    pub request_token_usage: HashMap<acp_thread::UserMessageId, language_model::TokenUsage>,
    #[serde(default)]
    pub model: Option<DbLanguageModel>,
    #[serde(default)]
    pub completion_mode: Option<CompletionMode>,
    #[serde(default)]
    pub profile: Option<AgentProfileId>,
    #[serde(default)]
    pub agent_id: Option<crate::AgentId>,
    #[serde(default)]
    pub agent_type: Option<crate::AgentType>,
}

impl DbThread {
    pub const VERSION: &'static str = "0.4.0";
    pub const PREVIOUS_VERSION: &'static str = "0.3.0";

    pub fn from_json(json: &[u8]) -> Result<Self> {
        let saved_thread_json = serde_json::from_slice::<serde_json::Value>(json)?;
        let mut thread: DbThread = match saved_thread_json.get("version") {
            Some(serde_json::Value::String(version)) if version == Self::VERSION => {
                serde_json::from_value(saved_thread_json)?
            }
            Some(serde_json::Value::String(version)) if version.starts_with("0.3.") => {
                serde_json::from_value(saved_thread_json)?
            }
            Some(_) | None => {
                return Self::upgrade_from_agent_1(crate::legacy_thread::SerializedThread::from_json(
                    json,
                )?);
            }
        };

        if thread.agent_id.is_none() {
            thread.agent_id = Some(crate::AgentId::from("native"));
        }
        if thread.agent_type.is_none() {
            thread.agent_type = Some(crate::AgentType::Builtin);
        }

        Ok(thread)
    }

    fn upgrade_from_agent_1(thread: crate::legacy_thread::SerializedThread) -> Result<Self> {
        let mut messages = Vec::new();
        let mut request_token_usage = HashMap::default();

        let mut last_user_message_id = None;
        for (ix, msg) in thread.messages.into_iter().enumerate() {
            let message = match msg.role {
                language_model::Role::User => {
                    let mut content = Vec::new();

                    // Convert segments to content
                    for segment in msg.segments {
                        match segment {
                            crate::legacy_thread::SerializedMessageSegment::Text { text } => {
                                content.push(UserMessageContent::Text(text));
                            }
                            crate::legacy_thread::SerializedMessageSegment::Thinking {
                                text,
                                ..
                            } => {
                                // User messages don't have thinking segments, but handle gracefully
                                content.push(UserMessageContent::Text(text));
                            }
                            crate::legacy_thread::SerializedMessageSegment::RedactedThinking {
                                ..
                            } => {
                                // User messages don't have redacted thinking, skip.
                            }
                        }
                    }

                    // If no content was added, add context as text if available
                    if content.is_empty() && !msg.context.is_empty() {
                        content.push(UserMessageContent::Text(msg.context));
                    }

                    let id = UserMessageId::new();
                    last_user_message_id = Some(id.clone());

                    crate::Message::User(UserMessage {
                        // MessageId from old format can't be meaningfully converted, so generate a new one
                        id,
                        content,
                    })
                }
                language_model::Role::Assistant => {
                    let mut content = Vec::new();

                    // Convert segments to content
                    for segment in msg.segments {
                        match segment {
                            crate::legacy_thread::SerializedMessageSegment::Text { text } => {
                                content.push(AgentMessageContent::Text(text));
                            }
                            crate::legacy_thread::SerializedMessageSegment::Thinking {
                                text,
                                signature,
                            } => {
                                content.push(AgentMessageContent::Thinking { text, signature });
                            }
                            crate::legacy_thread::SerializedMessageSegment::RedactedThinking {
                                data,
                            } => {
                                content.push(AgentMessageContent::RedactedThinking(data));
                            }
                        }
                    }

                    // Convert tool uses
                    let mut tool_names_by_id = HashMap::default();
                    for tool_use in msg.tool_uses {
                        tool_names_by_id.insert(tool_use.id.clone(), tool_use.name.clone());
                        content.push(AgentMessageContent::ToolUse(
                            language_model::LanguageModelToolUse {
                                id: tool_use.id,
                                name: tool_use.name.into(),
                                raw_input: serde_json::to_string(&tool_use.input)
                                    .unwrap_or_default(),
                                input: tool_use.input,
                                is_input_complete: true,
                            },
                        ));
                    }

                    // Convert tool results
                    let mut tool_results = IndexMap::default();
                    for tool_result in msg.tool_results {
                        let name = tool_names_by_id
                            .remove(&tool_result.tool_use_id)
                            .unwrap_or_else(|| SharedString::from("unknown"));
                        tool_results.insert(
                            tool_result.tool_use_id.clone(),
                            language_model::LanguageModelToolResult {
                                tool_use_id: tool_result.tool_use_id,
                                tool_name: name.into(),
                                is_error: tool_result.is_error,
                                content: tool_result.content,
                                output: tool_result.output,
                            },
                        );
                    }

                    if let Some(last_user_message_id) = &last_user_message_id
                        && let Some(token_usage) = thread.request_token_usage.get(ix).copied()
                    {
                        request_token_usage.insert(last_user_message_id.clone(), token_usage);
                    }

                    crate::Message::Agent(AgentMessage {
                        content,
                        tool_results,
                    })
                }
                language_model::Role::System => {
                    // Skip system messages as they're not supported in the new format
                    continue;
                }
            };

            messages.push(message);
        }

        Ok(Self {
            title: thread.summary,
            messages,
            updated_at: thread.updated_at,
            detailed_summary: match thread.detailed_summary_state {
                crate::legacy_thread::DetailedSummaryState::NotGenerated
                | crate::legacy_thread::DetailedSummaryState::Generating => None,
                crate::legacy_thread::DetailedSummaryState::Generated { text, .. } => Some(text),
            },
            initial_project_snapshot: thread.initial_project_snapshot,
            cumulative_token_usage: thread.cumulative_token_usage,
            request_token_usage,
            model: thread.model,
            completion_mode: thread.completion_mode,
            profile: thread.profile,
            agent_id: Some(crate::AgentId::from("native")),
            agent_type: Some(crate::AgentType::Builtin),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "zstd")]
    Zstd,
}

impl Bind for DataType {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let value = match self {
            DataType::Json => "json",
            DataType::Zstd => "zstd",
        };
        value.bind(statement, start_index)
    }
}

impl Column for DataType {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (value, next_index) = String::column(statement, start_index)?;
        let data_type = match value.as_str() {
            "json" => DataType::Json,
            "zstd" => DataType::Zstd,
            _ => anyhow::bail!("Unknown data type: {}", value),
        };
        Ok((data_type, next_index))
    }
}

pub(crate) struct ThreadsDatabase {
    executor: BackgroundExecutor,
    connection: Arc<Mutex<Connection>>,
}

struct GlobalThreadsDatabase(Shared<Task<Result<Arc<ThreadsDatabase>, Arc<anyhow::Error>>>>);

impl Global for GlobalThreadsDatabase {}

impl ThreadsDatabase {
    pub fn connect(cx: &mut App) -> Shared<Task<Result<Arc<ThreadsDatabase>, Arc<anyhow::Error>>>> {
        if cx.has_global::<GlobalThreadsDatabase>() {
            return cx.global::<GlobalThreadsDatabase>().0.clone();
        }
        let executor = cx.background_executor().clone();
        let task = executor
            .spawn({
                let executor = executor.clone();
                async move {
                    match ThreadsDatabase::new(executor) {
                        Ok(db) => Ok(Arc::new(db)),
                        Err(err) => Err(Arc::new(err)),
                    }
                }
            })
            .shared();

        cx.set_global(GlobalThreadsDatabase(task.clone()));
        task
    }

    pub fn new(executor: BackgroundExecutor) -> Result<Self> {
        let connection = if *ZED_STATELESS {
            Connection::open_memory(Some("THREAD_FALLBACK_DB"))
        } else if cfg!(any(feature = "test-support", test)) {
            // rust stores the name of the test on the current thread.
            // We use this to automatically create a database that will
            // be shared within the test (for the test_retrieve_old_thread)
            // but not with concurrent tests.
            let thread = std::thread::current();
            let test_name = thread.name();
            Connection::open_memory(Some(&format!(
                "THREAD_FALLBACK_{}",
                test_name.unwrap_or_default()
            )))
        } else {
            let threads_dir = paths::data_dir().join("threads");
            std::fs::create_dir_all(&threads_dir)?;
            let sqlite_path = threads_dir.join("threads.db");
            Connection::open_file(&sqlite_path.to_string_lossy())
        };

        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                data_type TEXT NOT NULL,
                data BLOB NOT NULL
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create threads table: {}", e))?;

        // Schema version tracking
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS schema_versions (
                domain TEXT PRIMARY KEY,
                version INTEGER NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create schema_versions table: {}", e))?;

        // Chat sessions table with foreign key to threads
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS chat_sessions (
                session_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
                agent_id TEXT NOT NULL DEFAULT 'native',
                agent_type TEXT NOT NULL DEFAULT 'builtin',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                message_count INTEGER NOT NULL DEFAULT 0,
                pending_embedding INTEGER NOT NULL DEFAULT 1,
                schema_version INTEGER NOT NULL DEFAULT 1
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create chat_sessions table: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_agent_id ON chat_sessions(agent_id)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_chat_sessions_agent_id: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_agent_type ON chat_sessions(agent_type)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_chat_sessions_agent_type: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_pending ON chat_sessions(pending_embedding) WHERE pending_embedding = 1
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_chat_sessions_pending: {}", e))?;

        // Session embeddings table
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS session_embeddings (
                session_id TEXT PRIMARY KEY REFERENCES chat_sessions(session_id) ON DELETE CASCADE,
                embedding BLOB NOT NULL,
                embedding_model TEXT NOT NULL DEFAULT 'bge-small-en-v1.5',
                embedding_model_version TEXT NOT NULL DEFAULT '1.0',
                embedding_dimension INTEGER NOT NULL DEFAULT 384,
                content_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                schema_version INTEGER NOT NULL DEFAULT 1
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create session_embeddings table: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_session_embeddings_model ON session_embeddings(embedding_model, embedding_model_version)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_session_embeddings_model: {}", e))?;

        // Message embeddings cache table
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS message_embeddings (
                content_hash TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                embedding_model TEXT NOT NULL,
                embedding_model_version TEXT NOT NULL DEFAULT '1.0',
                embedding_dimension INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create message_embeddings table: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_message_embeddings_model ON message_embeddings(embedding_model, embedding_model_version)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_message_embeddings_model: {}", e))?;

        // Agent global embeddings table
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS agent_global_embeddings (
                agent_id TEXT PRIMARY KEY,
                agent_type TEXT NOT NULL,
                embedding BLOB NOT NULL,
                embedding_model TEXT NOT NULL DEFAULT 'bge-small-en-v1.5',
                embedding_model_version TEXT NOT NULL DEFAULT '1.0',
                embedding_dimension INTEGER NOT NULL DEFAULT 384,
                session_count INTEGER NOT NULL DEFAULT 0,
                aggregation_method TEXT NOT NULL DEFAULT 'mean',
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                schema_version INTEGER NOT NULL DEFAULT 1
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create agent_global_embeddings table: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_agent_global_agent_type ON agent_global_embeddings(agent_type)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_agent_global_agent_type: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_agent_global_model ON agent_global_embeddings(embedding_model, embedding_model_version)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_agent_global_model: {}", e))?;

        // Embedding jobs queue table
        connection.exec(indoc! {"
            CREATE TABLE IF NOT EXISTS embedding_jobs (
                job_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES chat_sessions(session_id) ON DELETE CASCADE,
                content_hash TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                retry_count INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
        "})?()
        .map_err(|e| anyhow!("Failed to create embedding_jobs table: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_embedding_jobs_status ON embedding_jobs(status) WHERE status IN ('pending', 'processing')
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_embedding_jobs_status: {}", e))?;

        connection.exec(indoc! {"
            CREATE INDEX IF NOT EXISTS idx_embedding_jobs_session ON embedding_jobs(session_id)
        "})?()
        .map_err(|e| anyhow!("Failed to create idx_embedding_jobs_session: {}", e))?;

        let db = Self {
            executor,
            connection: Arc::new(Mutex::new(connection)),
        };

        // Run migration to backfill existing threads
        db.migrate_existing_threads()?;

        Ok(db)
    }

    fn migrate_existing_threads(&self) -> Result<()> {
        let connection = self.connection.lock();
        
        // Check if migration has already been run
        let mut select = connection.select_bound::<(), (Arc<str>, i32)>(indoc! {"
            SELECT domain, version FROM schema_versions WHERE domain = 'chat_sessions'
        "})?;
        
        let rows = select(())?;
        if rows.into_iter().next().is_some() {
            // Migration already run
            return Ok(());
        }

        // Get all existing thread IDs
        let mut select_threads = connection.select_bound::<(), Arc<str>>(indoc! {"
            SELECT id FROM threads
        "})?;
        
        let thread_rows = select_threads(())?;
        let thread_ids: Vec<Arc<str>> = thread_rows.into_iter().collect();

        if thread_ids.is_empty() {
            // No threads to migrate, just record schema version
            connection.exec_bound::<(Arc<str>, i32)>(indoc! {"
                INSERT OR REPLACE INTO schema_versions (domain, version) VALUES (?, ?)
            "})?(("chat_sessions".into(), 1))?;
            return Ok(());
        }

        // Migrate each thread in a transaction
        connection.with_savepoint("migrate_threads", || {
            let now = chrono::Utc::now().to_rfc3339();
            for thread_id in &thread_ids {
                let agent_id = "native";
                let agent_type = "builtin";
                let message_count = 0; // Will be updated on next save

                connection.exec_bound::<(Arc<str>, &str, &str, &str, &str, i32, i32)>(indoc! {"
                    INSERT OR IGNORE INTO chat_sessions 
                    (session_id, agent_id, agent_type, created_at, updated_at, message_count, pending_embedding)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                "})?((thread_id.clone(), agent_id, agent_type, &now, &now, message_count, 1))?;
            }

            // Record schema version
            connection.exec_bound::<(Arc<str>, i32)>(indoc! {"
                INSERT OR REPLACE INTO schema_versions (domain, version) VALUES (?, ?)
            "})?(("chat_sessions".into(), 1))?;

            Ok(())
        })?;

        Ok(())
    }

    fn save_thread_sync(
        connection: &Arc<Mutex<Connection>>,
        id: acp::SessionId,
        thread: DbThread,
    ) -> Result<()> {
        const COMPRESSION_LEVEL: i32 = 3;

        #[derive(Serialize)]
        struct SerializedThread {
            #[serde(flatten)]
            thread: DbThread,
            version: &'static str,
        }

        let title = thread.title.to_string();
        let updated_at = thread.updated_at.to_rfc3339();
        let message_count = thread.messages.len() as i32;
        let agent_id_str = thread.agent_id.as_ref().map(|id| id.as_str()).unwrap_or("native").to_string();
        let agent_type_str = match thread.agent_type {
            Some(crate::AgentType::Builtin) => "builtin",
            Some(crate::AgentType::Custom) => "custom",
            None => "builtin",
        };
        
        // Clone thread for embedding extraction (before moving into SerializedThread)
        let messages_for_embedding = thread.messages.clone();
        
        let thread_for_json = SerializedThread {
            thread,
            version: DbThread::VERSION,
        };
        let json_data = serde_json::to_string(&thread_for_json)?;

        let connection = connection.lock();

        connection.with_savepoint("save_thread", || {
            // Update threads table
        let compressed = zstd::encode_all(json_data.as_bytes(), COMPRESSION_LEVEL)?;
        let data_type = DataType::Zstd;
        let data = compressed;

        let mut insert = connection.exec_bound::<(Arc<str>, String, String, DataType, Vec<u8>)>(indoc! {"
            INSERT OR REPLACE INTO threads (id, summary, updated_at, data_type, data) VALUES (?, ?, ?, ?, ?)
        "})?;

            insert((id.0.clone(), title.clone(), updated_at.clone(), data_type, data))?;

            // Update chat_sessions table - use ON CONFLICT to preserve created_at and pending_embedding
            let mut upsert_session = connection.exec_bound::<(Arc<str>, &str, &str, &str, i32)>(indoc! {"
                INSERT INTO chat_sessions 
                (session_id, agent_id, agent_type, updated_at, message_count)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(session_id) DO UPDATE SET
                    agent_id = excluded.agent_id,
                    agent_type = excluded.agent_type,
                    updated_at = excluded.updated_at,
                    message_count = excluded.message_count
            "})?;

            let session_id_str = id.0.to_string();
            upsert_session((id.0, &agent_id_str, agent_type_str, &updated_at, message_count))?;

            // Queue embedding job if messages exist
            if message_count > 0 {
                // Extract session text for embedding
                let session_text = crate::extract_session_text(&messages_for_embedding);
                if !session_text.is_empty() {
                    let content_hash = agent_memory::embedding::content_hash(&session_text);
                    
                    // Check if embedding needs update by comparing content_hash
                    // First get pending_embedding, then check content_hash separately
                    let mut check_pending = connection.select_bound::<&str, i32>(indoc! {"
                        SELECT pending_embedding FROM chat_sessions WHERE session_id = ? LIMIT 1
                    "})?;
                    
                    let mut check_hash = connection.select_bound::<&str, Option<String>>(indoc! {"
                        SELECT content_hash FROM session_embeddings WHERE session_id = ? LIMIT 1
                    "})?;
                    
                    let pending = check_pending(&session_id_str)?.into_iter().next().unwrap_or(1);
                    let stored_hash = check_hash(&session_id_str)?.into_iter().next().flatten();
                    
                    let needs_embedding = match (pending, stored_hash) {
                        (0, Some(stored_hash)) => {
                            // Has embedding - check if content hash changed
                            stored_hash != content_hash
                        }
                        (1, _) => true,  // Pending
                        (_, None) => true, // No embedding yet
                        _ => true,              // New session
                    };

                    if needs_embedding {
                        // Mark as pending
                        connection.exec_bound::<&str>(indoc! {"
                            UPDATE chat_sessions SET pending_embedding = 1 WHERE session_id = ?
                        "})?(&session_id_str)?;

                        // Queue embedding job
                        let job_id = format!("{}-{}", session_id_str, content_hash);
                        let now = chrono::Utc::now().to_rfc3339();
                        connection.exec_bound::<(&str, &str, &str, &str, &str)>(indoc! {"
                            INSERT OR IGNORE INTO embedding_jobs
                            (job_id, session_id, content_hash, status, created_at, updated_at)
                            VALUES (?, ?, ?, 'pending', ?, ?)
                        "})?((&job_id, &session_id_str, &content_hash, &now, &now))?;
                    }
                }
            }

            Ok(())
        })?;

        Ok(())
    }

    pub fn list_threads(&self) -> Task<Result<Vec<DbThreadMetadata>>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            let connection = connection.lock();

            let mut select =
                connection.select_bound::<(), (Arc<str>, String, String)>(indoc! {"
                SELECT id, summary, updated_at FROM threads ORDER BY updated_at DESC
            "})?;

            let rows = select(())?;
            let mut threads = Vec::new();

            for (id, summary, updated_at) in rows {
                threads.push(DbThreadMetadata {
                    id: acp::SessionId(id),
                    title: summary.into(),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
                });
            }

            Ok(threads)
        })
    }

    pub fn load_thread(&self, id: acp::SessionId) -> Task<Result<Option<DbThread>>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            let connection = connection.lock();
            let mut select = connection.select_bound::<Arc<str>, (DataType, Vec<u8>)>(indoc! {"
                SELECT data_type, data FROM threads WHERE id = ? LIMIT 1
            "})?;

            let rows = select(id.0)?;
            if let Some((data_type, data)) = rows.into_iter().next() {
                let json_data = match data_type {
                    DataType::Zstd => {
                        let decompressed = zstd::decode_all(&data[..])?;
                        String::from_utf8(decompressed)?
                    }
                    DataType::Json => String::from_utf8(data)?,
                };
                let thread = DbThread::from_json(json_data.as_bytes())?;
                Ok(Some(thread))
            } else {
                Ok(None)
            }
        })
    }

    pub fn save_thread(&self, id: acp::SessionId, thread: DbThread) -> Task<Result<()>> {
        let connection = self.connection.clone();

        self.executor
            .spawn(async move { Self::save_thread_sync(&connection, id, thread) })
    }

    pub fn delete_thread(&self, id: acp::SessionId) -> Task<Result<()>> {
        let connection = self.connection.clone();

        self.executor.spawn(async move {
            let connection = connection.lock();

            let mut delete = connection.exec_bound::<Arc<str>>(indoc! {"
                DELETE FROM threads WHERE id = ?
            "})?;

            delete(id.0)?;

            Ok(())
        })
    }

    /// Get the database connection
    pub fn connection(&self) -> &Arc<Mutex<Connection>> {
        &self.connection
    }

    /// Create a vector store that uses this database's connection
    pub fn vector_store(&self) -> agent_memory::SQLiteVectorStore {
        agent_memory::SQLiteVectorStore::new(
            self.executor.clone(),
            self.connection.clone(),
        )
    }
}
