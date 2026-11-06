//! 会话相关的数据库操作：会话、消息、会话摘要、系统提示词与设置。
//!
//! 与其他仓储文件一样，这里的方法都是 `impl Database` 的固有方法，
//! 调用方统一通过 [`Database`] 使用，无需感知文件拆分。

use rusqlite::{params, Connection, OptionalExtension};

use crate::core::db::models::{Message, Session, SessionSummary, Setting, SystemPrompt};
use crate::core::db::repository::Database;
use crate::core::error::Result;

impl Database {
    /// 插入一个会话。
    ///
    /// 当调用方未提供 `intent_domain_id` 时，用当前 `is_active` 的领域补齐
    /// （即新会话的默认领域）；其余字段按传入值原样写入。只影响这一列，不做其他联动。
    pub fn create_session(&self, sess: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id, intent_domain_id, intent_domain_mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, COALESCE(?15, (SELECT id FROM intent_domains WHERE is_active = 1 LIMIT 1)), ?16)",
            params![sess.id, sess.title, sess.model_id, sess.system_prompt, sess.persona_id, sess.config, sess.title_source, sess.archived as i32, sess.created_at, sess.updated_at, sess.mode, sess.execution_status, sess.active_plan_id, sess.project_id, sess.intent_domain_id, sess.intent_domain_mode],
        )?;
        Ok(())
    }

    /// 列出全部会话（含已归档），按 `updated_at` 倒序。
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id, intent_domain_id, intent_domain_mode FROM sessions ORDER BY updated_at DESC",
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
                intent_domain_id: row.get(14)?,
                intent_domain_mode: row.get(15)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    /// 按 id 取会话；不存在返回 `None`。
    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id, project_id, intent_domain_id, intent_domain_mode FROM sessions WHERE id = ?1",
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
                intent_domain_id: row.get(14)?,
                intent_domain_mode: row.get(15)?,
            })
        }).optional()?;
        Ok(sess)
    }

    /// 删除会话及其消息、摘要和请求上下文设置（`request_ctx:<id>`）。
    ///
    /// 四条 DELETE 语句**没有包在事务中**：极端失败情况下可能只删掉一部分。
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM session_summaries WHERE session_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        self.conn.execute("DELETE FROM settings WHERE key = ?1", params![format!("request_ctx:{}", id)])?;
        Ok(())
    }

    /// 更新标题，并把 `updated_at` 刷新为当前时间。
    pub fn update_session_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET title = ?1, updated_at = datetime('now') WHERE id = ?2", params![title, id])?;
        Ok(())
    }

    /// 更新模型 id，并把 `updated_at` 刷新为当前时间。
    pub fn update_session_model(&self, id: &str, model_id: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET model_id = ?1, updated_at = datetime('now') WHERE id = ?2", params![model_id, id])?;
        Ok(())
    }

    /// 更新会话级系统提示词，并把 `updated_at` 刷新为当前时间。
    pub fn update_session_system_prompt(&self, id: &str, system_prompt: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET system_prompt = ?1, updated_at = datetime('now') WHERE id = ?2", params![system_prompt, id])?;
        Ok(())
    }

    /// 更新会话配置 JSON 字符串，并把 `updated_at` 刷新为当前时间。
    pub fn update_session_config(&self, id: &str, config: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET config = ?1, updated_at = datetime('now') WHERE id = ?2", params![config, id])?;
        Ok(())
    }

    /// 更新会话的意图领域状态（`intent_domain_id` 与 `intent_domain_mode`）。
    ///
    /// 只改这两列，**不**刷新 `updated_at`，避免路由写入把会话顶到列表最前面。
    pub fn set_session_intent_domain(
        &self,
        session_id: &str,
        domain_id: Option<&str>,
        mode: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET intent_domain_id = ?1, intent_domain_mode = ?2 WHERE id = ?3",
            params![domain_id, mode, session_id],
        )?;
        Ok(())
    }

    /// 在给定连接或事务上解绑引用 `domain_id` 的会话：领域置 NULL，模式回落 `auto`。
    ///
    /// 项目内 `PRAGMA foreign_keys` 为 OFF，领域删除时必须显式调用本方法；
    /// 传入事务连接可与删除操作保持原子。
    pub(crate) fn clear_intent_domain_references_on(conn: &Connection, domain_id: &str) -> Result<()> {
        // 本项目关闭了外键，因此会话引用必须显式清理；模式回落到 `auto`。
        conn.execute(
            "UPDATE sessions SET intent_domain_id = NULL, intent_domain_mode = 'auto' WHERE intent_domain_id = ?1",
            params![domain_id],
        )?;
        Ok(())
    }

    /// 解绑所有引用 `domain_id` 的会话：领域回到“无先验”，模式回到 `auto`。
    pub fn clear_intent_domain_references(&self, domain_id: &str) -> Result<()> {
        Self::clear_intent_domain_references_on(&self.conn, domain_id)
    }

    /// 清空会话的全部消息，并把会话的 `updated_at` 刷新为当前时间。
    pub fn clear_messages(&self, session_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM messages WHERE session_id = ?1", params![session_id])?;
        self.conn.execute("UPDATE sessions SET updated_at = datetime('now') WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    // ── 会话摘要 CRUD ──

    /// 插入一条会话摘要；不校验起止消息 id 是否真实存在。
    pub fn insert_summary(&self, summary: &SessionSummary) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_summaries (id, session_id, message_start_id, message_end_id, summary, key_points, original_token_count, summary_token_count, model_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![summary.id, summary.session_id, summary.message_start_id, summary.message_end_id, summary.summary, summary.key_points, summary.original_token_count, summary.summary_token_count, summary.model_used, summary.created_at],
        )?;
        Ok(())
    }

    /// 取会话的全部摘要，按 `created_at` 升序（即时间线顺序）。
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

    /// 取最近一条摘要覆盖到的最后一条消息 id；没有任何摘要时返回 `None`。
    pub fn get_latest_summary_end_id(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT message_end_id FROM session_summaries WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )?;
        let id = stmt.query_row(params![session_id], |row| row.get(0)).optional()?;
        Ok(id)
    }

    /// 找出可归档的会话 id：未归档且 `updated_at` 早于 N 天前。仅查询，不执行归档。
    pub fn list_archivable_sessions(&self, archive_after_days: u32) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM sessions WHERE archived = 0 AND updated_at < datetime('now', ?1)",
        )?;
        let since = format!("-{} days", archive_after_days);
        let ids = stmt.query_map(params![since], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// 设置归档标记；不刷新 `updated_at`。
    pub fn set_session_archived(&self, id: &str, archived: bool) -> Result<()> {
        self.conn.execute("UPDATE sessions SET archived = ?1 WHERE id = ?2", params![archived as i32, id])?;
        Ok(())
    }

    /// 同时更新标题与标题来源（`title_source`），并刷新 `updated_at`。
    pub fn update_session_title_with_source(&self, id: &str, title: &str, source: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?1, title_source = ?2, updated_at = datetime('now') WHERE id = ?3",
            params![title, source, id],
        )?;
        Ok(())
    }

    /// 插入一条消息，并把所属会话的 `updated_at` 刷新为当前时间。
    ///
    /// 两条语句未包事务；且外键关闭，会话不存在时消息仍可能插入成功。
    pub fn insert_message(&self, msg: &Message) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, tool_name, tokens, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![msg.id, msg.session_id, msg.role, msg.content, msg.tool_calls, msg.tool_call_id, msg.tool_name, msg.tokens, msg.created_at],
        )?;
        self.conn.execute("UPDATE sessions SET updated_at = datetime('now') WHERE id = ?1", params![msg.session_id])?;
        Ok(())
    }

    /// 取会话的全部消息，按 `created_at` 升序；时间戳相同的消息相对顺序由 SQLite 决定。
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, tool_calls, tool_call_id, tool_name, tokens, created_at FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let messages = stmt.query_map(params![session_id], |row| {
            Ok(Message { id: row.get(0)?, session_id: row.get(1)?, role: row.get(2)?, content: row.get(3)?, tool_calls: row.get(4)?, tool_call_id: row.get(5)?, tool_name: row.get(6)?, tokens: row.get(7)?, created_at: row.get(8)? })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// 写入或覆盖一个键值设置（upsert）。
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, value])?;
        Ok(())
    }

    /// 读取设置；键不存在返回 `None`。
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let value = stmt.query_row(params![key], |row| row.get(0)).optional()?;
        Ok(value)
    }

    /// 读取全部设置，**无序**：调用方不应依赖返回顺序。
    pub fn get_all_settings(&self) -> Result<Vec<Setting>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let settings = stmt.query_map([], |row| Ok(Setting { key: row.get(0)?, value: row.get(1)? }))?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(settings)
    }

    /// 插入系统提示词。若 `is_default` 为真，先清空其他默认标记；两条语句未包事务。
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

    /// 列出全部提示词，按 `created_at` 倒序。
    pub fn list_system_prompts(&self) -> Result<Vec<SystemPrompt>> {
        let mut stmt = self.conn.prepare("SELECT id, name, content, is_default, created_at FROM system_prompts ORDER BY created_at DESC")?;
        let prompts = stmt.query_map([], |row| {
            Ok(SystemPrompt { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, is_default: row.get::<_, i32>(3)? != 0, created_at: row.get(4)? })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(prompts)
    }

    /// 按 id 取提示词；不存在返回 `None`。
    pub fn get_system_prompt(&self, id: &str) -> Result<Option<SystemPrompt>> {
        let mut stmt = self.conn.prepare("SELECT id, name, content, is_default, created_at FROM system_prompts WHERE id = ?1")?;
        let prompt = stmt.query_row(params![id], |row| {
            Ok(SystemPrompt { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, is_default: row.get::<_, i32>(3)? != 0, created_at: row.get(4)? })
        }).optional()?;
        Ok(prompt)
    }

    /// 删除提示词；不存在时静默成功。
    pub fn delete_system_prompt(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM system_prompts WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 把指定提示词设为唯一默认（先清空所有默认标记）。
    ///
    /// id 不存在时不会留下任何默认项，也不报错。
    pub fn set_default_system_prompt(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE system_prompts SET is_default = 0", [])?;
        self.conn.execute("UPDATE system_prompts SET is_default = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 取默认提示词；没有默认项时返回 `None`。
    pub fn get_default_system_prompt(&self) -> Result<Option<SystemPrompt>> {
        let mut stmt = self.conn.prepare("SELECT id, name, content, is_default, created_at FROM system_prompts WHERE is_default = 1 LIMIT 1")?;
        let prompt = stmt.query_row([], |row| {
            Ok(SystemPrompt { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, is_default: row.get::<_, i32>(3)? != 0, created_at: row.get(4)? })
        }).optional()?;
        Ok(prompt)
    }

    // ── 会话执行状态 ──

    /// 只更新会话的 `execution_status`（JSON 字符串），不刷新 `updated_at`。
    /// 只更新会话的 `execution_status`（JSON 字符串），不刷新 `updated_at`。
    pub fn update_session_execution_status(&self, session_id: &str, status_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET execution_status = ?1 WHERE id = ?2",
            params![status_json, session_id],
        )?;
        Ok(())
    }

    /// 一次性更新会话的运行模式、执行状态与活跃计划 id；`active_plan_id` 为 `None` 表示清空。
    pub fn update_session_execution(&self, session_id: &str, mode: &str, status_json: &str, active_plan_id: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET mode = ?1, execution_status = ?2, active_plan_id = ?3 WHERE id = ?4",
            params![mode, status_json, active_plan_id, session_id],
        )?;
        Ok(())
    }
}
