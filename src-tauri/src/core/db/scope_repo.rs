//! 作用域声明的持久化。
//!
//! 这里只负责存取 JSON 文本：解析、校验与语义解析由 `llm::intent::scope`
//! 负责，因此 `core` 层不依赖 `llm` 层。

use rusqlite::{params, OptionalExtension};

use crate::core::db::repository::Database;
use crate::core::error::Result;

/// `intents` 行上的作用域。
pub const OWNER_INTENT: &str = "intent";
/// `intent_domains` 行上的默认作用域。
pub const OWNER_DOMAIN: &str = "domain";
/// 会话级的临时作用域。
pub const OWNER_SESSION: &str = "session";

impl Database {
    /// 读取某个所有者上的作用域 JSON；不存在返回 `None`。
    pub fn get_scope(&self, owner_kind: &str, owner_id: &str) -> Result<Option<String>> {
        let scope = self
            .conn
            .query_row(
                "SELECT scope_json FROM resource_scopes WHERE owner_kind = ?1 AND owner_id = ?2",
                params![owner_kind, owner_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(scope)
    }

    /// 写入（或覆盖）作用域 JSON。同一 `(owner_kind, owner_id)` 是 upsert。
    pub fn set_scope(
        &self,
        owner_kind: &str,
        owner_id: &str,
        scope_json: &str,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO resource_scopes (owner_kind, owner_id, scope_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(owner_kind, owner_id) DO UPDATE SET
                 scope_json = excluded.scope_json,
                 updated_at = excluded.updated_at",
            params![owner_kind, owner_id, scope_json, updated_at],
        )?;
        Ok(())
    }

    /// 删除某个所有者上的作用域；不存在时是空操作。
    pub fn delete_scope(&self, owner_kind: &str, owner_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM resource_scopes WHERE owner_kind = ?1 AND owner_id = ?2",
            params![owner_kind, owner_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::new_test().expect("内存数据库必须可用")
    }

    #[test]
    fn scope_round_trips() {
        let db = db();
        db.set_scope(OWNER_INTENT, "intent-1", r#"{"schema_version":1}"#, "t0")
            .unwrap();
        assert_eq!(
            db.get_scope(OWNER_INTENT, "intent-1").unwrap().as_deref(),
            Some(r#"{"schema_version":1}"#)
        );
    }

    #[test]
    fn missing_scope_is_none() {
        let db = db();
        assert!(db.get_scope(OWNER_INTENT, "nope").unwrap().is_none());
    }

    #[test]
    fn set_scope_upserts() {
        let db = db();
        db.set_scope(OWNER_INTENT, "intent-1", "{\"v\":1}", "t0").unwrap();
        db.set_scope(OWNER_INTENT, "intent-1", "{\"v\":2}", "t1").unwrap();
        assert_eq!(
            db.get_scope(OWNER_INTENT, "intent-1").unwrap().as_deref(),
            Some("{\"v\":2}")
        );
    }

    #[test]
    fn delete_scope_removes_row() {
        let db = db();
        db.set_scope(OWNER_DOMAIN, "domain-1", "{}", "t0").unwrap();
        db.delete_scope(OWNER_DOMAIN, "domain-1").unwrap();
        assert!(db.get_scope(OWNER_DOMAIN, "domain-1").unwrap().is_none());
    }

    #[test]
    fn owner_kinds_are_isolated() {
        let db = db();
        db.set_scope(OWNER_INTENT, "shared", "{\"kind\":\"intent\"}", "t0")
            .unwrap();
        db.set_scope(OWNER_SESSION, "shared", "{\"kind\":\"session\"}", "t0")
            .unwrap();
        assert_eq!(
            db.get_scope(OWNER_INTENT, "shared").unwrap().as_deref(),
            Some("{\"kind\":\"intent\"}")
        );
        assert_eq!(
            db.get_scope(OWNER_SESSION, "shared").unwrap().as_deref(),
            Some("{\"kind\":\"session\"}")
        );
    }
}
