use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::models::{BoundProjectModel, Conversation, Message, RuntimeVersionCache, Setting, SkillRecord, SystemPrompt};
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

    fn init_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = OFF;

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                model_id TEXT NOT NULL,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_calls TEXT,
                tool_call_id TEXT,
                tokens INTEGER,
                created_at TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
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

            CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id);
            ",
        )?;
        Ok(())
    }

    fn migrate_tables(conn: &Connection) -> Result<()> {
        let has_provider = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('conversations') WHERE name='provider'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_provider > 0 {
            conn.execute_batch(
                "
                ALTER TABLE conversations RENAME TO conversations_old;

                CREATE TABLE conversations (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    system_prompt TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                INSERT INTO conversations (id, title, model_id, system_prompt, created_at, updated_at)
                SELECT id, title, COALESCE(model, 'default-openai'), system_prompt, created_at, updated_at
                FROM conversations_old;

                DROP TABLE conversations_old;
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

        Ok(())
    }

    pub fn create_conversation(&self, conv: &Conversation) -> Result<()> {
        self.conn.execute(
            "INSERT INTO conversations (id, title, model_id, system_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![conv.id, conv.title, conv.model_id, conv.system_prompt, conv.created_at, conv.updated_at],
        )?;
        Ok(())
    }

    pub fn list_conversations(&self) -> Result<Vec<Conversation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, created_at, updated_at FROM conversations ORDER BY updated_at DESC",
        )?;
        let conversations = stmt.query_map([], |row| {
            Ok(Conversation { id: row.get(0)?, title: row.get(1)?, model_id: row.get(2)?, system_prompt: row.get(3)?, created_at: row.get(4)?, updated_at: row.get(5)? })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(conversations)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, created_at, updated_at FROM conversations WHERE id = ?1",
        )?;
        let conv = stmt.query_row(params![id], |row| {
            Ok(Conversation { id: row.get(0)?, title: row.get(1)?, model_id: row.get(2)?, system_prompt: row.get(3)?, created_at: row.get(4)?, updated_at: row.get(5)? })
        }).optional()?;
        Ok(conv)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM messages WHERE conversation_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_conversation_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute("UPDATE conversations SET title = ?1, updated_at = datetime('now') WHERE id = ?2", params![title, id])?;
        Ok(())
    }

    pub fn update_conversation_model(&self, id: &str, model_id: &str) -> Result<()> {
        self.conn.execute("UPDATE conversations SET model_id = ?1, updated_at = datetime('now') WHERE id = ?2", params![model_id, id])?;
        Ok(())
    }

    pub fn update_conversation_system_prompt(&self, id: &str, system_prompt: &str) -> Result<()> {
        self.conn.execute("UPDATE conversations SET system_prompt = ?1, updated_at = datetime('now') WHERE id = ?2", params![system_prompt, id])?;
        Ok(())
    }

    pub fn clear_messages(&self, conversation_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM messages WHERE conversation_id = ?1", params![conversation_id])?;
        self.conn.execute("UPDATE conversations SET updated_at = datetime('now') WHERE id = ?1", params![conversation_id])?;
        Ok(())
    }

    pub fn insert_message(&self, msg: &Message) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (id, conversation_id, role, content, tool_calls, tool_call_id, tokens, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![msg.id, msg.conversation_id, msg.role, msg.content, msg.tool_calls, msg.tool_call_id, msg.tokens, msg.created_at],
        )?;
        self.conn.execute("UPDATE conversations SET updated_at = datetime('now') WHERE id = ?1", params![msg.conversation_id])?;
        Ok(())
    }

    pub fn get_messages(&self, conversation_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conversation_id, role, content, tool_calls, tool_call_id, tokens, created_at FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC",
        )?;
        let messages = stmt.query_map(params![conversation_id], |row| {
            Ok(Message { id: row.get(0)?, conversation_id: row.get(1)?, role: row.get(2)?, content: row.get(3)?, tool_calls: row.get(4)?, tool_call_id: row.get(5)?, tokens: row.get(6)?, created_at: row.get(7)? })
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

    pub fn skill_exists(&self, id: &str) -> Result<bool> {
        let count: i32 = self.conn.query_row("SELECT COUNT(*) FROM skills WHERE id = ?1", params![id], |row| row.get(0))?;
        Ok(count > 0)
    }
}
