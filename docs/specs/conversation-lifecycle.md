# Conversation Lifecycle Management — Design Spec

> 对话生命周期管理：自动标题、智能摘要、上下文压缩、会话归档

## Overview

为 Agent 桌面客户端的对话系统增加全链路生命周期管理。在现有 Session + Message 架构基础上，新增摘要、自动标题、上下文压缩和归档能力，让对话随时间推移自我管理。

## Data Model

### New Table: `session_summaries`

```sql
CREATE TABLE session_summaries (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    message_start_id TEXT NOT NULL,
    message_end_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    key_points TEXT,              -- JSON array of bullet points
    original_token_count INTEGER, -- token count of the original messages
    summary_token_count INTEGER,  -- token count of the generated summary
    model_used TEXT,              -- which model generated this summary
    created_at TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
```

### Extended `sessions` Table (migration)

```sql
ALTER TABLE sessions ADD COLUMN title_source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
```

- `title_source`: `'manual'` (user-set) or `'auto_generated'` (AI-set). Auto titles never overwrite manual ones.
- `archived`: Soft-delete flag for archival. Archive status is determined by `updated_at + archive_after_days` config.

## Backend Architecture

### New Module: `src-tauri/src/lifecycle/`

```
src-tauri/src/lifecycle/
├── mod.rs            # Public API: init, trigger_title, trigger_compact, etc.
├── titler.rs         # Title generation
├── summarizer.rs     # Summary generation
├── compactor.rs      # Context compression (AgentLoop integration)
└── archiver.rs       # Archival policy
```

Registered in `lib.rs` as `mod lifecycle`.

### titler.rs

**Trigger**: After `send_message_stream` completes, when:
- `session.title_source != 'manual'` (don't overwrite manual titles)
- Session has >= 2 messages (at least 1 user + 1 assistant)
- No auto-title has been generated yet for this session

**Execution**: `tokio::spawn` async, non-blocking.

**Prompt**:
```
Based on the following conversation, generate a concise title in the user's language (max 8 words).
Return ONLY the title, no quotation marks or explanation.

Conversation:
{first_user_message}
{first_assistant_message}
```

**Outcome**: `UPDATE sessions SET title = ?, title_source = 'auto_generated'`

### summarizer.rs

**Trigger**: After assistant message is persisted, check:
- Total messages not yet covered by summaries >= `summarize_chunk_size` (default 20)
- Or: total session tokens >= `context_window * 0.7` (aggressive compression needed)

**Strategy (sliding window from last summarized point)**:
1. Query `session_summaries` for existing summaries, find the latest `message_end_id`
2. Collect new messages since `message_end_id`, up to `SUMMARIZER_CHUNK_SIZE`
3. Call LLM with summarization prompt
4. Insert `session_summary` record linking `message_start_id` → `message_end_id`
5. Optionally generate combined top-level session summary from all chunk summaries

**Chunk Summary Prompt**:
```
Summarize the key information, decisions, and conclusions from this conversation segment.
Be concise but preserve important technical details.

{segment_messages}
```

**Top-level Summary Prompt**:
```
Combine these segment summaries into a coherent overall session summary.
Focus on the main topic, key decisions, and unresolved items.

{chunk_summaries}
```

**Configuration**:
```rust
pub struct LifecycleConfig {
    pub auto_title_enabled: bool,
    pub title_model: Option<String>,       // default: same as session model
    pub auto_summarize_enabled: bool,
    pub summarize_chunk_size: usize,       // default: 20
    pub summarize_model: Option<String>,   // default: same as session model
    pub auto_archive_enabled: bool,
    pub archive_after_days: u32,           // default: 30
}
```

### compactor.rs (AgentLoop Integration)

**Current `optimize_context`**: Drops oldest messages when context exceeds limit.

**Upgraded `compress_context`**:

```
1. Preserve system / persona / memory messages
2. Calculate total token count of non-system messages
3. If over limit (context_window * safety_factor):
   a. Find earliest contiguous block of messages
   b. Check session_summaries for existing summary covering this range
      - Found: replace the block with a system message containing the summary
      - Not found: drop as before (summarizer will process asynchronously later)
   c. Repeat until token count is within limit
4. If under limit, no compression needed
```

> **Note**: Compressor is READ-ONLY for summaries. It never blocks on summary generation. The summarizer runs asynchronously after message completion, ensuring summaries are available for future requests.

**Integration point**: The session's `model_id` is used to locate the correct `context_window` from config. The compressor is called at the start of `AgentLoop::run()` and `AgentLoop::run_stream()`.

**Summary injection format**:
```
[Previous conversation compressed]
Summary: {summary_text}
Key points: {key_points}
```

### archiver.rs

**Policy**: On app startup and periodically (hourly tick):
1. Query sessions where `updated_at < now - archive_after_days AND archived = 0`
2. For each:
   - Generate final summary if none exists for the last N messages
   - `UPDATE sessions SET archived = 1`
3. Skip sessions with `title_source = 'manual'` and empty content (user may want to keep)

## IPC Commands

| Command | Parameters | Returns | Description |
|---------|-----------|---------|-------------|
| `get_session_summaries` | `sessionId: String` | `Vec<SessionSummary>` | List all summaries for a session |
| `force_generate_summary` | `sessionId: String` | `SessionSummary` | Manually trigger summary generation |
| `archive_session` | `id: String` | `()` | Mark session as archived |
| `unarchive_session` | `id: String` | `()` | Restore archived session |
| `list_archived_sessions` | — | `Vec<Session>` | List archived sessions only |
| `update_lifecycle_config` | `config: LifecycleConfig` | `()` | Save lifecycle preferences to settings |
| `get_lifecycle_config` | — | `LifecycleConfig` | Read lifecycle preferences |

## Frontend Changes

### Sidebar (Sidebar.tsx)

- **Title display**: Use auto-generated titles by default. Manual titles (user-renamed) prioritized.
- **Summary preview**: On hover/selection, show a small summary card (3-line max).
- **Archived toggle**: Tab switch "Active / Archived" at top of session list.
- **Metadata**: Show message count and approximate token count as small text under title.

### ChatArea (ChatArea.tsx)

- **Summary divider**: At compression points, render a collapsible "📝 Previous N messages compressed" bar. On expand, show the summary text + option to view original messages.
- **Session info bar**: Optional display of message count / token count.

### Settings (SettingsPage.tsx)

New "Conversation Management" section:

```
─────────────────────────────────
Conversation Management
─────────────────────────────────

Auto-title generation     ● On  ○ Off
Title model               [gpt-4o-mini ▼]

Auto-summarization        ● On  ○ Off
Summarize every           [20 messages ▼]
Summary model             [Same as session ▼]

Auto-archive              ● On  ○ Off
Archive after             [30 days ▼]
─────────────────────────────────
```

### Frontend State (Zustand)

New slice: `lifecycleSlice.ts`

```typescript
interface LifecycleSlice {
  lifecycleConfig: LifecycleConfig;
  archivedSessions: Session[];
  fetchLifecycleConfig: () => Promise<void>;
  updateLifecycleConfig: (config: LifecycleConfig) => Promise<void>;
  fetchArchivedSessions: () => Promise<void>;
  archiveSession: (id: string) => Promise<void>;
  unarchiveSession: (id: string) => Promise<void>;
  forceGenerateSummary: (sessionId: string) => Promise<void>;
}
```

## LifecycleConfig Settings Storage

Stored as JSON string in the `settings` table under key `lifecycle_config`.

```json
{
  "auto_title_enabled": true,
  "title_model": null,
  "auto_summarize_enabled": true,
  "summarize_chunk_size": 20,
  "summarize_model": null,
  "auto_archive_enabled": true,
  "archive_after_days": 30
}
```

## Implementation Plan

1. **Data layer**: DB migration (session_summaries table + session column extensions), add Rust models
2. **Backend module**: Create `lifecycle/` module with titler, summarizer, compactor, archiver
3. **IPC commands**: Register all new commands in `lib.rs`
4. **AgentLoop integration**: Hook compressor into `optimize_context` / `run_stream`
5. **Frontend**: lifecycleSlice, sidebar enhancement, settings UI, summary dividers
6. **Async wiring**: Hook title/summary triggers into `send_message_stream` completion
7. **Testing**: Unit tests for compressor logic, integration test for lifecycle flow

## Open Questions

- Should summary generation use the session's current model or always a cheaper one?
- Should we support user-editing auto-generated summaries?
- Auto-archive: exclude sessions manually starred/pinned?
