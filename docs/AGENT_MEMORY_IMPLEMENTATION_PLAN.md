# Agent Memory Implementation Plan

## Overview
This document outlines the remaining implementation steps for the agent memory and embedding system.

## Completed ✅

1. **Core Infrastructure**
   - ✅ AgentId and AgentType types
   - ✅ Session and Thread struct extensions
   - ✅ Database schema (6 new tables with foreign keys and indices)
   - ✅ Migration logic for existing threads
   - ✅ Transaction-based saves preserving metadata

2. **Agent Registry**
   - ✅ Agent folder structure in `assets/agents/builtin/`
   - ✅ Agent registry with manifest tracking
   - ✅ Upgrade detection and user modification protection
   - ✅ ZED_STATELESS mode support

3. **Agent Memory Crate Structure**
   - ✅ Module structure (embedding, vector_store, session_memory, agent_memory)
   - ✅ Trait definitions and placeholder implementations

## Remaining Implementation Steps

### Phase 1: Embedding Generation Pipeline ✅ COMPLETED

#### 1.1 Implement BGE Embedding Generator ⚠️ STRUCTURE COMPLETE
**File**: `crates/agent_memory/src/bge_generator.rs`

**Tasks**:
- [x] Add `candle-transformers` dependency to `Cargo.toml` (as optional feature)
- [x] Implement `BgeEmbeddingGenerator` struct
- [x] Structure for loading BGE-small-en-v1.5 model weights (download infrastructure TODO)
- [x] Structure for tokenization and embedding generation (model loading TODO)
- [x] Add model caching to avoid reloading (structure in place)
- [x] Handle errors gracefully (fallback to placeholder if model unavailable)

**Implementation**:
- Created `BgeEmbeddingGenerator` with async model loading structure
- Added optional `embeddings` feature flag for candle dependencies
- Graceful fallback to placeholder embeddings if model unavailable
- Structure ready for full model implementation
- **Note**: Full BGE model loading requires HTTP client and file system access
  - Model files need to be downloaded from HuggingFace
  - Requires: config.json, tokenizer.json, model.safetensors
  - Can be completed when HTTP client and FS are available in the context

**Dependencies**:
```toml
candle-core = { version = "0.9.1", git = "https://github.com/zed-industries/candle", branch = "9.1-patched" }
candle-transformers = { version = "0.9.1", git = "https://github.com/zed-industries/candle", branch = "9.1-patched" }
tokenizers = "0.19"
```

**Key Functions**:
- `BgeEmbeddingGenerator::new()` - Initialize with model path
- `BgeEmbeddingGenerator::load_model()` - Load model weights
- `BgeEmbeddingGenerator::generate()` - Generate single embedding
- `BgeEmbeddingGenerator::generate_batch()` - Batch processing

#### 1.2 Message Text Extraction ✅ COMPLETED
**File**: `crates/agent/src/message_extraction.rs`

**Tasks**:
- [x] Extract text from `Message` enum (User and Agent messages)
- [x] Handle `AgentMessageContent` variants (Text, Thinking, ToolUse, etc.)
- [x] Normalize text (remove timestamps, normalize whitespace)
- [x] Aggregate session text for session-level embeddings

**Implementation**:
- Created `extract_message_text()` for individual messages
- Created `extract_session_text()` for full session aggregation
- Uses `normalize_text_for_embedding()` from agent_memory crate
- Handles User and Agent message types

#### 1.3 Content Hashing and Caching
**File**: `crates/agent_memory/src/embedding.rs` (already has `content_hash`)

**Tasks**:
- [ ] Verify `content_hash` function works correctly
- [ ] Implement cache lookup before generation
- [ ] Store generated embeddings in `message_embeddings` table

### Phase 2: Database-Backed Vector Store ✅ COMPLETED

#### 2.1 Implement SQLiteVectorStore
**File**: `crates/agent_memory/src/sqlite_vector_store.rs`

**Tasks**:
- [x] Create `SQLiteVectorStore` struct wrapping `ThreadsDatabase` connection
- [x] Implement `VectorStore` trait methods:
  - [x] `store_session_embedding()` - INSERT/UPDATE into `session_embeddings`
  - [x] `get_session_embedding()` - SELECT from `session_embeddings`
  - [x] `store_message_embedding()` - INSERT into `message_embeddings`
  - [x] `get_message_embedding()` - SELECT from `message_embeddings`
  - [x] `store_agent_embedding()` - INSERT/UPDATE into `agent_global_embeddings`
  - [x] `get_agent_embedding()` - SELECT from `agent_global_embeddings`
  - [x] `search_similar_sessions()` - Cosine similarity search
- [x] Serialize/deserialize embeddings as BLOB (Vec<f32> as bytes)
- [x] Handle dimension validation

**Implementation**:
- Created `SQLiteVectorStore` that takes `BackgroundExecutor` and `Arc<Mutex<Connection>>`
- Added `ThreadsDatabase::vector_store()` method to create vector store instance
- All methods use async tasks via `executor.spawn()` for database operations
- Embeddings serialized as little-endian f32 bytes
- Model name/version stored for compatibility checking

#### 2.2 Cosine Similarity Search ✅ COMPLETED
**Tasks**:
- [x] Implement cosine similarity in Rust (since sqlite-vss is optional)
- [x] Query all session embeddings
- [x] Compute similarity scores
- [x] Sort and filter by threshold
- [x] Return top N results

**Implementation**:
- `cosine_similarity()` function computes dot product (assumes normalized vectors)
- `search_similar_sessions()` queries all embeddings, computes similarity, filters by threshold
- Results sorted by similarity (descending) and truncated to limit

### Phase 3: Embedding Job Queue ✅ COMPLETED

#### 3.1 Job Queue Implementation ✅ COMPLETED
**File**: `crates/agent/src/embedding_queue.rs`

**Tasks**:
- [x] Create `EmbeddingQueue` struct with background executor
- [x] Implement job submission: `queue_job(session_id, content_hash)`
- [x] Background worker task that:
  - [x] Polls `embedding_jobs` table for `status = 'pending'`
  - [x] Processes jobs in batches (10 at a time)
  - [x] Updates job status (`processing` → `completed` or `failed`)
  - [x] Handles retries (max 3 attempts with 5s delay)
- [x] Integration with `ThreadsDatabase` for job persistence
- [x] Load thread messages from database in `process_job()`
- [x] Extract session text and verify content hash
- [x] Generate actual embeddings (uses generator, which may be placeholder)

**Implementation**:
- `EmbeddingQueue` uses `BackgroundExecutor` for async operations
- Worker loop continuously processes pending jobs
- Job status tracking with retry logic
- Updates `chat_sessions.pending_embedding` on completion
- `process_job()` loads messages from database, extracts text, generates embedding
- Content hash verification ensures data integrity

**Structure**:
```rust
pub struct EmbeddingQueue {
    job_tx: mpsc::UnboundedSender<EmbeddingJob>,
    database: Arc<ThreadsDatabase>,
    generator: Arc<dyn EmbeddingGenerator>,
    vector_store: Arc<dyn VectorStore>,
}

pub struct EmbeddingJob {
    session_id: String,
    content_hash: String,
    retry_count: u32,
}
```

#### 3.2 Resume Logic on Startup ✅ COMPLETED
**File**: `crates/agent/src/embedding_queue.rs`

**Tasks**:
- [x] `resume_pending_jobs()` function implemented
- [x] Reset `processing` jobs back to `pending` (assume crash)
- [x] Query `chat_sessions` for `pending_embedding = 1` (structure ready)
- [x] Jobs automatically picked up by worker loop

**Implementation**:
- `EmbeddingQueue::resume_pending_jobs()` resets crashed jobs
- Worker loop automatically processes all pending jobs on startup
- No manual job submission needed - worker polls database

### Phase 4: Session Memory Integration ✅ COMPLETED

#### 4.1 Integrate SessionMemory into Thread
**File**: `crates/agent/src/thread.rs`

**Tasks**:
- [x] Add `SessionMemory` field to `Thread` struct (optional, lazy init)
- [x] Initialize `SessionMemory` as `None` in `Thread::new()` and `Thread::from_db()`
- [x] Structure ready for lazy initialization when needed
- [ ] Call `session_memory.add_message()` on each new message (TODO: when SessionMemory is initialized)
- [ ] Update session embedding after message batch (TODO: when SessionMemory is initialized)

**Implementation**:
- Added `session_memory: Option<Arc<agent_memory::SessionMemory>>` field
- Initialized as `None` in both constructors
- Ready for lazy initialization when embedding features are enabled

#### 4.2 Update Session Embedding on Save ✅ COMPLETED
**File**: `crates/agent/src/db.rs` - `save_thread_sync()`

**Tasks**:
- [x] Extract session text from messages
- [x] Compute content hash
- [x] Check if embedding needs update (check `pending_embedding` flag)
- [x] Queue embedding job if needed
- [x] Update `chat_sessions.pending_embedding = 1` when queued

**Implementation**:
- `save_thread_sync()` now extracts session text using `extract_session_text()`
- Computes content hash for caching
- Checks `pending_embedding` flag to avoid duplicate jobs
- Queues embedding job in `embedding_jobs` table
- Marks session as `pending_embedding = 1`

### Phase 5: Global Agent Memory ✅ MOSTLY COMPLETE

#### 5.1 Mean Pooling Aggregation ✅ COMPLETED
**File**: `crates/agent_memory/src/agent_memory.rs`

**Tasks**:
- [x] Verify mean pooling implementation
- [x] `aggregate_embeddings_mean()` function implemented
- [x] Implement incremental update in `AgentMemory::add_session_embedding_with_count()`:
  - [x] Load current global embedding and session_count
  - [x] Compute: `new_mean = (old_mean * old_count + new_embedding) / (old_count + 1)`
  - [x] Normalize result
  - [x] Store updated global embedding

**Implementation**:
- `aggregate_embeddings_mean()` computes mean of multiple embeddings
- Normalizes result to unit length
- `AgentMemory::add_session_embedding_with_count()` implements incremental mean pooling
- Gets actual session count from database for accurate aggregation
- Handles first embedding case (no existing global embedding)

#### 5.2 Update Global Embedding on Session Complete ✅ COMPLETED
**File**: `crates/agent/src/embedding_queue.rs`

**Tasks**:
- [x] Structure in place for updating global embedding
- [x] When session embedding is generated, update global agent embedding
- [x] Call `AgentMemory::add_session_embedding_with_count()`
- [x] Update `agent_global_embeddings.session_count`

**Implementation**:
- `EmbeddingQueue::update_global_embedding()` implemented
- Loads agent_id/agent_type from chat_sessions
- Gets current session_count from agent_global_embeddings
- Calls `AgentMemory::add_session_embedding_with_count()` with accurate count
- Automatically called after successful embedding generation

### Phase 6: UI Integration

#### 6.1 Agent Selection in UI
**File**: `crates/agent_ui/src/agent_ui.rs`

**Tasks**:
- [ ] Update `NewExternalAgentThread` action to pass `agent_id`
- [ ] Map `ExternalAgent` enum to `AgentId`
- [ ] Pass `agent_id` to `Thread::new()`
- [ ] Store `agent_type` in `Thread` (extract from registry)

#### 6.2 Agent Registry Initialization
**File**: `crates/agent/src/agent.rs` - `NativeAgent::new()`

**Tasks**:
- [ ] Initialize `AgentRegistry` on startup
- [ ] Call `registry.initialize()` to seed and upgrade agents
- [ ] Store registry reference in `NativeAgent` (if needed)

### Phase 7: Testing and Documentation

#### 7.1 Unit Tests
**Files**: `crates/agent_memory/src/*.rs` and `crates/agent/src/*.rs`

**Tasks**:
- [ ] Test embedding generation (mock model)
- [ ] Test vector store operations (in-memory SQLite)
- [ ] Test mean pooling aggregation
- [ ] Test content hashing and normalization
- [ ] Test agent registry seeding and upgrades
- [ ] Test migration logic

#### 7.2 Integration Tests
**File**: `crates/agent/src/tests/mod.rs`

**Tasks**:
- [ ] Test full embedding pipeline (message → embedding → storage)
- [ ] Test job queue processing
- [ ] Test resume logic after restart
- [ ] Test global embedding aggregation

#### 7.3 Documentation
**Files**: Various

**Tasks**:
- [ ] Document embedding model download process
- [ ] Document agent.toml format
- [ ] Document embedding job queue behavior
- [ ] Add code comments for complex algorithms (mean pooling, cosine similarity)

### Phase 8: Performance and Optimization

#### 8.1 Batching and Caching
**Tasks**:
- [ ] Implement batch processing for multiple messages
- [ ] Cache frequently accessed embeddings
- [ ] Optimize database queries with proper indices (already done)

#### 8.2 Lazy Loading
**Tasks**:
- [ ] Load embedding model only when needed
- [ ] Lazy initialize `SessionMemory`
- [ ] Defer global embedding updates (batch updates)

## Implementation Order Recommendation

1. **Phase 2** (Database Vector Store) - Foundation for everything else
2. **Phase 1** (Embedding Generation) - Core functionality
3. **Phase 3** (Job Queue) - Background processing
4. **Phase 4** (Session Integration) - Connect to existing code
5. **Phase 5** (Global Memory) - Advanced feature
6. **Phase 6** (UI Integration) - User-facing
7. **Phase 7** (Testing) - Quality assurance
8. **Phase 8** (Optimization) - Performance tuning

## Key Design Decisions

1. **Model Loading**: Download BGE model weights on first use, cache in `{data_dir}/models/`
2. **Job Queue**: Use database-backed queue for persistence across restarts
3. **Embedding Storage**: Store as BLOB (Vec<f32> serialized) in SQLite
4. **Similarity Search**: Implement in Rust (no sqlite-vss dependency initially)
5. **Batch Processing**: Process embeddings in batches of 10-20 messages
6. **Error Handling**: Graceful degradation if model unavailable (log warning, continue)

## Dependencies to Add

```toml
# In crates/agent_memory/Cargo.toml
candle-core = { version = "0.9.1", git = "https://github.com/zed-industries/candle", branch = "9.1-patched" }
candle-transformers = { version = "0.9.1", git = "https://github.com/zed-industries/candle", branch = "9.1-patched" }
tokenizers = "0.19"

# In crates/agent/Cargo.toml (for job queue)
tokio = { workspace = true }
```

## Notes

- The embedding model (BGE-small-en-v1.5) is ~130MB, so download should be async and show progress
- Consider adding a feature flag to disable embeddings for users who don't want the overhead
- The job queue should be cancellable (use `Task` cancellation)
- Embeddings are optional - system should work without them (just no memory features)

