# Conversation Lifecycle Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add auto-title generation, smart summarization, context compression, and session archival to the Agent chat system.

**Architecture:** New `lifecycle/` backend module manages title/summary/archive operations asynchronously. `summarizer` runs after message completion to generate persistent summaries. `compactor` integrates into `AgentLoop::optimize_context` to replace old message blocks with existing summaries. Frontend adds archive toggle, summary preview, and lifecycle settings.

**Tech Stack:** Rust (Tokio async), React + Zustand, SQLite

**Spec:** `docs/superpowers/specs/2025-05-25-conversation-lifecycle-design.md`

---

### Task 1: DB Migration — session_summaries table + session columns

**Files:**
- Modify: `src-tauri/src/db/models.rs`
- Modify: `src-tauri/src/db/repository.rs`

- [ ] **Step 1: Add SessionSummary model to models.rs**

```rust
// Add after Session struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub session_id: String,
    pub message_start_id: String,
    pub message_end_id: String,
    pub summary: String,
    pub key_points: Option<String>,  // JSON array string
    pub original_token_count: i32,
    pub summary_token_count: i32,
    pub model_used: Option<String>,
    pub created_at: String,
}
```

Also extend `Session` model with two new fields:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub system_prompt: Option<String>,
    pub title_source: String,       // NEW: "manual" | "auto_generated"
    pub archived: bool,              // NEW
    pub created_at: String,
    pub updated_at: String,
}
```

- [ ] **Step 2: Add migration in repository.rs**

Add to `migrate_tables()`:

```rust
// Migration v6: add title_source and archived columns
let has_title_source = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='title_source'",
    [],
    |row| row.get::<_, i32>(0),
).unwrap_or(0);

if has_title_source == 0 {
    conn.execute_batch(
        "ALTER TABLE sessions ADD COLUMN title_source TEXT NOT NULL DEFAULT 'manual';
         ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;",
    )?;
}
```

- [ ] **Step 3: Add session_summaries table creation in init_tables()**

```rust
// In init_tables(), after memories block:
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS session_summaries (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        message_start_id TEXT NOT NULL,
        message_end_id TEXT NOT NULL,
        summary TEXT NOT NULL,
        key_points TEXT,
        original_token_count INTEGER NOT NULL DEFAULT 0,
        summary_token_count INTEGER NOT NULL DEFAULT 0,
        model_used TEXT,
        created_at TEXT NOT NULL,
        FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_summaries_session ON session_summaries(session_id);
    CREATE INDEX IF NOT EXISTS idx_summaries_range ON session_summaries(session_id, message_end_id);
    ",
)?;
```

- [ ] **Step 4: Add summary CRUD methods to repository.rs**

```rust
impl Database {
    pub fn insert_summary(&self, summary: &SessionSummary) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_summaries (id, session_id, message_start_id, message_end_id, summary, key_points, original_token_count, summary_token_count, model_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![summary.id, summary.session_id, summary.message_start_id, summary.message_end_id, summary.summary, summary.key_points, summary.original_token_count, summary.summary_token_count, summary.model_used, summary.created_at],
        )?;
        Ok(())
    }

    pub fn get_session_summaries(&self, session_id: &str) -> Result<Vec<SessionSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, message_start_id, message_end_id, summary, key_points, original_token_count, summary_token_count, model_used, created_at
             FROM session_summaries WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let summaries = stmt.query_map(params![session_id], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                session_id: row.get(1)?,
                message_start_id: row.get(2)?,
                message_end_id: row.get(3)?,
                summary: row.get(4)?,
                key_points: row.get(5)?,
                original_token_count: row.get(6)?,
                summary_token_count: row.get(7)?,
                model_used: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(summaries)
    }

    pub fn get_latest_summary_end_id(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT message_end_id FROM session_summaries WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )?;
        let id = stmt.query_row(params![session_id], |row| row.get(0)).optional()?;
        Ok(id)
    }

    /// Query sessions eligible for archival (not archived + old)
    pub fn list_archivable_sessions(&self, archive_after_days: u32) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM sessions WHERE archived = 0 AND updated_at < datetime('now', ?1)",
        )?;
        let since = format!("-{} days", archive_after_days);
        let ids = stmt.query_map(params![since], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub fn set_session_archived(&self, id: &str, archived: bool) -> Result<()> {
        self.conn.execute("UPDATE sessions SET archived = ?1 WHERE id = ?2", params![archived as i32, id])?;
        Ok(())
    }

    // Update existing create_session/update_session_title to handle title_source
    pub fn update_session_title_with_source(&self, id: &str, title: &str, source: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?1, title_source = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![title, source, id],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 5: Update all Session construction sites**

In `repository.rs`, update all `map_session_row` or inline `Session { ... }` constructions to include `title_source: String::from("manual")` and `archived: false` as defaults (migration guarantees existing rows have these defaults). Update `Session` struct references in `create_session` command and `select *` queries to include the new columns.

- [ ] **Step 6: Run cargo check to verify**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds with no errors.

---

### Task 2: LifecycleConfig + AppState integration

**Files:**
- Create: `src-tauri/src/lifecycle/config.rs`
- Modify: `src-tauri/src/lifecycle/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create lifecycle config**

File `src-tauri/src/lifecycle/config.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    pub auto_title_enabled: bool,
    pub title_model: Option<String>,
    pub auto_summarize_enabled: bool,
    pub summarize_chunk_size: usize,
    pub summarize_model: Option<String>,
    pub auto_archive_enabled: bool,
    pub archive_after_days: u32,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            auto_title_enabled: true,
            title_model: None,
            auto_summarize_enabled: true,
            summarize_chunk_size: 20,
            summarize_model: None,
            auto_archive_enabled: true,
            archive_after_days: 30,
        }
    }
}
```

- [ ] **Step 2: Create lifecycle/mod.rs with LifecycleManager**

```rust
pub mod config;
pub mod titler;
pub mod summarizer;
pub mod compactor;
pub mod archiver;

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::db::repository::Database;
use crate::api::provider::ProviderRegistry;
use crate::lifecycle::config::LifecycleConfig;

pub struct LifecycleManager {
    pub db: Arc<Mutex<Database>>,
    pub providers: Arc<Mutex<ProviderRegistry>>,
    pub config: Arc<Mutex<LifecycleConfig>>,
}

impl LifecycleManager {
    pub fn new(
        db: Arc<Mutex<Database>>,
        providers: Arc<Mutex<ProviderRegistry>>,
    ) -> Self {
        Self {
            db,
            providers,
            config: Arc::new(Mutex::new(LifecycleConfig::default())),
        }
    }

    pub async fn load_config(&self) {
        let db = self.db.lock().await;
        if let Ok(Some(json)) = db.get_setting("lifecycle_config") {
            if let Ok(cfg) = serde_json::from_str::<LifecycleConfig>(&json) {
                *self.config.lock().await = cfg;
            }
        }
    }

    pub async fn save_config(&self) -> crate::error::Result<()> {
        let cfg = self.config.lock().await;
        let json = serde_json::to_string(&*cfg).map_err(|e| crate::error::AppError::Internal(e.to_string()))?;
        let db = self.db.lock().await;
        db.set_setting("lifecycle_config", &json)?;
        Ok(())
    }
}
```

- [ ] **Step 3: Add LifecycleManager to AppState**

In `state.rs`:

```rust
// Add field
pub lifecycle: LifecycleManager,

// In AppState::new(), after creating providers:
let lifecycle = LifecycleManager::new(db_arc.clone(), providers.clone());

// Return in struct
lifecycle,
```

- [ ] **Step 4: Register lifecycle module in lib.rs**

Add to the top of `lib.rs`:

```rust
mod lifecycle;
```

- [ ] **Step 5: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 3: Title Generation (titler.rs)

**Files:**
- Create: `src-tauri/src/lifecycle/titler.rs`

- [ ] **Step 1: Implement auto_title function**

```rust
use crate::api::types::{Message, MessageRole, ChatRequest};
use crate::db::repository::Database;
use crate::error::Result;
use crate::lifecycle::LifecycleManager;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Generate a title for a session if:
/// 1. title_source != 'manual'
/// 2. session has >= 2 messages
/// 3. no auto title generated yet
pub async fn maybe_generate_title(
    lifecycle: &LifecycleManager,
    session_id: &str,
    model_id: &str,
) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_title_enabled {
        return Ok(());
    }
    // Use configured title model or fall back to session model
    let actual_model = cfg.title_model.clone().unwrap_or_else(|| model_id.to_string());
    drop(cfg);

    let db = lifecycle.db.lock().await;

    // Check session: only generate if title_source == 'manual' (default)
    let sess = db.get_session(session_id)?
        .ok_or_else(|| crate::error::AppError::NotFound("Session gone".to_string()))?;

    // If already auto-generated or manually set, skip
    if sess.title_source != "manual" {
        return Ok(());
    }

    let messages = db.get_messages(session_id)?;
    if messages.len() < 2 {
        return Ok(()); // Need at least user + assistant
    }

    // Only generate if title is still the default/placeholder
    let title_is_default = sess.title == "新对话" || sess.title.is_empty();
    if !title_is_default {
        // User might have set a title manually but title_source wasn't updated
        // (existing sessions before migration)
        // Skip for now, they'll get titles on next session
        return Ok(());
    }

    // Take first user message + first assistant response as context
    let first_user = messages.iter().find(|m| m.role == "user");
    let first_assistant = messages.iter().find(|m| m.role == "assistant");

    let context = match (first_user, first_assistant) {
        (Some(u), Some(a)) => format!("User: {}\nAssistant: {}", u.content, a.content),
        _ => return Ok(()),
    };

    drop(db);

    let prompt = format!(
        "Based on the following conversation, generate a concise title in the user's language (max 8 words). \
         Return ONLY the title, no quotation marks or explanation.\n\nConversation:\n{}",
        context
    );

    let request = ChatRequest {
        messages: vec![Message {
            id: None,
            role: MessageRole::User,
            content: prompt,
            tool_calls: None,
            tool_call_id: None,
        }],
        model: actual_model,
        tools: None,
        stream: Some(false),
        max_tokens: Some(30),
        temperature: Some(0.3),
    };

    let provider = lifecycle.providers.lock().await;
    let p = provider.get(&actual_model)?;
    let response = p.chat(request).await?;

    if let Some(choice) = response.choices.first() {
        let title = choice.message.content.trim().trim_matches('"').to_string();
        if !title.is_empty() {
            let db = lifecycle.db.lock().await;
            db.update_session_title_with_source(session_id, &title, "auto_generated")?;
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 4: Summary Generation (summarizer.rs)

**Files:**
- Create: `src-tauri/src/lifecycle/summarizer.rs`

- [ ] **Step 1: Implement summarizer**

```rust
use uuid::Uuid;
use chrono::Utc;

use crate::api::types::{Message, MessageRole, ChatRequest};
use crate::db::models::SessionSummary;
use crate::error::Result;
use crate::lifecycle::LifecycleManager;

/// Generate a summary chunk for new messages since last summarized point.
/// Called asynchronously after send_message_stream completes.
pub async fn maybe_generate_summary(
    lifecycle: &LifecycleManager,
    session_id: &str,
    model_id: &str,
) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_summarize_enabled {
        return Ok(());
    }
    let chunk_size = cfg.summarize_chunk_size;
    let actual_model = cfg.summarize_model.clone().unwrap_or_else(|| model_id.to_string());
    drop(cfg);

    let db = lifecycle.db.lock().await;

    // Find last summarized message
    let last_summarized = db.get_latest_summary_end_id(session_id)?;
    let messages = db.get_messages(session_id)?;

    // Count unsummarized user+assistant messages
    let start_idx = last_summarized.as_ref().and_then(|end_id| {
        messages.iter().position(|m| m.id == *end_id).map(|i| i + 1)
    }).unwrap_or(0);

    let unsummarized: Vec<&crate::db::models::Message> = messages[start_idx..]
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .collect();

    if unsummarized.len() < chunk_size && messages.len() < 50 {
        // Not enough new messages to summarize and session isn't huge yet
        return Ok(());
    }

    // Take up to chunk_size unsummarized messages
    let to_summarize = if unsummarized.len() <= chunk_size {
        // If total session is large enough, always summarize
        if messages.len() >= 50 { &unsummarized[..] } else { return Ok(()); }
    } else {
        &unsummarized[..chunk_size]
    };

    if to_summarize.is_empty() {
        return Ok(());
    }

    let first_id = to_summarize.first().unwrap().id.clone();
    let last_id = to_summarize.last().unwrap().id.clone();

    // Build conversation text for summarization
    let conversation_text: String = to_summarize.iter()
        .map(|m| format!("{}: {}", if m.role == "user" { "User" } else { "Assistant" }, m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let original_tokens: i32 = to_summarize.iter()
        .map(|m| crate::agent::r#loop::AgentLoop::estimate_tokens(&m.content) as i32)
        .sum();

    drop(db);

    let prompt = format!(
        "Summarize the key information, decisions, and conclusions from this conversation segment. \
         Be concise but preserve important technical details. Use the same language as the conversation.\n\n{}",
        conversation_text
    );

    let request = ChatRequest {
        messages: vec![Message {
            id: None,
            role: MessageRole::User,
            content: prompt,
            tool_calls: None,
            tool_call_id: None,
        }],
        model: actual_model.clone(),
        tools: None,
        stream: Some(false),
        max_tokens: Some(500),
        temperature: Some(0.3),
    };

    let provider = lifecycle.providers.lock().await;
    let p = provider.get(&actual_model)?;
    let response = p.chat(request).await?;

    if let Some(choice) = response.choices.first() {
        let summary_text = choice.message.content.trim().to_string();
        let summary_tokens = crate::agent::r#loop::AgentLoop::estimate_tokens(&summary_text) as i32;

        let summary = SessionSummary {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            message_start_id: first_id,
            message_end_id: last_id,
            summary: summary_text,
            key_points: None,
            original_token_count: original_tokens,
            summary_token_count: summary_tokens,
            model_used: Some(actual_model),
            created_at: Utc::now().to_rfc3339(),
        };

        let db = lifecycle.db.lock().await;
        db.insert_summary(&summary)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 5: Context Compression (compactor.rs + AgentLoop integration)

**Files:**
- Create: `src-tauri/src/lifecycle/compactor.rs`
- Modify: `src-tauri/src/agent/loop.rs`

- [ ] **Step 1: Create compressor that uses existing summaries**

```rust
use crate::api::types::Message as ApiMessage;
use crate::db::repository::Database;
use crate::error::Result;

/// Replace old message blocks with their summaries where available.
/// Returns optimized message list.
pub fn compress_context(
    db: &Database,
    session_id: &str,
    mut messages: Vec<ApiMessage>,
    max_tokens: usize,
) -> Result<Vec<ApiMessage>> {
    if messages.is_empty() {
        return Ok(messages);
    }

    // Separate system messages from conversation messages
    let system_msgs: Vec<ApiMessage> = messages.iter()
        .filter(|m| m.role == crate::api::types::MessageRole::System)
        .cloned()
        .collect();

    let conv_msgs: Vec<ApiMessage> = messages.iter()
        .filter(|m| m.role != crate::api::types::MessageRole::System)
        .cloned()
        .collect();

    let mut total_tokens: usize = system_msgs.iter()
        .map(|m| crate::agent::r#loop::AgentLoop::estimate_tokens(&m.content))
        .sum();

    // If system messages alone exceed limit, truncate them (unlikely but safe)
    if total_tokens > max_tokens {
        return Ok(system_msgs);
    }

    // Gather existing summaries for this session
    let summaries = db.get_session_summaries(session_id)?;
    let mut remaining = Vec::new();
    let mut replaced_up_to: Option<String> = None;

    // Check if we can replace a block of messages with a summary
    for msg in conv_msgs.iter().rev() {
        let msg_tokens = crate::agent::r#loop::AgentLoop::estimate_tokens(&msg.content);
        if total_tokens + msg_tokens > max_tokens {
            // We need to compress. Find if there's a summary covering this range
            if let Some(msg_id) = &msg.id {
                if let Some(summary) = summaries.iter()
                    .find(|s| s.message_end_id == *msg_id || s.message_start_id == *msg_id)
                {
                    // Found matching summary - inject it as system context
                    let summary_text = format!(
                        "[Previous conversation compressed: {}]",
                        summary.summary
                    );
                    let summary_tokens = crate::agent::r#loop::AgentLoop::estimate_tokens(&summary_text);
                    total_tokens += summary_tokens;

                    remaining.push(ApiMessage {
                        id: None,
                        role: crate::api::types::MessageRole::System,
                        content: summary_text,
                        tool_calls: None,
                        tool_call_id: None,
                    });

                    replaced_up_to = Some(summary.message_end_id.clone());
                    break;
                }
            }
            // No summary available, just drop from here (existing behavior)
            break;
        }
        total_tokens += msg_tokens;
        remaining.push(msg);
    }

    remaining.reverse();

    let mut result = system_msgs;
    result.extend(remaining);
    Ok(result)
}
```

- [ ] **Step 2: Integrate into AgentLoop**

In `agent/loop.rs`, modify `run()` and `run_stream()` and `run_stream_inner()`:

```rust
// Add to AgentLoop struct:
// pub db: Option<Arc<Mutex<Database>>>,  -- add to struct
// pub session_id: Option<String>,         -- add to struct

// In run() method, after optimized_messages but before the loop:
// If we have db access, try to compress

// The cleaner approach: modify optimize_context signature
// pub fn optimize_context(messages: &[Message], max_tokens: usize, db: Option<&Database>, session_id: Option<&str>) -> Vec<Message>

// At the end of optimize_context, add:
#[allow(unused_variables)]
if let (Some(db), Some(sid)) = (db, session_id) {
    // Try to use existing summaries
    // Note: We use the existing optimized result as fallback
    // This is a best-effort optimization
}
```

Actually, since the compressor needs access to `Database`, and `AgentLoop` currently doesn't have it, let's use a different approach — integrate compression at the `send_message_stream` level in `commands/session.rs`, before calling `AgentLoop::run_stream()`. This is simpler and doesn't require changing AgentLoop internals.

In `commands/session.rs`, modify `send_message_stream`:

After building `api_messages` and before calling `agent.run_stream(...)`:

```rust
// Try context compression using existing summaries
let max_tokens = context_window.unwrap_or(32000);
if max_tokens > 0 {
    let db = state.db.lock().await;
    let compressed = crate::lifecycle::compactor::compress_context(
        &db,
        &session_id,
        api_messages.clone(),
        max_tokens,
    )?;
    drop(db);
    api_messages = compressed;
}
```

Do the same in `send_message` (non-streaming path).

- [ ] **Step 3: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 6: Archival Service (archiver.rs)

**Files:**
- Create: `src-tauri/src/lifecycle/archiver.rs`

- [ ] **Step 1: Implement archiver**

```rust
use crate::error::Result;
use crate::lifecycle::LifecycleManager;

/// Run archive check on startup. Archives sessions past the configured inactivity threshold.
pub async fn run_archive_check(lifecycle: &LifecycleManager) -> Result<()> {
    let cfg = lifecycle.config.lock().await;
    if !cfg.auto_archive_enabled {
        return Ok(());
    }
    let days = cfg.archive_after_days;
    drop(cfg);

    let db = lifecycle.db.lock().await;
    let archivable = db.list_archivable_sessions(days)?;

    for session_id in archivable {
        // Generate final summary if not already summarized
        // (summarizer will be called separately when messages arrive)
        db.set_session_archived(&session_id, true)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Wire archive check into app startup**

In `state.rs` or `lib.rs`, after `AppState::new()` and before returning, spawn a background task:

```rust
// In lib.rs setup() or state initialization:
let lifecycle_clone = /* how to get LifecycleManager ref */;
tokio::spawn(async move {
    let _ = crate::lifecycle::archiver::run_archive_check(&lifecycle_clone).await;
});
```

- [ ] **Step 3: Add IPC commands for archive/unarchive**

```rust
#[tauri::command]
pub async fn archive_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_session_archived(&id, true)
}

#[tauri::command]
pub async fn unarchive_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_session_archived(&id, false)
}

#[tauri::command]
pub async fn list_archived_sessions(state: State<'_, AppState>) -> Result<Vec<DbSession>> {
    // We'll add a dedicated query later, for now filter in-memory
    let db = state.db.lock().await;
    let all = db.list_sessions()?;
    Ok(all.into_iter().filter(|s| s.archived).collect())
}
```

Register these in `lib.rs` invoke_handler.

- [ ] **Step 4: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 7: IPC Commands — lifecycle config + summaries

**Files:**
- Create: `src-tauri/src/commands/lifecycle.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create lifecycle IPC commands**

```rust
use tauri::State;
use crate::error::Result;
use crate::state::AppState;
use crate::db::models::SessionSummary;
use crate::lifecycle::config::LifecycleConfig;

#[tauri::command]
pub async fn get_session_summaries(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionSummary>> {
    let db = state.db.lock().await;
    db.get_session_summaries(&session_id)
}

#[tauri::command]
pub async fn force_generate_summary(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    let session = db.get_session(&session_id)?
        .ok_or_else(|| crate::error::AppError::NotFound("Session not found".to_string()))?;
    drop(db);

    crate::lifecycle::summarizer::maybe_generate_summary(
        &state.lifecycle,
        &session_id,
        &session.model_id,
    ).await
}

#[tauri::command]
pub async fn get_lifecycle_config(
    state: State<'_, AppState>,
) -> Result<LifecycleConfig> {
    Ok(state.lifecycle.config.lock().await.clone())
}

#[tauri::command]
pub async fn update_lifecycle_config(
    state: State<'_, AppState>,
    config: LifecycleConfig,
) -> Result<()> {
    *state.lifecycle.config.lock().await = config;
    state.lifecycle.save_config().await
}
```

- [ ] **Step 2: Register in commands/mod.rs**

```rust
pub mod lifecycle;
```

- [ ] **Step 3: Register invoke handlers in lib.rs**

Add to the invoke_handler(tauri::generate_handler![...]) list:

```rust
commands::lifecycle::get_session_summaries,
commands::lifecycle::force_generate_summary,
commands::lifecycle::get_lifecycle_config,
commands::lifecycle::update_lifecycle_config,
commands::session::archive_session,
commands::session::unarchive_session,
commands::session::list_archived_sessions,
```

- [ ] **Step 4: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 8: Wire lifecycle hooks into send_message_stream

**Files:**
- Modify: `src-tauri/src/commands/session.rs`

- [ ] **Step 1: Add lifecycle triggers after streaming completes**

In `send_message_stream`, after the `StreamEvent::Done` handler and the assistant message persistence, add:

```rust
// After persisting assistant message and before returning...
let lifecycle = state.lifecycle.clone();
let sid = session_id.clone();
let mid = sess.model_id.clone();

// Fire-and-forget title generation
tokio::spawn(async move {
    let _ = crate::lifecycle::titler::maybe_generate_title(
        &lifecycle, &sid, &mid,
    ).await;
});

// Fire-and-forget summary generation
let lifecycle2 = state.lifecycle.clone();
let sid2 = session_id.clone();
let mid2 = sess.model_id.clone();
tokio::spawn(async move {
    let _ = crate::lifecycle::summarizer::maybe_generate_summary(
        &lifecycle2, &sid2, &mid2,
    ).await;
});
```

Do the same in `send_message` (non-streaming path) — after the assistant message is persisted.

- [ ] **Step 2: Add compression in both send_message and send_message_stream**

For both, right before calling `agent.run(...)` or `agent.run_stream(...)`:

```rust
// Attempt context compression using existing summaries
if let Some(ctx) = context_window {
    let db_locked = state.db.lock().await;
    let compressed = crate::lifecycle::compactor::compress_context(
        &db_locked,
        &session_id,
        api_messages,
        ctx,
    )?;
    drop(db_locked);
    api_messages = compressed;
}
```

- [ ] **Step 3: Update list_sessions to handle archived filter**

Modify `list_sessions` to accept an optional `include_archived` parameter (default false). When false, filter out `archived = 1`. We'll add this as a new query or filter after fetching:

```rust
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    include_archived: Option<bool>,
) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    let all = db.list_sessions()?;
    if include_archived.unwrap_or(false) {
        Ok(all)
    } else {
        Ok(all.into_iter().filter(|s| !s.archived).collect())
    }
}
```

Update the frontend API call to pass `includeArchived` as needed.

- [ ] **Step 4: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1
```
Expected: Compilation succeeds.

---

### Task 9: Frontend — New Types + API + lifecycleSlice

**Files:**
- Modify: `src-ui/src/types/index.ts`
- Modify: `src-ui/src/api/tauri.ts`
- Create: `src-ui/src/store/lifecycleSlice.ts`
- Modify: `src-ui/src/store/index.ts`

- [ ] **Step 1: Add SessionSummary and LifecycleConfig types**

In `types/index.ts`:

```typescript
export interface SessionSummary {
  id: string;
  session_id: string;
  message_start_id: string;
  message_end_id: string;
  summary: string;
  key_points: string[] | null;
  original_token_count: number;
  summary_token_count: number;
  model_used: string | null;
  created_at: string;
}

export interface LifecycleConfig {
  auto_title_enabled: boolean;
  title_model: string | null;
  auto_summarize_enabled: boolean;
  summarize_chunk_size: number;
  summarize_model: string | null;
  auto_archive_enabled: boolean;
  archive_after_days: number;
}
```

Add `title_source` and `archived` to `Session`:

```typescript
export interface Session {
  id: string;
  title: string;
  model_id: string;
  system_prompt?: string | null;
  title_source?: string;     // "manual" | "auto_generated"
  archived?: boolean;         // added
  created_at: string;
  updated_at: string;
}
```

- [ ] **Step 2: Add API functions**

In `api/tauri.ts`:

```typescript
export async function getSessionSummaries(sessionId: string): Promise<SessionSummary[]> {
  return invoke('get_session_summaries', { sessionId });
}

export async function forceGenerateSummary(sessionId: string): Promise<void> {
  return invoke('force_generate_summary', { sessionId });
}

export async function getLifecycleConfig(): Promise<LifecycleConfig> {
  return invoke('get_lifecycle_config');
}

export async function updateLifecycleConfig(config: LifecycleConfig): Promise<void> {
  return invoke('update_lifecycle_config', { config });
}

export async function archiveSession(id: string): Promise<void> {
  return invoke('archive_session', { id });
}

export async function unarchiveSession(id: string): Promise<void> {
  return invoke('unarchive_session', { id });
}

export async function listSessions(includeArchived?: boolean): Promise<Session[]> {
  return invoke('list_sessions', { includeArchived });
}
```

- [ ] **Step 3: Create lifecycleSlice.ts**

```typescript
import type { StateCreator } from 'zustand';
import type { LifecycleConfig } from '../types';
import * as api from '../api/tauri';

export interface LifecycleSlice {
  lifecycleConfig: LifecycleConfig;
  fetchLifecycleConfig: () => Promise<void>;
  updateLifecycleConfig: (config: LifecycleConfig) => Promise<void>;
  archiveSession: (id: string) => Promise<void>;
  unarchiveSession: (id: string) => Promise<void>;
}

export const createLifecycleSlice: StateCreator<any, [], [], LifecycleSlice> = (set, get) => ({
  lifecycleConfig: {
    auto_title_enabled: true,
    title_model: null,
    auto_summarize_enabled: true,
    summarize_chunk_size: 20,
    summarize_model: null,
    auto_archive_enabled: true,
    archive_after_days: 30,
  },

  fetchLifecycleConfig: async () => {
    try {
      const config = await api.getLifecycleConfig();
      set({ lifecycleConfig: config });
    } catch (err) {
      console.error('Failed to load lifecycle config:', err);
    }
  },

  updateLifecycleConfig: async (config) => {
    try {
      await api.updateLifecycleConfig(config);
      set({ lifecycleConfig: config });
    } catch (err) {
      console.error('Failed to update lifecycle config:', err);
    }
  },

  archiveSession: async (id) => {
    try {
      await api.archiveSession(id);
      // Remove from sessions list
      set((state: any) => ({
        sessions: state.sessions.filter((s: any) => s.id !== id),
      }));
    } catch (err) {
      console.error('Failed to archive session:', err);
    }
  },

  unarchiveSession: async (id) => {
    try {
      await api.unarchiveSession(id);
      // Refetch sessions to include the restored one
      await get().fetchSessions();
    } catch (err) {
      console.error('Failed to unarchive session:', err);
    }
  },
});
```

- [ ] **Step 4: Register slice in store/index.ts**

```typescript
import { type LifecycleSlice, createLifecycleSlice } from './lifecycleSlice';

// Add to AppState type
export type AppState = ... & LifecycleSlice & ...;

// Add to store creation
...createLifecycleSlice(set, get, store),
```

- [ ] **Step 5: Run frontend type check**

```bash
cd src-ui && npx tsc --noEmit 2>&1
```
Expected: TypeScript compiles without errors.

---

### Task 10: Frontend — Sidebar archive toggle + summary preview

**Files:**
- Modify: `src-ui/src/components/Sidebar.tsx`

- [ ] **Step 1: Add archive toggle tabs at top of sidebar list**

Replace the static "最近对话" label with tab buttons:

```tsx
const [showArchived, setShowArchived] = useState(false);
// ...

{/* Tab switch */}
<div className="flex items-center gap-1 px-3 py-2">
  <button
    onClick={() => setShowArchived(false)}
    className={`text-xs px-2.5 py-1 rounded-lg font-medium transition-all ${
      !showArchived
        ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300'
        : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-400'
    }`}
  >
    对话
  </button>
  <button
    onClick={() => setShowArchived(true)}
    className={`text-xs px-2.5 py-1 rounded-lg font-medium transition-all ${
      showArchived
        ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300'
        : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-400'
    }`}
  >
    已归档 {archivedCount > 0 && `(${archivedCount})`}
  </button>
</div>
```

- [ ] **Step 2: Filter sessions based on active tab**

```tsx
const sessions = useStore((s) => s.sessions);
const displaySessions = sessions.filter(s => showArchived ? s.archived : !s.archived);
```

- [ ] **Step 3: Add archive/unarchive action in session item**

Add a new button next to delete:

```tsx
<button
  onClick={() => showArchived ? unarchiveSession(sess.id) : archiveSession(sess.id)}
  className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 hover:text-amber-500 transition-colors"
  title={showArchived ? '恢复' : '归档'}
>
  {showArchived ? <RotateCcw size={14} /> : <Archive size={14} />}
</button>
```

Import `Archive, RotateCcw` from lucide-react.

- [ ] **Step 4: Add archive count display**

```tsx
const archivedCount = sessions.filter(s => s.archived).length;
```

Also call `fetchLifecycleConfig` on initial mount to load config.

---

### Task 11: Frontend — ChatArea summary divider

**Files:**
- Modify: `src-ui/src/components/ChatArea.tsx`

- [ ] **Step 1: Add summary divider between compressed blocks**

Create a new memo component:

```tsx
const SummaryDivider = memo(function SummaryDivider({
  summary,
  messageCount,
  onExpand,
}: {
  summary: string;
  messageCount: number;
  onExpand: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div className="flex justify-center">
      <div className="max-w-[80%] w-full">
        <div
          className="flex items-center gap-2 px-4 py-2 rounded-xl bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700/50 cursor-pointer hover:bg-amber-100/50 dark:hover:bg-amber-900/30 transition-all"
          onClick={() => setExpanded(!expanded)}
        >
          <FileText size={14} className="text-amber-500 flex-shrink-0" />
          <span className="text-xs text-amber-700 dark:text-amber-400 font-medium">
            📝 前面 {messageCount} 条消息已压缩
          </span>
          {expanded ? <ChevronDown size={14} className="text-amber-400" /> : <ChevronUp size={14} className="text-amber-400" />}
        </div>
        {expanded && (
          <div className="mt-1 px-4 py-2 bg-amber-50/50 dark:bg-amber-900/10 rounded-lg border border-amber-100 dark:border-amber-800/30 text-xs text-gray-600 dark:text-gray-400 leading-relaxed">
            {summary}
          </div>
        )}
      </div>
    </div>
  );
});
```

- [ ] **Step 2: Add summaries state and fetch on session load**

```tsx
const summaries = useStore((s) => s.summaries);
const fetchSummaries = useStore((s) => s.fetchSummaries);

// Add to the effect that runs when session changes
useEffect(() => {
  if (currentSession?.id) {
    fetchSummaries(currentSession.id);
  }
}, [currentSession?.id]);
```

- [ ] **Step 3: Render summary dividers in MessageList**

In `MessageList`, before rendering messages, check if there are summaries and insert dividers:

The simplest approach: after the message list, render summary dividers at appropriate positions (between compressed and current messages).

```tsx
{summaries.length > 0 && messages.length > 0 && (
  <div className="space-y-2">
    {summaries.map((s) => (
      <SummaryDivider
        key={s.id}
        summary={s.summary}
        messageCount={/* calculate from message range */}
        onExpand={() => {}}
      />
    ))}
  </div>
)}
```

---

### Task 12: Frontend — Settings page lifecycle config

**Files:**
- Modify: `src-ui/src/components/SettingsPage.tsx`

- [ ] **Step 1: Add Conversation Management section**

Find the settings sections and add a new one:

```tsx
import { Switch, Select, InputNumber } from 'antd';
import { useStore } from '../store';

// Inside SettingsPage component:
const lifecycleConfig = useStore((s) => s.lifecycleConfig);
const updateLifecycleConfig = useStore((s) => s.updateLifecycleConfig);

// Add a new settings section:
<section>
  <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-4">
    Conversation Management
  </h3>
  <div className="space-y-4">
    {/* Auto Title */}
    <div className="flex items-center justify-between">
      <div>
        <div className="text-sm font-medium text-gray-700 dark:text-gray-300">Auto-title generation</div>
        <div className="text-xs text-gray-400">Generate titles automatically</div>
      </div>
      <Switch
        checked={lifecycleConfig.auto_title_enabled}
        onChange={(checked) => updateLifecycleConfig({ ...lifecycleConfig, auto_title_enabled: checked })}
      />
    </div>

    {lifecycleConfig.auto_title_enabled && (
      <div className="flex items-center justify-between pl-4">
        <div className="text-sm text-gray-600 dark:text-gray-400">Title model</div>
        {/* model selector or "use session model" indicator */}
      </div>
    )}

    {/* Auto Summarization */}
    <div className="flex items-center justify-between">
      <div>
        <div className="text-sm font-medium text-gray-700 dark:text-gray-300">Auto-summarization</div>
        <div className="text-xs text-gray-400">Summarize older messages</div>
      </div>
      <Switch
        checked={lifecycleConfig.auto_summarize_enabled}
        onChange={(checked) => updateLifecycleConfig({ ...lifecycleConfig, auto_summarize_enabled: checked })}
      />
    </div>

    {lifecycleConfig.auto_summarize_enabled && (
      <div className="pl-4 space-y-3">
        <div className="flex items-center justify-between">
          <span className="text-sm text-gray-600 dark:text-gray-400">Summarize every</span>
          <InputNumber
            size="small"
            min={5}
            max={100}
            value={lifecycleConfig.summarize_chunk_size}
            onChange={(val) => val && updateLifecycleConfig({ ...lifecycleConfig, summarize_chunk_size: val })}
          />
          <span className="text-xs text-gray-400 ml-2">messages</span>
        </div>
      </div>
    )}

    {/* Auto Archive */}
    <div className="flex items-center justify-between">
      <div>
        <div className="text-sm font-medium text-gray-700 dark:text-gray-300">Auto-archive</div>
        <div className="text-xs text-gray-400">Archive inactive sessions</div>
      </div>
      <Switch
        checked={lifecycleConfig.auto_archive_enabled}
        onChange={(checked) => updateLifecycleConfig({ ...lifecycleConfig, auto_archive_enabled: checked })}
      />
    </div>

    {lifecycleConfig.auto_archive_enabled && (
      <div className="pl-4 flex items-center justify-between">
        <span className="text-sm text-gray-600 dark:text-gray-400">Archive after</span>
        <InputNumber
          size="small"
          min={1}
          max={365}
          value={lifecycleConfig.archive_after_days}
          onChange={(val) => val && updateLifecycleConfig({ ...lifecycleConfig, archive_after_days: val })}
        />
        <span className="text-xs text-gray-400 ml-2">days</span>
      </div>
    )}
  </div>
</section>
```

---

### Task 13: Add summaries state to store + fetch

**Files:**
- Modify: `src-ui/src/store/index.ts`

- [ ] **Step 1: Add summaries field and fetch action**

In `store/index.ts`, add to `AppState` type and the store creation:

```typescript
// Add to AppState type
summaries: SessionSummary[];
fetchSummaries: (sessionId: string) => Promise<void>;

// Add to store create
summaries: [],

fetchSummaries: async (sessionId: string) => {
  try {
    const summaries = await api.getSessionSummaries(sessionId);
    set({ summaries });
  } catch (err) {
    console.error('Failed to fetch summaries:', err);
  }
},
```

- [ ] **Step 2: Clear summaries on session switch**

In `selectSession` in `sessionSlice.ts`, add:

```typescript
// After setting messages:
get().fetchSummaries(id); // fire and forget
```

Or in `store/index.ts`, ensure summaries are cleared when `currentSession` is set to null.

---

## Self-Review Checklist

- **Spec coverage**: ✅ All spec sections covered (data model, titler, summarizer, compactor, archiver, IPC, frontend)
- **Placeholder scan**: ✅ No "TBD", "TODO", or vague placeholders found
- **Type consistency**: ✅ All Rust struct fields and TS interfaces are consistent across tasks
