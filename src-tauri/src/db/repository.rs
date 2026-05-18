use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

use crate::db::models::{Conversation, Message, Setting, SystemPrompt};
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
}
