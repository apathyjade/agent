use rusqlite::{params, OptionalExtension};

use crate::core::db::models::IntentDomainRecord;
use crate::core::db::repository::Database;
use crate::core::error::{AppError, Result};

impl Database {
    fn map_intent_domain_row(row: &rusqlite::Row) -> rusqlite::Result<IntentDomainRecord> {
        Ok(IntentDomainRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            is_active: row.get::<_, i64>(3)? != 0,
            source: row.get(4)?,
            version: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    pub fn list_intent_domains(&self) -> Result<Vec<IntentDomainRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, is_active, source, version, created_at, updated_at
             FROM intent_domains ORDER BY is_active DESC, name ASC",
        )?;
        let domains = stmt
            .query_map([], Self::map_intent_domain_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(domains)
    }

    pub fn get_intent_domain(&self, id: &str) -> Result<Option<IntentDomainRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, is_active, source, version, created_at, updated_at
             FROM intent_domains WHERE id = ?1",
        )?;
        let domain = stmt
            .query_row(params![id], Self::map_intent_domain_row)
            .optional()?;
        Ok(domain)
    }

    pub fn get_active_intent_domain(&self) -> Result<Option<IntentDomainRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, is_active, source, version, created_at, updated_at
             FROM intent_domains WHERE is_active = 1 LIMIT 1",
        )?;
        let domain = stmt.query_row([], Self::map_intent_domain_row).optional()?;
        Ok(domain)
    }

    pub fn insert_intent_domain(&self, d: &IntentDomainRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO intent_domains (id, name, description, is_active, source, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                d.id,
                d.name,
                d.description,
                d.is_active as i32,
                d.source,
                d.version,
                d.created_at,
                d.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Updates the user-editable fields only; `is_active`, `source` and `version` are untouched.
    pub fn update_intent_domain(&self, d: &IntentDomainRecord) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE intent_domains SET name = ?2, description = ?3, updated_at = ?4 WHERE id = ?1",
            params![d.id, d.name, d.description, d.updated_at],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("领域不存在: {}", d.id)));
        }
        Ok(())
    }

    /// Deletes a domain together with its intents.
    /// `PRAGMA foreign_keys` is OFF, so the child rows must be removed explicitly.
    pub fn delete_intent_domain(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let is_active: Option<i64> = tx
            .query_row(
                "SELECT is_active FROM intent_domains WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;

        match is_active {
            None => return Err(AppError::NotFound(format!("领域不存在: {id}"))),
            Some(active) if active != 0 => {
                return Err(AppError::InvalidInput("不能删除当前活跃领域".to_string()))
            }
            Some(_) => {}
        }

        tx.execute("DELETE FROM intents WHERE domain_id = ?1", params![id])?;
        tx.execute("DELETE FROM intent_domains WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically makes `id` the single active domain.
    pub fn set_active_intent_domain(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let exists: Option<i64> = tx
            .query_row(
                "SELECT is_active FROM intent_domains WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("领域不存在: {id}")));
        }

        let enabled: i64 = tx.query_row(
            "SELECT COUNT(*) FROM intents WHERE domain_id = ?1 AND enabled = 1",
            params![id],
            |row| row.get(0),
        )?;
        if enabled == 0 {
            return Err(AppError::InvalidInput(
                "该领域没有启用的意图，无法激活".to_string(),
            ));
        }

        tx.execute("UPDATE intent_domains SET is_active = 0", [])?;
        tx.execute(
            "UPDATE intent_domains SET is_active = 1 WHERE id = ?1",
            params![id],
        )?;
        if tx.changes() == 0 {
            return Err(AppError::NotFound(format!("领域不存在: {id}")));
        }
        tx.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn count_intents(&self, domain_id: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM intents WHERE domain_id = ?1",
                params![domain_id],
                |row| row.get(0),
            )
            .unwrap_or(-1)
    }

    #[cfg(test)]
    pub fn count_columns(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}')"), [], |r| {
                r.get(0)
            })
            .unwrap_or(-1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::IntentDomainRecord;
    use crate::core::error::AppError;

    /// Fixed timestamp so tests exercise exactly what the caller supplies.
    const TEST_TS: &str = "2026-11-04T00:00:00Z";

    fn mk_domain(id: &str, name: &str) -> IntentDomainRecord {
        IntentDomainRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            is_active: false,
            source: "user".to_string(),
            version: 1,
            created_at: TEST_TS.to_string(),
            updated_at: TEST_TS.to_string(),
        }
    }

    /// Insert a minimal intent row directly; defaults fill the remaining columns.
    fn insert_enabled_intent(db: &Database, id: &str, domain_id: &str, enabled: bool) {
        db.conn
            .execute(
                "INSERT INTO intents (id, domain_id, name, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, domain_id, id, enabled as i32, TEST_TS, TEST_TS],
            )
            .unwrap();
    }

    fn active_ids(db: &Database) -> Vec<String> {
        db.list_intent_domains()
            .unwrap()
            .into_iter()
            .filter(|d| d.is_active)
            .map(|d| d.id)
            .collect()
    }

    #[test]
    fn intent_tables_exist_with_expected_columns() {
        let db = Database::new_test().unwrap();
        let domains = db.count_columns("intent_domains");
        let intents = db.count_columns("intents");
        assert_eq!(domains, 8, "intent_domains 列数不符");
        assert_eq!(intents, 21, "intents 列数不符");
    }

    #[test]
    fn delete_active_domain_is_rejected() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("active-domain", "活跃领域"))
            .unwrap();
        insert_enabled_intent(&db, "active-intent", "active-domain", true);
        db.set_active_intent_domain("active-domain").unwrap();

        let err = db.delete_intent_domain("active-domain").unwrap_err();
        match err {
            AppError::InvalidInput(msg) => assert_eq!(msg, "不能删除当前活跃领域"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        assert!(db.get_intent_domain("active-domain").unwrap().is_some());
        assert_eq!(db.count_intents("active-domain"), 1);
        assert_eq!(active_ids(&db), vec!["active-domain".to_string()]);
    }

    #[test]
    fn activating_domain_without_enabled_intents_is_rejected() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("empty-domain", "空领域"))
            .unwrap();
        // Only a disabled intent: it must not satisfy the guard.
        insert_enabled_intent(&db, "disabled-intent", "empty-domain", false);

        let err = db.set_active_intent_domain("empty-domain").unwrap_err();
        match err {
            AppError::InvalidInput(msg) => assert_eq!(msg, "该领域没有启用的意图，无法激活"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        assert!(db.get_active_intent_domain().unwrap().is_none());
        assert!(active_ids(&db).is_empty());
    }

    #[test]
    fn set_active_domain_is_exclusive() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("domain-a", "领域甲"))
            .unwrap();
        db.insert_intent_domain(&mk_domain("domain-b", "领域乙"))
            .unwrap();
        insert_enabled_intent(&db, "intent-a", "domain-a", true);
        insert_enabled_intent(&db, "intent-b", "domain-b", true);

        db.set_active_intent_domain("domain-a").unwrap();
        assert_eq!(active_ids(&db), vec!["domain-a".to_string()]);
        assert_eq!(
            db.get_active_intent_domain().unwrap().map(|d| d.id),
            Some("domain-a".to_string())
        );

        db.set_active_intent_domain("domain-b").unwrap();
        assert_eq!(active_ids(&db), vec!["domain-b".to_string()]);
        assert_eq!(
            db.get_active_intent_domain().unwrap().map(|d| d.id),
            Some("domain-b".to_string())
        );
    }

    #[test]
    fn deleting_inactive_domain_removes_its_intents() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("doomed-domain", "待删领域"))
            .unwrap();
        insert_enabled_intent(&db, "doomed-intent", "doomed-domain", true);

        db.delete_intent_domain("doomed-domain").unwrap();

        assert!(db.get_intent_domain("doomed-domain").unwrap().is_none());
        assert_eq!(db.count_intents("doomed-domain"), 0);
    }
}
