//! 数据库连接、表结构与迁移，以及项目、绑定项目、运行时版本缓存和工作流运行记录的读写。
//!
//! 会话、记忆、人格、执行计划与意图相关的 `impl Database` 分别位于
//! `session_repo`、`memory_repo`、`persona_repo`、`plan_repo` 与 `intent_repo`；
//! 调用方统一通过 [`Database`] 使用，无需感知文件划分。

use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::core::db::models::{BoundProjectModel, Project, RuntimeVersionCache, Session, WorkflowRunRecord};
use crate::core::error::Result;

/// SQLite 连接持有者，所有持久化操作的入口。
///
/// 连接由上层以 `Arc<Mutex<Database>>` 共享，因此这里的方法都只借用 `&self`：
/// 单条语句依赖 SQLite 自身的串行化，需要多语句原子性的方法在内部显式开事务。
pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    /// 打开（必要时创建）磁盘上的数据库文件，并保证表结构与迁移就绪。
    ///
    /// 副作用：创建父目录、建表、执行迁移。`migrate_tables` 的每一步都可重复执行，
    /// 因此重复调用是安全的。
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

    /// 数据库文件的固定位置：`<系统数据目录>/agent/agent.db`。
    /// 取不到系统数据目录时退化为当前目录，便于受限环境下启动。
    fn db_path() -> PathBuf {
        let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("agent");
        path.push("agent.db");
        path
    }

    /// 创建一个仅存在于内存中的测试数据库，表结构与迁移与真实库一致。
    /// 只在测试构建中可用，测试之间互不共享数据。
    #[cfg(test)]
    pub fn new_test() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        Self::init_tables(&conn)?;
        Self::migrate_tables(&conn)?;
        Ok(Self { conn })
    }

    /// 建立全部表、索引、FTS5 虚表与触发器（均带 `IF NOT EXISTS`，可重复执行）。
    ///
    /// 注意：这里刻意保持 `PRAGMA foreign_keys = OFF`，级联删除由各仓储方法显式处理。
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
                tool_name TEXT,
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

            CREATE TABLE IF NOT EXISTS intent_domains (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT NOT NULL DEFAULT '',
                is_active INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'user',
                version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS intents (
                id TEXT PRIMARY KEY,
                domain_id TEXT NOT NULL REFERENCES intent_domains(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                examples TEXT NOT NULL DEFAULT '[]',
                workflow TEXT NOT NULL DEFAULT '',
                allowed_tools TEXT NOT NULL DEFAULT '[]',
                default_agent TEXT NOT NULL DEFAULT '',
                scope TEXT NOT NULL DEFAULT 'global',
                enabled INTEGER NOT NULL DEFAULT 1,
                version INTEGER NOT NULL DEFAULT 1,
                confidence_threshold REAL NOT NULL DEFAULT 0.7,
                fallback_intent TEXT NOT NULL DEFAULT '',
                path TEXT NOT NULL DEFAULT 'fast',
                auto_escalate INTEGER NOT NULL DEFAULT 0,
                system_prompt_appendix TEXT NOT NULL DEFAULT '',
                model_id TEXT NOT NULL DEFAULT '',
                max_iterations INTEGER,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(domain_id, name)
            );

            CREATE INDEX IF NOT EXISTS idx_intents_domain ON intents(domain_id, sort_order);

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
            ",
        )?;

        // 记忆全文检索用的 FTS5 虚表（索引 content 与 tags 两列）。
        // 采用 external content 模式：这里只存索引，正文仍在 memories 表中。
        // unicode61 分词器同时支持英文与中日韩文字。
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

        // 会话摘要表，用于对话生命周期管理（压缩历史消息）。
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

    /// 按版本逐项补齐历史库缺失的列与表，并在需要时重建 FTS 索引。
    ///
    /// 每一步都先探测目标列/表是否存在，因此对已是最新结构的库是空操作。
    /// 迁移失败时错误向上传播，不会继续执行后续步骤。
    fn migrate_tables(conn: &Connection) -> Result<()> {
        // 迁移 v1：重命名 provider 列（旧结构）。
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

        // 迁移 v3：为 workflow_runs 增加 trigger_type 与 step_progress 列。
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

        // 迁移 v4：把 sessersations 改名为 sessions、sessersation_id 改名为 session_id。
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

        // 迁移 v6：为 sessions 增加 title_source 与 archived 列。
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

        // 迁移 v7：为 sessions 增加 persona_id 列。
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

        // 迁移 v8：为 sessions 增加 config 列。
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

        // 迁移 v9：为 sessions 增加 mode、execution_status 与 active_plan_id 列。
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

        // 迁移 v10：为 sessions 增加 project_id 列。
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

        // 迁移 v11：为 sessions 增加会话级意图领域状态。
        // `intent_domain_id` 是会话最近所属的领域（仅作为路由先验，不是约束）；
        // `intent_domain_mode` 的合法值为 "auto" 或 "pinned"。
        let has_intent_domain_id = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='intent_domain_id'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_intent_domain_id == 0 {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN intent_domain_id TEXT;
                 ALTER TABLE sessions ADD COLUMN intent_domain_mode TEXT NOT NULL DEFAULT 'auto';",
            )?;
        }

        // 迁移 v12：为 messages 增加工具名，避免历史里的工具结果只能叫 "tool"。
        let has_tool_name = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='tool_name'",
            [],
            |row| row.get::<_, i32>(0),
        ).unwrap_or(0);

        if has_tool_name == 0 {
            conn.execute_batch("ALTER TABLE messages ADD COLUMN tool_name TEXT;")?;
        }

        // 迁移 v5：为尚未建立索引的历史记忆重建 FTS5 索引。
        // FTS 表刚在 init_tables 中创建（或上次运行就已存在），
        // 因此这里只需判断“memories 有数据而 FTS 索引为空”的情况。
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

    // ── 工作流运行记录 ──

    /// 插入一条工作流运行记录；不做去重，`id` 冲突会返回数据库错误。
    pub fn insert_workflow_run(&self, run: &WorkflowRunRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workflow_runs (id, workflow_name, status, step_results, step_progress, error, trigger_type, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![run.id, run.workflow_name, run.status, run.step_results, run.step_progress, run.error, run.trigger_type, run.started_at, run.finished_at],
        )?;
        Ok(())
    }

    /// 更新运行状态、步骤结果与错误信息。
    ///
    /// `step_results` 会被序列化为 JSON，序列化失败时写入空字符串而不是报错。
    /// `status == "running"` 时 `finished_at` 置 NULL，否则写入当前时间，
    /// 因此同一方法既能表示“开始”也能表示“结束”。
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

    /// 按 `started_at` 倒序列出最近的运行记录，`limit` 为硬上限。
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

    /// 按 id 取运行记录；不存在时返回 `None`（不报错）。
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

    // ── 运行时版本缓存 ──

    /// 取某运行时类型的全部缓存版本，按 `version` 字符串倒序（字典序，不做语义化版本比较）。
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

    /// 以 (`runtime_type`, `version`) 为键写入或覆盖缓存条目；自增的 `id` 列不参与。
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

    /// 清空某运行时类型的版本缓存；返回后该类型没有任何缓存条目。
    pub fn clear_version_cache(&self, rt: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM runtime_version_cache WHERE runtime_type = ?1", params![rt])?;
        Ok(())
    }

    // ── 绑定项目 ──

    /// 绑定一个本地项目目录并返回新记录。
    ///
    /// `path` 在库中唯一，重复绑定会返回数据库错误；新记录默认 `auto_sync = true`，
    /// `last_scan` 与 `requirements` 为 NULL。
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

    /// 列出全部绑定项目，按 `updated_at` 倒序。
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

    /// 解除绑定；记录不存在时静默成功。
    pub fn remove_bound_project(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM bound_projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 按 id 取绑定项目；不存在返回 `None`。
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

    /// 覆盖除 `id`/`created_at` 外的字段，并把 `updated_at` 刷新为当前时间。
    /// 记录不存在时静默成功（不报错）。
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

    // ── 项目增删改查 ──

    /// 插入项目记录；`id` 冲突会返回数据库错误。
    pub fn create_project(&self, project: &Project) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project.id, project.name, project.path, project.created_at, project.updated_at],
        )?;
        Ok(())
    }

    /// 列出全部项目，按 `updated_at` 倒序。
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

    /// 按 id 取项目；不存在返回 `None`。
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

    /// 删除项目记录。不会级联处理会话，调用方应先执行 [`Self::set_sessions_project_null`]。
    pub fn delete_project(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 把归属该项目的会话的 `project_id` 置 NULL（外键关闭，必须显式解绑）。
    pub fn set_sessions_project_null(&self, project_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET project_id = NULL WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }

    /// 取归属某项目的会话，按 `updated_at` 倒序。
    pub fn get_sessions_by_project(&self, project_id: &str) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id, intent_domain_id, intent_domain_mode
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
                intent_domain_id: row.get(14)?,
                intent_domain_mode: row.get(15)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::Session;
    use rusqlite::Connection;

    const TEST_TS: &str = "2026-11-05T00:00:00Z";

    fn mk_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            title: format!("会话-{id}"),
            model_id: "model-a".to_string(),
            system_prompt: None,
            persona_id: None,
            config: None,
            title_source: "manual".to_string(),
            archived: false,
            created_at: TEST_TS.to_string(),
            updated_at: TEST_TS.to_string(),
            mode: "chat".to_string(),
            execution_status: r#"{"type":"idle"}"#.to_string(),
            active_plan_id: None,
            project_id: None,
            intent_domain_id: None,
            intent_domain_mode: "auto".to_string(),
        }
    }

    /// 插入一个领域和一个启用意图，并把该领域设为活跃领域
    /// （即新会话的默认领域）。
    fn seed_active_domain(db: &Database, domain_id: &str) {
        db.conn
            .execute(
                "INSERT INTO intent_domains (id, name, is_active, source, version, created_at, updated_at)
                 VALUES (?1, ?2, 0, 'user', 1, ?3, ?3)",
                params![domain_id, format!("领域-{domain_id}"), TEST_TS],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO intents (id, domain_id, name, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?4)",
                params![format!("intent-{domain_id}"), domain_id, "意图", TEST_TS],
            )
            .unwrap();
        db.set_active_intent_domain(domain_id).unwrap();
    }

    #[test]
    fn migrating_old_sessions_table_adds_intent_domain_columns_with_defaults() {
        let conn = Connection::open_in_memory().unwrap();
        // init_tables 对应 v11 之前的表结构；迁移一个已有数据的库
        // 不能丢数据，也不能在既有行上报错。
        Database::init_tables(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, model_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params!["legacy-session", "旧会话", "model-a", TEST_TS],
        )
        .unwrap();

        Database::migrate_tables(&conn).unwrap();
        // v11 迁移有存在性判断，因此第二次执行必须是空操作。
        Database::migrate_tables(&conn).unwrap();

        let columns: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions')
                 WHERE name IN ('intent_domain_id', 'intent_domain_mode')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(columns, 2, "升级后 sessions 必须存在领域两列");

        let (domain, mode): (Option<String>, String) = conn
            .query_row(
                "SELECT intent_domain_id, intent_domain_mode FROM sessions WHERE id = 'legacy-session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(
            domain.is_none(),
            "既有会话的 intent_domain_id 必须默认为 NULL"
        );
        assert_eq!(mode, "auto", "既有会话的 intent_domain_mode 必须默认为 auto");
    }

    #[test]
    fn create_session_inherits_active_domain() {
        let db = Database::new_test().unwrap();
        seed_active_domain(&db, "domain-a");

        db.create_session(&mk_session("session-a")).unwrap();

        let stored = db.get_session("session-a").unwrap().unwrap();
        assert_eq!(
            stored.intent_domain_id.as_deref(),
            Some("domain-a"),
            "未指定领域的新会话必须继承当前默认（is_active）领域"
        );
        assert_eq!(stored.intent_domain_mode, "auto");
    }

    #[test]
    fn create_session_without_active_domain_leaves_domain_null() {
        let db = Database::new_test().unwrap();

        db.create_session(&mk_session("session-b")).unwrap();

        let stored = db.get_session("session-b").unwrap().unwrap();
        assert!(stored.intent_domain_id.is_none(), "无默认领域时必须保持 NULL");
        assert_eq!(stored.intent_domain_mode, "auto");
    }

    #[test]
    fn set_session_intent_domain_updates_only_intent_columns() {
        let db = Database::new_test().unwrap();
        db.create_session(&mk_session("session-c")).unwrap();
        let before = db.get_session("session-c").unwrap().unwrap();

        db.set_session_intent_domain("session-c", Some("domain-a"), "pinned")
            .unwrap();

        let after = db.get_session("session-c").unwrap().unwrap();
        assert_eq!(after.intent_domain_id.as_deref(), Some("domain-a"));
        assert_eq!(after.intent_domain_mode, "pinned");
        assert_eq!(after.title, before.title, "不得改动标题");
        assert_eq!(after.updated_at, before.updated_at, "只更新领域两列，不得刷新 updated_at");

        db.set_session_intent_domain("session-c", None, "auto")
            .unwrap();
        let cleared = db.get_session("session-c").unwrap().unwrap();
        assert!(cleared.intent_domain_id.is_none(), "传 None 必须清空领域");
        assert_eq!(cleared.intent_domain_mode, "auto");
    }

    #[test]
    fn clear_intent_domain_references_only_touches_matching_sessions() {
        let db = Database::new_test().unwrap();
        let mut referencing = mk_session("session-d");
        referencing.intent_domain_id = Some("domain-a".to_string());
        referencing.intent_domain_mode = "pinned".to_string();
        db.create_session(&referencing).unwrap();

        let mut unrelated = mk_session("session-e");
        unrelated.intent_domain_id = Some("domain-b".to_string());
        unrelated.intent_domain_mode = "pinned".to_string();
        db.create_session(&unrelated).unwrap();

        db.clear_intent_domain_references("domain-a").unwrap();

        let cleared = db.get_session("session-d").unwrap().unwrap();
        assert!(cleared.intent_domain_id.is_none(), "引用会话必须置 NULL");
        assert_eq!(cleared.intent_domain_mode, "auto", "模式必须回落 auto");

        let kept = db.get_session("session-e").unwrap().unwrap();
        assert_eq!(kept.intent_domain_id.as_deref(), Some("domain-b"));
        assert_eq!(kept.intent_domain_mode, "pinned");
    }
}
