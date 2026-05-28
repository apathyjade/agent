use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::models::{BoundProjectModel, Project, Session, SessionSummary, MemoryRecord, Message, PersonaRecord, RuntimeVersionCache, Setting, SkillRecord, SystemPrompt};
use crate::orchestrator::plan_types::{ExecutionPlanRecord, PlanStepRecord};
use crate::pipeline::models::WorkflowRunRecord;
use crate::error::Result;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = Self::db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        Self::init_tables(&conn)?;
        Self::migrate_tables(&conn)?;

        Ok(Self { conn })
    }

    fn db_path() -> PathBuf {
        let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("agent");
        path.push("agent.db");
        path
    }

    /// Create a temporary in-memory database for testing.
    /// Only available in test builds.
    #[cfg(test)]
    pub fn new_test() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::init_tables(&conn)?;
        Self::migrate_tables(&conn)?;
        Ok(Self { conn })
    }

    fn init_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = OFF;

            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                model_id TEXT NOT NULL,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                project_id TEXT REFERENCES projects(id)
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
                tool_call_id TEXT,
                tokens INTEGER,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS system_prompts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                version TEXT NOT NULL DEFAULT '0.1.0',
                author TEXT,
                icon TEXT,
                tags TEXT,
                source_type TEXT NOT NULL,
                source_path TEXT,
                entry_type TEXT NOT NULL,
                entry_value TEXT NOT NULL,
                config_schema TEXT,
                config TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                installed_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workflow_runs (
                id TEXT PRIMARY KEY,
                workflow_name TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'running',
                step_results TEXT,
                error TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT
            );

            CREATE TABLE IF NOT EXISTS runtime_version_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                runtime_type TEXT NOT NULL,
                version TEXT NOT NULL,
                display_name TEXT NOT NULL,
                url TEXT NOT NULL,
                lts TEXT,
                is_stable INTEGER NOT NULL DEFAULT 1,
                release_date TEXT,
                file_size INTEGER,
                fetched_at TEXT NOT NULL,
                UNIQUE(runtime_type, version)
            );

            CREATE TABLE IF NOT EXISTS bound_projects (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                auto_sync INTEGER NOT NULL DEFAULT 1,
                last_scan TEXT,
                requirements TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                memory_type TEXT NOT NULL DEFAULT 'fact',
                scope TEXT NOT NULL DEFAULT 'global',
                source TEXT NOT NULL DEFAULT 'manual',
                relevance REAL NOT NULL DEFAULT 1.0,
                tags TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_accessed_at TEXT NOT NULL,
                access_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
            CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
            CREATE INDEX IF NOT EXISTS idx_memories_relevance ON memories(relevance DESC);

            CREATE TABLE IF NOT EXISTS personas (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL DEFAULT '',
                emoji TEXT NOT NULL DEFAULT '🧑‍💻',
                description TEXT NOT NULL DEFAULT '',
                system_prompt TEXT NOT NULL,
                temperature REAL NOT NULL DEFAULT 0.3,
                response_style TEXT NOT NULL DEFAULT 'concise',
                model_provider TEXT NOT NULL DEFAULT '',
                model_name TEXT NOT NULL DEFAULT '',
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS persona_memories (
                persona_id TEXT NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
                memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                PRIMARY KEY (persona_id, memory_id)
            );

            CREATE TABLE IF NOT EXISTS persona_projects (
                persona_id TEXT NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
                project_path TEXT NOT NULL,
                auto_select INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (persona_id, project_path)
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            ",
        )?;

        // FTS5 virtual table for full-text search on memories (content + tags)
        // Uses external content model — indexes only, data lives in memories table.
        // unicode61 tokenizer handles both English and CJK characters.
        conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                content, tags,
                content='memories',
                content_rowid='rowid',
                tokenize='unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, content, tags)
                VALUES (new.rowid, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, tags)
                VALUES ('delete', old.rowid, old.content, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, tags)
                VALUES ('delete', old.rowid, old.content, old.tags);
                INSERT INTO memories_fts(rowid, content, tags)
                VALUES (new.rowid, new.content, new.tags);
            END;
            ",
        )?;

        // Session summaries table for conversation lifecycle management
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

            CREATE TABLE IF NOT EXISTS execution_plans (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                source TEXT NOT NULL,
                goal TEXT,
                plan_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                finished_at TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_plan_steps (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL,
                step_index INTEGER NOT NULL,
                label TEXT NOT NULL,
                step_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                result_json TEXT,
                error TEXT,
                started_at TEXT,
                duration_ms INTEGER,
                FOREIGN KEY (plan_id) REFERENCES execution_plans(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_exec_plans_session ON execution_plans(session_id);
            CREATE INDEX IF NOT EXISTS idx_exec_steps_plan ON execution_plan_steps(plan_id);
            ",
        )?;

        Ok(())
    }

    fn migrate_tables(conn: &Connection) -> Result<()> {
        // Migration v1: rename provider column (old schema)
        let has_provider = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='provider'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_provider > 0 {
            conn.execute_batch(
                "
                ALTER TABLE sessions RENAME TO sessions_old;

                CREATE TABLE sessions (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    system_prompt TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                INSERT INTO sessions (id, title, model_id, system_prompt, created_at, updated_at)
                SELECT id, title, COALESCE(model, 'default-openai'), system_prompt, created_at, updated_at
                FROM sessions_old;

                DROP TABLE sessions_old;
                ",
            )?;
        }

        // Migration v2: add agent_sources column to skills table
        let has_agent_sources = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('skills') WHERE name='agent_sources'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_agent_sources == 0 {
            conn.execute_batch(
                "ALTER TABLE skills ADD COLUMN agent_sources TEXT;",
            )?;
        }

        // Migration v3: add trigger_type and step_progress to workflow_runs
        let has_trigger_type = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('workflow_runs') WHERE name='trigger_type'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_trigger_type == 0 {
            conn.execute_batch(
                "ALTER TABLE workflow_runs ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'manual';
                 ALTER TABLE workflow_runs ADD COLUMN step_progress TEXT;",
            )?;
        }

        // Migration v4: rename sessersations → sessions, sessersation_id → session_id
        let has_old_table = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sessersations'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);
        if has_old_table > 0 {
            conn.execute_batch(
                "
                PRAGMA foreign_keys = OFF;

                -- Recreate messages table with session_id
                ALTER TABLE messages RENAME TO messages_old;
                CREATE TABLE messages (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    tool_calls TEXT,
                    tool_call_id TEXT,
                    tokens INTEGER,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY (session_id) REFERENCES sessions(id)
                );
                INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, tokens, created_at)
                SELECT id, sessersation_id, role, content, tool_calls, tool_call_id, tokens, created_at
                FROM messages_old;
                DROP TABLE messages_old;

                -- Rename sessersations table
                ALTER TABLE sessersations RENAME TO sessions;

                PRAGMA foreign_keys = ON;
                ",
            )?;
        }

        // Migration v6: add title_source and archived columns to sessions table
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

        // Migration v7: add persona_id column to sessions table
        let has_persona_id = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='persona_id'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_persona_id == 0 {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN persona_id TEXT REFERENCES personas(id);",
            )?;
        }

        // Migration v8: add config column to sessions table
        let has_config = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='config'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_config == 0 {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN config TEXT;",
            )?;
        }

        // Migration v9: add mode, execution_status, active_plan_id to sessions
        let has_mode = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='mode'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_mode == 0 {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN mode TEXT NOT NULL DEFAULT 'chat';
                 ALTER TABLE sessions ADD COLUMN execution_status TEXT NOT NULL DEFAULT 'idle';
                 ALTER TABLE sessions ADD COLUMN active_plan_id TEXT;",
            )?;
        }

        // Migration v10: add project_id column to sessions table
        let has_project_id = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='project_id'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_project_id == 0 {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id);",
            )?;
        }

        // Migration v5: rebuild FTS5 index if existing memories haven't been indexed yet.
        // The FTS table was just created in init_tables (or already existed from a prior run).
        // Check if the FTS index is empty while the memories table has rows.
        let mem_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap_or(0);
        if mem_count > 0 {
            let fts_count: i32 = conn
                .query_row("SELECT COUNT(*) FROM memories_fts", [], |row| row.get(0))
                .unwrap_or(0);
            if fts_count == 0 {
                conn.execute_batch("INSERT INTO memories_fts(memories_fts) VALUES('rebuild');")?;
            }
        }

        Ok(())
    }

    pub fn create_session(&self, sess: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![sess.id, sess.title, sess.model_id, sess.system_prompt, sess.persona_id, sess.config, sess.title_source, sess.archived as i32, sess.created_at, sess.updated_at, sess.mode, sess.execution_status, sess.active_plan_id, sess.project_id],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id FROM sessions ORDER BY updated_at DESC",
        )?;
        let sessions = stmt.query_map([], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                model_id: row.get(2)?,
                system_prompt: row.get(3)?,
                persona_id: row.get(4)?,
                config: row.get(5)?,
                title_source: row.get(6)?,
                archived: row.get::<_, i32>(7)? != 0,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                mode: row.get(10)?,
                execution_status: row.get(11)?,
                active_plan_id: row.get(12)?,
                project_id: row.get(13)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id FROM sessions WHERE id = ?1",
        )?;
        let sess = stmt.query_row(params![id], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                model_id: row.get(2)?,
                system_prompt: row.get(3)?,
                persona_id: row.get(4)?,
                config: row.get(5)?,
                title_source: row.get(6)?,
                archived: row.get::<_, i32>(7)? != 0,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                mode: row.get(10)?,
                execution_status: row.get(11)?,
                active_plan_id: row.get(12)?,
                project_id: row.get(13)?,
            })
        }).optional()?;
        Ok(sess)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM session_summaries WHERE session_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        self.conn.execute("DELETE FROM settings WHERE key = ?1", params![format!("request_ctx:{}", id)])?;
        Ok(())
    }

    pub fn update_session_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET title = ?1, updated_at = datetime('now') WHERE id = ?2", params![title, id])?;
        Ok(())
    }

    pub fn update_session_model(&self, id: &str, model_id: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET model_id = ?1, updated_at = datetime('now') WHERE id = ?2", params![model_id, id])?;
        Ok(())
    }

    pub fn update_session_system_prompt(&self, id: &str, system_prompt: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET system_prompt = ?1, updated_at = datetime('now') WHERE id = ?2", params![system_prompt, id])?;
        Ok(())
    }

    pub fn update_session_config(&self, id: &str, config: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET config = ?1, updated_at = datetime('now') WHERE id = ?2", params![config, id])?;
        Ok(())
    }

    pub fn clear_messages(&self, session_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session_id])?;
        self.conn.execute("UPDATE sessions SET updated_at = datetime('now') WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    // ── Session Summaries CRUD ──

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

    pub fn update_session_title_with_source(&self, id: &str, title: &str, source: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?1, title_source = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![title, source, id],
        )?;
        Ok(())
    }

    pub fn insert_message(&self, msg: &Message) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, tokens, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![msg.id, msg.session_id, msg.role, msg.content, msg.tool_calls, msg.tool_call_id, msg.tokens, msg.created_at],
        )?;
        self.conn.execute("UPDATE sessions SET updated_at = datetime('now') WHERE id = ?1", params![msg.session_id])?;
        Ok(())
    }

    pub fn get_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_calls, tool_call_id, tokens, created_at FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let messages = stmt.query_map(params![session_id], |row| {
            Ok(Message { id: row.get(0)?, session_id: row.get(1)?, role: row.get(2)?, content: row.get(3)?, tool_calls: row.get(4)?, tool_call_id: row.get(5)?, tokens: row.get(6)?, created_at: row.get(7)? })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, value])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let value = stmt.query_row(params![key], |row| row.get(0)).optional()?;
        Ok(value)
    }

    pub fn get_all_settings(&self) -> Result<Vec<Setting>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let settings = stmt.query_map([], |row| Ok(Setting { key: row.get(0)?, value: row.get(1)? }))?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(settings)
    }

    pub fn create_system_prompt(&self, prompt: &SystemPrompt) -> Result<()> {
        if prompt.is_default {
            self.conn.execute("UPDATE system_prompts SET is_default = 0", [])?;
        }
        self.conn.execute(
            "INSERT INTO system_prompts (id, name, content, is_default, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![prompt.id, prompt.name, prompt.content, prompt.is_default as i32, prompt.created_at],
        )?;
        Ok(())
    }

    pub fn list_system_prompts(&self) -> Result<Vec<SystemPrompt>> {
        let mut stmt = self.conn.prepare("SELECT id, name, content, is_default, created_at FROM system_prompts ORDER BY created_at DESC")?;
        let prompts = stmt.query_map([], |row| {
            Ok(SystemPrompt { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, is_default: row.get::<_, i32>(3)? != 0, created_at: row.get(4)? })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(prompts)
    }

    pub fn get_system_prompt(&self, id: &str) -> Result<Option<SystemPrompt>> {
        let mut stmt = self.conn.prepare("SELECT id, name, content, is_default, created_at FROM system_prompts WHERE id = ?1")?;
        let prompt = stmt.query_row(params![id], |row| {
            Ok(SystemPrompt { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, is_default: row.get::<_, i32>(3)? != 0, created_at: row.get(4)? })
        }).optional()?;
        Ok(prompt)
    }

    pub fn delete_system_prompt(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM system_prompts WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_default_system_prompt(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE system_prompts SET is_default = 0", [])?;
        self.conn.execute("UPDATE system_prompts SET is_default = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_default_system_prompt(&self) -> Result<Option<SystemPrompt>> {
        let mut stmt = self.conn.prepare("SELECT id, name, content, is_default, created_at FROM system_prompts WHERE is_default = 1 LIMIT 1")?;
        let prompt = stmt.query_row([], |row| {
            Ok(SystemPrompt { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, is_default: row.get::<_, i32>(3)? != 0, created_at: row.get(4)? })
        }).optional()?;
        Ok(prompt)
    }

    pub fn insert_skill(&self, skill: &SkillRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO skills (id, name, description, version, author, icon, tags, source_type, source_path, entry_type, entry_value, config_schema, config, enabled, agent_sources, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![skill.id, skill.name, skill.description, skill.version, skill.author, skill.icon, skill.tags, skill.source_type, skill.source_path, skill.entry_type, skill.entry_value, skill.config_schema, skill.config, skill.enabled as i32, skill.agent_sources, skill.installed_at, skill.updated_at],
        )?;
        Ok(())
    }

    pub fn get_skill(&self, id: &str) -> Result<Option<SkillRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, version, author, icon, tags, source_type, source_path, entry_type, entry_value, config_schema, config, enabled, agent_sources, installed_at, updated_at FROM skills WHERE id = ?1",
        )?;
        let skill = stmt.query_row(params![id], |row| {
            Ok(SkillRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                version: row.get(3)?,
                author: row.get(4)?,
                icon: row.get(5)?,
                tags: row.get(6)?,
                source_type: row.get(7)?,
                source_path: row.get(8)?,
                entry_type: row.get(9)?,
                entry_value: row.get(10)?,
                config_schema: row.get(11)?,
                config: row.get(12)?,
                enabled: row.get::<_, i32>(13)? != 0,
                agent_sources: row.get(14)?,
                installed_at: row.get(15)?,
                updated_at: row.get(16)?,
            })
        }).optional()?;
        Ok(skill)
    }

    pub fn list_skills(&self) -> Result<Vec<SkillRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, version, author, icon, tags, source_type, source_path, entry_type, entry_value, config_schema, config, enabled, agent_sources, installed_at, updated_at FROM skills ORDER BY name ASC",
        )?;
        let skills = stmt.query_map([], |row| {
            Ok(SkillRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                version: row.get(3)?,
                author: row.get(4)?,
                icon: row.get(5)?,
                tags: row.get(6)?,
                source_type: row.get(7)?,
                source_path: row.get(8)?,
                entry_type: row.get(9)?,
                entry_value: row.get(10)?,
                config_schema: row.get(11)?,
                config: row.get(12)?,
                enabled: row.get::<_, i32>(13)? != 0,
                agent_sources: row.get(14)?,
                installed_at: row.get(15)?,
                updated_at: row.get(16)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(skills)
    }

    pub fn update_skill_agent_sources(&self, id: &str, agent_sources: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET agent_sources = ?2, updated_at = datetime('now') WHERE id = ?1",
            params![id, agent_sources],
        )?;
        Ok(())
    }

    pub fn update_skill_source_type(&self, id: &str, source_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET source_type = ?2, updated_at = datetime('now') WHERE id = ?1",
            params![id, source_type],
        )?;
        Ok(())
    }

    pub fn update_skill_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        self.conn.execute("UPDATE skills SET enabled = ?2, updated_at = datetime('now') WHERE id = ?1", params![id, enabled as i32])?;
        Ok(())
    }

    pub fn update_skill_config(&self, id: &str, config: &str) -> Result<()> {
        self.conn.execute("UPDATE skills SET config = ?2, updated_at = datetime('now') WHERE id = ?1", params![id, config])?;
        Ok(())
    }

    pub fn delete_skill(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM skills WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn delete_skills_by_source_type(&self, source_type: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM skills WHERE source_type = ?1", params![source_type])?;
        Ok(())
    }

    // ── Workflow Runs ──

    pub fn insert_workflow_run(&self, run: &WorkflowRunRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workflow_runs (id, workflow_name, status, step_results, step_progress, error, trigger_type, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![run.id, run.workflow_name, run.status, run.step_results, run.step_progress, run.error, run.trigger_type, run.started_at, run.finished_at],
        )?;
        Ok(())
    }

    pub fn update_workflow_run_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
        step_results: &HashMap<String, serde_json::Value>,
        step_progress: Option<&str>,
    ) -> Result<()> {
        let step_results_str = serde_json::to_string(step_results).unwrap_or_default();
        let finished_at = if status == "running" {
            None
        } else {
            Some(chrono::Utc::now().to_rfc3339())
        };
        self.conn.execute(
            "UPDATE workflow_runs SET status = ?2, step_results = ?3, step_progress = ?4, error = ?5, finished_at = ?6 WHERE id = ?1",
            params![id, status, step_results_str, step_progress, error, finished_at],
        )?;
        Ok(())
    }

    pub fn list_workflow_runs(&self, limit: i64) -> Result<Vec<WorkflowRunRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workflow_name, status, step_results, step_progress, error, trigger_type, started_at, finished_at
             FROM workflow_runs ORDER BY started_at DESC LIMIT ?1",
        )?;
        let runs = stmt.query_map(params![limit], |row| {
            Ok(WorkflowRunRecord {
                id: row.get(0)?,
                workflow_name: row.get(1)?,
                status: row.get(2)?,
                step_results: row.get(3)?,
                step_progress: row.get(4)?,
                error: row.get(5)?,
                trigger_type: row.get(6)?,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(runs)
    }

    pub fn get_workflow_run(&self, id: &str) -> Result<Option<WorkflowRunRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workflow_name, status, step_results, step_progress, error, trigger_type, started_at, finished_at
             FROM workflow_runs WHERE id = ?1",
        )?;
        let run = stmt.query_row(params![id], |row| {
            Ok(WorkflowRunRecord {
                id: row.get(0)?,
                workflow_name: row.get(1)?,
                status: row.get(2)?,
                step_results: row.get(3)?,
                step_progress: row.get(4)?,
                error: row.get(5)?,
                trigger_type: row.get(6)?,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
            })
        }).optional()?;
        Ok(run)
    }

    // ── Runtime Version Cache ──

    pub fn get_cached_versions(&self, rt: &str) -> Result<Vec<RuntimeVersionCache>> {
        let mut stmt = self.conn.prepare(
            "SELECT runtime_type, version, display_name, url, lts, is_stable, release_date, file_size, fetched_at
             FROM runtime_version_cache
             WHERE runtime_type = ?1
             ORDER BY version DESC",
        )?;
        let entries = stmt
            .query_map(params![rt], |row| {
                Ok(RuntimeVersionCache {
                    runtime_type: row.get(0)?,
                    version: row.get(1)?,
                    display_name: row.get(2)?,
                    url: row.get(3)?,
                    lts: row.get(4)?,
                    is_stable: row.get::<_, i32>(5)? != 0,
                    release_date: row.get(6)?,
                    file_size: row.get(7)?,
                    fetched_at: row.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    pub fn upsert_version_cache(&self, entry: &RuntimeVersionCache) -> Result<()> {
        self.conn.execute(
            "INSERT INTO runtime_version_cache (runtime_type, version, display_name, url, lts, is_stable, release_date, file_size, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(runtime_type, version) DO UPDATE SET
                display_name = excluded.display_name,
                url = excluded.url,
                lts = excluded.lts,
                is_stable = excluded.is_stable,
                release_date = excluded.release_date,
                file_size = excluded.file_size,
                fetched_at = excluded.fetched_at",
            params![
                entry.runtime_type,
                entry.version,
                entry.display_name,
                entry.url,
                entry.lts,
                entry.is_stable as i32,
                entry.release_date,
                entry.file_size,
                entry.fetched_at,
            ],
        )?;
        Ok(())
    }

    pub fn clear_version_cache(&self, rt: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM runtime_version_cache WHERE runtime_type = ?1", params![rt])?;
        Ok(())
    }

    // ── Bound Projects ──

    pub fn add_bound_project(&self, path: &str, name: &str) -> Result<BoundProjectModel> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO bound_projects (id, path, name, auto_sync, last_scan, requirements, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, NULL, NULL, ?4, ?5)",
            params![id, path, name, now, now],
        )?;
        Ok(BoundProjectModel {
            id,
            path: path.to_string(),
            name: name.to_string(),
            auto_sync: true,
            last_scan: None,
            requirements: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_bound_projects(&self) -> Result<Vec<BoundProjectModel>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name, auto_sync, last_scan, requirements, created_at, updated_at
             FROM bound_projects ORDER BY updated_at DESC",
        )?;
        let projects = stmt
            .query_map([], |row| {
                Ok(BoundProjectModel {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    auto_sync: row.get::<_, i32>(3)? != 0,
                    last_scan: row.get(4)?,
                    requirements: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(projects)
    }

    pub fn remove_bound_project(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM bound_projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn get_bound_project(&self, id: &str) -> Result<Option<BoundProjectModel>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name, auto_sync, last_scan, requirements, created_at, updated_at
             FROM bound_projects WHERE id = ?1",
        )?;
        let project = stmt
            .query_row(params![id], |row| {
                Ok(BoundProjectModel {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    auto_sync: row.get::<_, i32>(3)? != 0,
                    last_scan: row.get(4)?,
                    requirements: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .optional()?;
        Ok(project)
    }

    pub fn update_bound_project(&self, project: &BoundProjectModel) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE bound_projects SET path = ?2, name = ?3, auto_sync = ?4, last_scan = ?5, requirements = ?6, updated_at = ?7 WHERE id = ?1",
            params![
                project.id,
                project.path,
                project.name,
                project.auto_sync as i32,
                project.last_scan,
                project.requirements,
                now,
            ],
        )?;
        Ok(())
    }

    // ── Projects CRUD ──

    pub fn create_project(&self, project: &Project) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project.id, project.name, project.path, project.created_at, project.updated_at],
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, created_at, updated_at
             FROM projects ORDER BY updated_at DESC",
        )?;
        let projects = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(projects)
    }

    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, created_at, updated_at
             FROM projects WHERE id = ?1",
        )?;
        let project = stmt.query_row(params![id], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        }).optional()?;
        Ok(project)
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_sessions_project_null(&self, project_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET project_id = NULL WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }

    pub fn get_sessions_by_project(&self, project_id: &str) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id
             FROM sessions WHERE project_id = ?1 ORDER BY updated_at DESC",
        )?;
        let sessions = stmt.query_map(params![project_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                model_id: row.get(2)?,
                system_prompt: row.get(3)?,
                persona_id: row.get(4)?,
                config: row.get(5)?,
                title_source: row.get(6)?,
                archived: row.get::<_, i32>(7)? != 0,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                mode: row.get(10)?,
                execution_status: row.get(11)?,
                active_plan_id: row.get(12)?,
                project_id: row.get(13)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn skill_exists(&self, id: &str) -> Result<bool> {
        let count: i32 = self.conn.query_row("SELECT COUNT(*) FROM skills WHERE id = ?1", params![id], |row| row.get(0))?;
        Ok(count > 0)
    }

    // ── Memories CRUD ──

    pub fn insert_memory(&self, mem: &MemoryRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO memories (id, content, memory_type, scope, source, relevance, tags, created_at, updated_at, last_accessed_at, access_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                mem.id, mem.content, mem.memory_type, mem.scope, mem.source,
                mem.relevance, mem.tags, mem.created_at, mem.updated_at,
                mem.last_accessed_at, mem.access_count,
            ],
        )?;
        Ok(())
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<MemoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, memory_type, scope, source, relevance, tags, created_at, updated_at, last_accessed_at, access_count
             FROM memories WHERE id = ?1",
        )?;
        let mem = stmt.query_row(params![id], |row| {
            Ok(MemoryRecord {
                id: row.get(0)?,
                content: row.get(1)?,
                memory_type: row.get(2)?,
                scope: row.get(3)?,
                source: row.get(4)?,
                relevance: row.get(5)?,
                tags: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                last_accessed_at: row.get(9)?,
                access_count: row.get(10)?,
            })
        }).optional()?;
        Ok(mem)
    }

    pub fn list_memories(&self) -> Result<Vec<MemoryRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, memory_type, scope, source, relevance, tags, created_at, updated_at, last_accessed_at, access_count
             FROM memories ORDER BY relevance DESC, last_accessed_at DESC",
        )?;
        let memories = stmt.query_map([], |row| {
            Ok(MemoryRecord {
                id: row.get(0)?,
                content: row.get(1)?,
                memory_type: row.get(2)?,
                scope: row.get(3)?,
                source: row.get(4)?,
                relevance: row.get(5)?,
                tags: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                last_accessed_at: row.get(9)?,
                access_count: row.get(10)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(memories)
    }

    /// Sanitize user input for safe use in FTS5 MATCH queries.
    /// Removes FTS5 operator characters and reserved keywords to prevent syntax errors.
    fn sanitize_fts_query(input: &str) -> String {
        let cleaned: String = input
            .chars()
            .filter(|c| !matches!(c, '"' | '(' | ')' | '*' | '^' | '~' | ':' | '+' | '-'))
            .collect();

        let tokens: Vec<&str> = cleaned
            .split_whitespace()
            .filter(|t| {
                if t.is_empty() {
                    return false;
                }
                // Filter out FTS5 reserved keywords (case-insensitive)
                !matches!(
                    t.to_uppercase().as_str(),
                    "AND" | "OR" | "NOT" | "NEAR"
                )
            })
            .collect();

        if tokens.is_empty() {
            return String::new();
        }

        tokens.join(" AND ")
    }

    /// Search memories by FTS5 full-text match on content and tags.
    pub fn search_memories(&self, query: &str, memory_type: Option<&str>, scope: Option<&str>) -> Result<Vec<MemoryRecord>> {
        let fts_query = Self::sanitize_fts_query(query);

        if fts_query.is_empty() {
            // Fallback: return all memories (no FTS filtering) with optional type/scope filters
            let mut sql = String::from(
                "SELECT id, content, memory_type, scope, source, relevance, tags, created_at, updated_at, last_accessed_at, access_count
                 FROM memories WHERE 1=1"
            );
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(t) = memory_type {
                sql.push_str(" AND memory_type = ?1");
                param_values.push(Box::new(t.to_string()));
            }
            if let Some(s) = scope {
                let idx = if param_values.is_empty() { 1 } else { 2 };
                sql.push_str(&format!(" AND scope LIKE ?{}", idx));
                param_values.push(Box::new(format!("{}%", s)));
            }

            sql.push_str(" ORDER BY relevance DESC, last_accessed_at DESC");

            let mut stmt = self.conn.prepare(&sql)?;
            let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
            let memories = stmt.query_map(params_refs.as_slice(), Self::map_memory_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            return Ok(memories);
        }

        // FTS5 full-text search: content + tags columns are indexed
        let mut sql = String::from(
            "SELECT m.id, m.content, m.memory_type, m.scope, m.source, m.relevance, m.tags, m.created_at, m.updated_at, m.last_accessed_at, m.access_count
             FROM memories m
             JOIN memories_fts fts ON m.rowid = fts.rowid
             WHERE memories_fts MATCH ?1"
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(fts_query)];

        if let Some(t) = memory_type {
            sql.push_str(" AND m.memory_type = ?2");
            param_values.push(Box::new(t.to_string()));
        }
        if let Some(s) = scope {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND m.scope LIKE ?{}", idx));
            param_values.push(Box::new(format!("{}%", s)));
        }

        sql.push_str(" ORDER BY m.relevance DESC, m.last_accessed_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        let memories = stmt.query_map(params_refs.as_slice(), Self::map_memory_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(memories)
    }

    pub fn update_memory(&self, mem: &MemoryRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE memories SET content = ?2, memory_type = ?3, scope = ?4, source = ?5, relevance = ?6, tags = ?7, updated_at = ?8
             WHERE id = ?1",
            params![
                mem.id, mem.content, mem.memory_type, mem.scope, mem.source,
                mem.relevance, mem.tags, mem.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Record that a memory was accessed (bumps last_accessed_at and access_count).
    pub fn touch_memory(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE memories SET last_accessed_at = datetime('now'), access_count = access_count + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Retrieve memories relevant to a context string using FTS5 full-text search.
    /// The entire context is sanitized and passed as a single FTS5 MATCH query.
    pub fn retrieve_relevant(&self, context: &str, limit: i64) -> Result<Vec<MemoryRecord>> {
        if context.trim().is_empty() {
            return Ok(vec![]);
        }

        let fts_query = Self::sanitize_fts_query(context);
        if fts_query.is_empty() {
            return Ok(vec![]);
        }

        let sql = format!(
            "SELECT m.id, m.content, m.memory_type, m.scope, m.source, m.relevance, m.tags, m.created_at, m.updated_at, m.last_accessed_at, m.access_count
             FROM memories m
             JOIN memories_fts fts ON m.rowid = fts.rowid
             WHERE memories_fts MATCH ?1
               AND m.scope = 'global'
             ORDER BY m.relevance DESC, m.access_count DESC, m.last_accessed_at DESC
             LIMIT ?2"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let memories = stmt.query_map(params![fts_query, limit], Self::map_memory_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(memories)
    }

    /// Helper to map a SQL row to a MemoryRecord (shared by search/retrieve paths).
    fn map_memory_row(row: &rusqlite::Row) -> rusqlite::Result<MemoryRecord> {
        Ok(MemoryRecord {
            id: row.get(0)?,
            content: row.get(1)?,
            memory_type: row.get(2)?,
            scope: row.get(3)?,
            source: row.get(4)?,
            relevance: row.get(5)?,
            tags: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            last_accessed_at: row.get(9)?,
            access_count: row.get(10)?,
        })
    }

    fn map_persona_row(row: &rusqlite::Row) -> rusqlite::Result<PersonaRecord> {
        Ok(PersonaRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            title: row.get(2)?,
            emoji: row.get(3)?,
            description: row.get(4)?,
            system_prompt: row.get(5)?,
            temperature: row.get(6)?,
            response_style: row.get(7)?,
            model_provider: row.get(8)?,
            model_name: row.get(9)?,
            is_default: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    }

    // ── Persona CRUD ──

    pub fn insert_persona(&self, p: &PersonaRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO personas (id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![p.id, p.name, p.title, p.emoji, p.description, p.system_prompt, p.temperature, p.response_style, p.model_provider, p.model_name, p.is_default, p.created_at, p.updated_at],
        )?;
        Ok(())
    }

    pub fn list_personas(&self) -> Result<Vec<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas ORDER BY name ASC"
        )?;
        let personas = stmt.query_map([], Self::map_persona_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(personas)
    }

    pub fn get_persona(&self, id: &str) -> Result<Option<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas WHERE id = ?1"
        )?;
        let p = stmt.query_row(params![id], Self::map_persona_row).optional()?;
        Ok(p)
    }

    pub fn get_persona_by_name(&self, name: &str) -> Result<Option<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas WHERE name = ?1"
        )?;
        let p = stmt.query_row(params![name], Self::map_persona_row).optional()?;
        Ok(p)
    }

    pub fn get_default_persona(&self) -> Result<Option<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas WHERE is_default = 1 LIMIT 1"
        )?;
        let p = stmt.query_row([], Self::map_persona_row).optional()?;
        Ok(p)
    }

    pub fn update_persona(&self, p: &PersonaRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE personas SET name=?2, title=?3, emoji=?4, description=?5, system_prompt=?6, temperature=?7, response_style=?8, model_provider=?9, model_name=?10, is_default=?11, updated_at=?12
             WHERE id=?1",
            params![p.id, p.name, p.title, p.emoji, p.description, p.system_prompt, p.temperature, p.response_style, p.model_provider, p.model_name, p.is_default, p.updated_at],
        )?;
        Ok(())
    }

    pub fn delete_persona(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM personas WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_persona_default(&self) -> Result<()> {
        self.conn.execute("UPDATE personas SET is_default = 0 WHERE is_default = 1", [])?;
        Ok(())
    }

    // ── Persona-Memory relations ──

    pub fn link_memory_to_persona(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO persona_memories (persona_id, memory_id) VALUES (?1, ?2)",
            params![persona_id, memory_id],
        )?;
        Ok(())
    }

    pub fn unlink_memory_from_persona(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM persona_memories WHERE persona_id = ?1 AND memory_id = ?2",
            params![persona_id, memory_id],
        )?;
        Ok(())
    }

    pub fn get_persona_memory_ids(&self, persona_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT memory_id FROM persona_memories WHERE persona_id = ?1"
        )?;
        let ids = stmt.query_map(params![persona_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    // ── Persona-Project bindings ──

    pub fn bind_persona_to_project(&self, persona_id: &str, project_path: &str, auto_select: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO persona_projects (persona_id, project_path, auto_select) VALUES (?1, ?2, ?3)",
            params![persona_id, project_path, auto_select],
        )?;
        Ok(())
    }

    pub fn unbind_persona_from_project(&self, persona_id: &str, project_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM persona_projects WHERE persona_id = ?1 AND project_path = ?2",
            params![persona_id, project_path],
        )?;
        Ok(())
    }

    /// Update session execution_status (used by ExecutionRuntime via conn access)
    pub fn update_session_execution_status(&self, session_id: &str, status_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET execution_status = ?1 WHERE id = ?2",
            params![status_json, session_id],
        )?;
        Ok(())
    }

    pub fn update_session_execution(&self, session_id: &str, mode: &str, status_json: &str, active_plan_id: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET mode = ?1, execution_status = ?2, active_plan_id = ?3 WHERE id = ?4",
            params![mode, status_json, active_plan_id, session_id],
        )?;
        Ok(())
    }

    pub fn get_personas_for_project(&self, project_path: &str) -> Result<Vec<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.title, p.emoji, p.description, p.system_prompt, p.temperature, p.response_style, p.model_provider, p.model_name, p.is_default, p.created_at, p.updated_at
             FROM personas p
             JOIN persona_projects pp ON p.id = pp.persona_id
             WHERE pp.project_path = ?1
             ORDER BY pp.auto_select DESC, p.name ASC"
        )?;
        let personas = stmt.query_map(params![project_path], Self::map_persona_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(personas)
    }

    // ── Execution Plans CRUD ──

    pub fn insert_execution_plan(&self, plan: &ExecutionPlanRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO execution_plans (id, session_id, source, goal, plan_json, status, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![plan.id, plan.session_id, plan.source, plan.goal, plan.plan_json, plan.status, plan.created_at, plan.finished_at],
        )?;
        Ok(())
    }

    pub fn update_execution_plan_status(&self, id: &str, status: &str, finished_at: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE execution_plans SET status = ?2, finished_at = ?3 WHERE id = ?1",
            params![id, status, finished_at],
        )?;
        Ok(())
    }

    pub fn get_execution_plan(&self, id: &str) -> Result<Option<ExecutionPlanRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, source, goal, plan_json, status, created_at, finished_at
             FROM execution_plans WHERE id = ?1",
        )?;
        let plan = stmt.query_row(params![id], |row| {
            Ok(ExecutionPlanRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                source: row.get(2)?,
                goal: row.get(3)?,
                plan_json: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                finished_at: row.get(7)?,
            })
        }).optional()?;
        Ok(plan)
    }

    pub fn list_execution_plans(&self, session_id: &str) -> Result<Vec<ExecutionPlanRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, source, goal, plan_json, status, created_at, finished_at
             FROM execution_plans WHERE session_id = ?1 ORDER BY created_at DESC",
        )?;
        let plans = stmt.query_map(params![session_id], |row| {
            Ok(ExecutionPlanRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                source: row.get(2)?,
                goal: row.get(3)?,
                plan_json: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                finished_at: row.get(7)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(plans)
    }

    pub fn upsert_plan_step(&self, step: &PlanStepRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO execution_plan_steps (id, plan_id, step_index, label, step_type, status, result_json, error, started_at, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                result_json = excluded.result_json,
                error = excluded.error,
                duration_ms = excluded.duration_ms",
            params![step.id, step.plan_id, step.step_index, step.label, step.step_type,
                    step.status, step.result_json, step.error, step.started_at, step.duration_ms],
        )?;
        Ok(())
    }

    pub fn get_plan_steps(&self, plan_id: &str) -> Result<Vec<PlanStepRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, plan_id, step_index, label, step_type, status, result_json, error, started_at, duration_ms
             FROM execution_plan_steps WHERE plan_id = ?1 ORDER BY step_index ASC",
        )?;
        let steps = stmt.query_map(params![plan_id], |row| {
            Ok(PlanStepRecord {
                id: row.get(0)?,
                plan_id: row.get(1)?,
                step_index: row.get(2)?,
                label: row.get(3)?,
                step_type: row.get(4)?,
                status: row.get(5)?,
                result_json: row.get(6)?,
                error: row.get(7)?,
                started_at: row.get(8)?,
                duration_ms: row.get(9)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(steps)
    }
}
