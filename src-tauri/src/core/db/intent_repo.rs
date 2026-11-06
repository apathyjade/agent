//! 意图领域（intent domains）与意图（intents）的读写。
//!
//! 领域是路由的第一层分类，意图是领域内的具体处理路径；
//! 两者都有“最后一条启用项不可删除/停用”的一致性约束。

use rusqlite::{params, OptionalExtension};

use crate::core::db::models::{IntentDomainRecord, IntentRecord};
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

    /// 列出全部意图领域：活跃领域排最前，其余按 `name` 升序。
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

    /// 按 id 取领域；不存在返回 `None`。
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

    /// 取当前活跃领域。自会话拥有独立领域状态后，`is_active` 同时充当
    /// **新会话的默认领域**。
    pub fn get_active_intent_domain(&self) -> Result<Option<IntentDomainRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, is_active, source, version, created_at, updated_at
             FROM intent_domains WHERE is_active = 1 LIMIT 1",
        )?;
        let domain = stmt.query_row([], Self::map_intent_domain_row).optional()?;
        Ok(domain)
    }

    /// 插入领域时**强制**写入 `is_active = 0`，忽略记录里的值；
    /// 激活必须走 [`Self::set_active_intent_domain`]。
    pub fn insert_intent_domain(&self, d: &IntentDomainRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO intent_domains (id, name, description, is_active, source, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
            params![
                d.id,
                d.name,
                d.description,
                d.source,
                d.version,
                d.created_at,
                d.updated_at,
            ],
        )?;
        Ok(())
    }

    /// 只更新用户可编辑的字段（`name`、`description`、`updated_at`）；
    /// `is_active`、`source` 与 `version` 保持不变。
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

    /// 在一个事务内删除领域及其意图，并解绑引用该领域的会话。
    ///
    /// `PRAGMA foreign_keys` 为 OFF，所以子行必须显式删除。活跃领域不允许删除。
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
        // 会话按 id 引用领域，而外键是关闭的，因此引用清理必须与删除处于同一事务。
        Self::clear_intent_domain_references_on(&tx, id)?;
        tx.execute("DELETE FROM intent_domains WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// 原子地把 `id` 设为唯一活跃领域；领域不存在或没有启用的意图时报错。
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

    const INTENT_COLUMNS: &str =
        "id, domain_id, name, description, examples, workflow, allowed_tools, \
         default_agent, scope, enabled, version, confidence_threshold, fallback_intent, \
         path, auto_escalate, system_prompt_appendix, model_id, max_iterations, \
         sort_order, created_at, updated_at";

    fn map_intent_row(row: &rusqlite::Row) -> rusqlite::Result<IntentRecord> {
        let examples: String = row.get(4)?;
        let allowed_tools: String = row.get(6)?;
        let max_iterations: Option<i64> = row.get(17)?;
        Ok(IntentRecord {
            id: row.get(0)?,
            domain_id: row.get(1)?,
            name: row.get(2)?,
            description: row.get(3)?,
            // 损坏的 JSON 退化为空列表，而不是 panic。
            examples: serde_json::from_str(&examples).unwrap_or_default(),
            workflow: row.get(5)?,
            allowed_tools: serde_json::from_str(&allowed_tools).unwrap_or_default(),
            default_agent: row.get(7)?,
            scope: row.get(8)?,
            enabled: row.get::<_, i64>(9)? != 0,
            version: row.get(10)?,
            confidence_threshold: row.get(11)?,
            fallback_intent: row.get(12)?,
            path: row.get(13)?,
            auto_escalate: row.get::<_, i64>(14)? != 0,
            system_prompt_appendix: row.get(15)?,
            model_id: row.get(16)?,
            max_iterations: max_iterations.map(|v| v as usize),
            sort_order: row.get(18)?,
            created_at: row.get(19)?,
            updated_at: row.get(20)?,
        })
    }

    /// 取领域的全部意图，按 `sort_order` 升序、再按 `name` 升序。
    /// 取领域的全部意图，按 `sort_order` 升序、再按 `name` 升序。
    pub fn list_intents(&self, domain_id: &str) -> Result<Vec<IntentRecord>> {
        let sql = format!(
            "SELECT {} FROM intents WHERE domain_id = ?1 ORDER BY sort_order ASC, name ASC",
            Self::INTENT_COLUMNS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let intents = stmt
            .query_map(params![domain_id], Self::map_intent_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(intents)
    }

    /// 取领域内已启用的意图，排序与 [`Self::list_intents`] 一致。
    pub fn list_enabled_intents(&self, domain_id: &str) -> Result<Vec<IntentRecord>> {
        let sql = format!(
            "SELECT {} FROM intents WHERE domain_id = ?1 AND enabled = 1 \
             ORDER BY sort_order ASC, name ASC",
            Self::INTENT_COLUMNS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let intents = stmt
            .query_map(params![domain_id], Self::map_intent_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(intents)
    }

    /// 取所有领域内已启用的意图。
    ///
    /// 供级联路由在超出会话先验领域时扩大候选范围。按领域 id 升序排列，
    /// 领域内仍按显示顺序（`sort_order`、`name`），因此每个领域的切片与
    /// [`Self::list_enabled_intents`] 的顺序一致。
    pub fn list_all_enabled_intents(&self) -> Result<Vec<IntentRecord>> {
        let sql = format!(
            "SELECT {} FROM intents WHERE enabled = 1 \
             ORDER BY domain_id ASC, sort_order ASC, name ASC",
            Self::INTENT_COLUMNS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let intents = stmt
            .query_map([], Self::map_intent_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(intents)
    }

    /// 按 id 取意图；不存在返回 `None`。
    pub fn get_intent(&self, id: &str) -> Result<Option<IntentRecord>> {
        let sql = format!("SELECT {} FROM intents WHERE id = ?1", Self::INTENT_COLUMNS);
        let mut stmt = self.conn.prepare(&sql)?;
        let intent = stmt
            .query_row(params![id], Self::map_intent_row)
            .optional()?;
        Ok(intent)
    }

    /// 插入意图。`examples` 与 `allowed_tools` 序列化为 JSON 文本；
    /// `name` 在同一领域内唯一，冲突会返回数据库错误。
    pub fn insert_intent(&self, i: &IntentRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO intents (id, domain_id, name, description, examples, workflow,
                 allowed_tools, default_agent, scope, enabled, version, confidence_threshold,
                 fallback_intent, path, auto_escalate, system_prompt_appendix, model_id,
                 max_iterations, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21)",
            params![
                i.id,
                i.domain_id,
                i.name,
                i.description,
                serde_json::to_string(&i.examples)?,
                i.workflow,
                serde_json::to_string(&i.allowed_tools)?,
                i.default_agent,
                i.scope,
                i.enabled as i32,
                i.version,
                i.confidence_threshold,
                i.fallback_intent,
                i.path,
                i.auto_escalate as i32,
                i.system_prompt_appendix,
                i.model_id,
                i.max_iterations.map(|v| v as i64),
                i.sort_order,
                i.created_at,
                i.updated_at,
            ],
        )?;
        Ok(())
    }

    /// 在一个事务内播种领域：写入领域与意图、激活领域，并记录一个标记设置。
    ///
    /// 要么全部落库，要么全部回滚，因此崩溃或报错不会留下“半播种”的领域，
    /// 也不会出现标记已写而数据缺失的情况。领域没有启用意图时拒绝激活，
    /// 与 [`Self::set_active_intent_domain`] 保持一致。
    pub fn seed_intent_domain_atomic(
        &self,
        domain: &IntentDomainRecord,
        intents: &[IntentRecord],
        marker_key: &str,
        marker_value: &str,
    ) -> Result<()> {
        if !intents.iter().any(|i| i.enabled) {
            return Err(AppError::InvalidInput(
                "该领域没有启用的意图，无法激活".to_string(),
            ));
        }

        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO intent_domains (id, name, description, is_active, source, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
            params![
                domain.id,
                domain.name,
                domain.description,
                domain.source,
                domain.version,
                domain.created_at,
                domain.updated_at,
            ],
        )?;

        for i in intents {
            tx.execute(
                "INSERT INTO intents (id, domain_id, name, description, examples, workflow,
                     allowed_tools, default_agent, scope, enabled, version, confidence_threshold,
                     fallback_intent, path, auto_escalate, system_prompt_appendix, model_id,
                     max_iterations, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, ?20, ?21)",
                params![
                    i.id,
                    i.domain_id,
                    i.name,
                    i.description,
                    serde_json::to_string(&i.examples)?,
                    i.workflow,
                    serde_json::to_string(&i.allowed_tools)?,
                    i.default_agent,
                    i.scope,
                    i.enabled as i32,
                    i.version,
                    i.confidence_threshold,
                    i.fallback_intent,
                    i.path,
                    i.auto_escalate as i32,
                    i.system_prompt_appendix,
                    i.model_id,
                    i.max_iterations.map(|v| v as i64),
                    i.sort_order,
                    i.created_at,
                    i.updated_at,
                ],
            )?;
        }

        tx.execute("UPDATE intent_domains SET is_active = 0", [])?;
        tx.execute(
            "UPDATE intent_domains SET is_active = 1 WHERE id = ?1",
            params![domain.id],
        )?;
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![marker_key, marker_value],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// 在一个事务内插入领域及其意图。领域一律以非活跃状态插入，
    /// 调用方需要时再显式激活。
    ///
    /// 整批要么全部落库，要么全部回滚，被拒绝的导入不会留下半写入的领域。
    pub fn insert_intent_domain_atomic(
        &self,
        domain: &IntentDomainRecord,
        intents: &[IntentRecord],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO intent_domains (id, name, description, is_active, source, version, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
            params![
                domain.id,
                domain.name,
                domain.description,
                domain.source,
                domain.version,
                domain.created_at,
                domain.updated_at,
            ],
        )?;

        for i in intents {
            tx.execute(
                "INSERT INTO intents (id, domain_id, name, description, examples, workflow,
                     allowed_tools, default_agent, scope, enabled, version, confidence_threshold,
                     fallback_intent, path, auto_escalate, system_prompt_appendix, model_id,
                     max_iterations, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, ?20, ?21)",
                params![
                    i.id,
                    i.domain_id,
                    i.name,
                    i.description,
                    serde_json::to_string(&i.examples)?,
                    i.workflow,
                    serde_json::to_string(&i.allowed_tools)?,
                    i.default_agent,
                    i.scope,
                    i.enabled as i32,
                    i.version,
                    i.confidence_threshold,
                    i.fallback_intent,
                    i.path,
                    i.auto_escalate as i32,
                    i.system_prompt_appendix,
                    i.model_id,
                    i.max_iterations.map(|v| v as i64),
                    i.sort_order,
                    i.created_at,
                    i.updated_at,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// 更新意图的可编辑字段。调用方传入的 `version` 会被忽略：
    /// 读取库中版本后 +1 写回。`domain_id` 不可变，且不允许停用领域内
    /// 最后一个启用的意图。
    pub fn update_intent(&self, i: &IntentRecord) -> Result<()> {
        let row: Option<(i64, String, i64)> = self
            .conn
            .query_row(
                "SELECT version, domain_id, enabled FROM intents WHERE id = ?1",
                params![i.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (old_version, domain_id, current_enabled) = match row {
            Some(v) => v,
            None => return Err(AppError::NotFound(format!("意图不存在: {}", i.id))),
        };

        if current_enabled != 0 && !i.enabled && self.count_enabled_intents(&domain_id)? <= 1 {
            return Err(AppError::InvalidInput(
                "不能停用领域内最后一个启用的意图".to_string(),
            ));
        }

        let changed = self.conn.execute(
            "UPDATE intents SET name = ?2, description = ?3, examples = ?4, workflow = ?5,
                 allowed_tools = ?6, default_agent = ?7, scope = ?8, enabled = ?9,
                 version = ?10, confidence_threshold = ?11, fallback_intent = ?12, path = ?13,
                 auto_escalate = ?14, system_prompt_appendix = ?15, model_id = ?16,
                 max_iterations = ?17, sort_order = ?18, updated_at = ?19
             WHERE id = ?1",
            params![
                i.id,
                i.name,
                i.description,
                serde_json::to_string(&i.examples)?,
                i.workflow,
                serde_json::to_string(&i.allowed_tools)?,
                i.default_agent,
                i.scope,
                i.enabled as i32,
                old_version + 1,
                i.confidence_threshold,
                i.fallback_intent,
                i.path,
                i.auto_escalate as i32,
                i.system_prompt_appendix,
                i.model_id,
                i.max_iterations.map(|v| v as i64),
                i.sort_order,
                i.updated_at,
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("意图不存在: {}", i.id)));
        }
        Ok(())
    }

    /// 删除意图；不允许删除领域内最后一个启用的意图。
    /// 同一事务内会清空指向它的 `fallback_intent` 引用。
    pub fn delete_intent(&self, id: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT domain_id, enabled FROM intents WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (domain_id, enabled) = match row {
            Some(v) => v,
            None => return Err(AppError::NotFound(format!("意图不存在: {id}"))),
        };

        if enabled != 0 && self.count_enabled_intents(&domain_id)? <= 1 {
            return Err(AppError::InvalidInput(
                "不能删除领域内最后一个启用的意图".to_string(),
            ));
        }

        // 在同一事务内清理入站引用：`fallback_intent` 存的是 id，
        // 悬空 id 会静默退化为 `clarify`，并在之后无关的编辑中触发命令层校验失败。
        tx.execute(
            "UPDATE intents SET fallback_intent = '' WHERE domain_id = ?1 AND fallback_intent = ?2",
            params![domain_id, id],
        )?;

        tx.execute("DELETE FROM intents WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// 启用或停用意图，同时递增 `version` 并写入 `updated_at`；
    /// 不允许停用领域内最后一个启用的意图。
    pub fn set_intent_enabled(&self, id: &str, enabled: bool, updated_at: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let row: Option<(String, i64, i64)> = tx
            .query_row(
                "SELECT domain_id, enabled, version FROM intents WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (domain_id, current_enabled, old_version) = match row {
            Some(v) => v,
            None => return Err(AppError::NotFound(format!("意图不存在: {id}"))),
        };

        if !enabled && current_enabled != 0 && self.count_enabled_intents(&domain_id)? <= 1 {
            return Err(AppError::InvalidInput(
                "不能停用领域内最后一个启用的意图".to_string(),
            ));
        }

        tx.execute(
            "UPDATE intents SET enabled = ?2, version = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, enabled as i32, old_version + 1, updated_at],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 统计领域内已启用的意图数量。
    pub fn count_enabled_intents(&self, domain_id: &str) -> Result<i64> {
        let count = self.conn.query_row(
            "SELECT COUNT(*) FROM intents WHERE domain_id = ?1 AND enabled = 1",
            params![domain_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    #[cfg(test)]
    /// 测试辅助：统计领域内意图总数，查询失败返回 -1。
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
    /// 测试辅助：统计某张表的列数，查询失败返回 -1。
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
    use crate::core::db::models::Session;

    /// 固定时间戳，确保测试校验的正是调用方传入的值。
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

    /// 直接插入一条最小意图行，其余列由默认值填充。
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
        // 只有一个停用意图：不得满足守卫条件。
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
    fn insert_domain_never_inserts_active() {
        let db = Database::new_test().unwrap();
        let mut domain = mk_domain("forced-active", "伪活跃领域");
        domain.is_active = true;

        db.insert_intent_domain(&domain).unwrap();

        let stored = db.get_intent_domain("forced-active").unwrap().unwrap();
        assert!(!stored.is_active, "insert_intent_domain 必须强制写入 is_active = 0");
        assert!(db.get_active_intent_domain().unwrap().is_none());
    }

    fn mk_intent(id: &str, domain_id: &str, name: &str) -> IntentRecord {
        IntentRecord {
            id: id.to_string(),
            domain_id: domain_id.to_string(),
            name: name.to_string(),
            description: "测试意图".to_string(),
            examples: vec!["示例一".to_string()],
            workflow: String::new(),
            allowed_tools: vec!["read_file".to_string()],
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: true,
            version: 1,
            confidence_threshold: 0.7,
            fallback_intent: String::new(),
            path: "fast".to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
            sort_order: 0,
            created_at: TEST_TS.to_string(),
            updated_at: TEST_TS.to_string(),
        }
    }

    #[test]
    fn disable_succeeds_when_another_enabled_intent_remains() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("guard-domain", "守卫领域"))
            .unwrap();
        db.insert_intent(&mk_intent("intent-a", "guard-domain", "意图甲"))
            .unwrap();
        db.insert_intent(&mk_intent("intent-b", "guard-domain", "意图乙"))
            .unwrap();

        db.set_intent_enabled("intent-a", false, TEST_TS).unwrap();

        let stored = db.get_intent("intent-a").unwrap().unwrap();
        assert!(!stored.enabled, "停用后库里必须确实变为停用");
        assert_eq!(stored.version, 2, "set_intent_enabled 必须递增版本");
        assert_eq!(stored.updated_at, TEST_TS);
        assert_eq!(db.count_enabled_intents("guard-domain").unwrap(), 1);
        assert_eq!(
            db.list_enabled_intents("guard-domain")
                .unwrap()
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            vec!["intent-b"]
        );
    }

    #[test]
    fn deleting_intent_clears_inbound_fallback_references() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("ref-domain", "引用领域"))
            .unwrap();

        let target = mk_intent("target-intent", "ref-domain", "目标意图");
        let mut referencing = mk_intent("referencing-intent", "ref-domain", "引用意图");
        referencing.fallback_intent = "target-intent".to_string();
        let mut unrelated = mk_intent("unrelated-intent", "ref-domain", "无关意图");
        unrelated.fallback_intent = "referencing-intent".to_string();

        db.insert_intent(&target).unwrap();
        db.insert_intent(&referencing).unwrap();
        db.insert_intent(&unrelated).unwrap();

        db.delete_intent("target-intent").unwrap();

        assert!(db.get_intent("target-intent").unwrap().is_none());
        assert_eq!(
            db.get_intent("referencing-intent")
                .unwrap()
                .unwrap()
                .fallback_intent,
            "",
            "指向被删意图的 fallback 必须被清空"
        );
        assert_eq!(
            db.get_intent("unrelated-intent")
                .unwrap()
                .unwrap()
                .fallback_intent,
            "referencing-intent",
            "不指向被删意图的行必须保持不变"
        );
    }

    #[test]
    fn deleting_unreferenced_intent_leaves_other_rows_unchanged() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("keep-domain", "保留领域"))
            .unwrap();

        let spare = mk_intent("spare-intent", "keep-domain", "备用意图");
        let keeper = mk_intent("keeper-intent", "keep-domain", "保留意图");
        let mut holder = mk_intent("holder-intent", "keep-domain", "持有意图");
        holder.fallback_intent = "keeper-intent".to_string();

        db.insert_intent(&spare).unwrap();
        db.insert_intent(&keeper).unwrap();
        db.insert_intent(&holder).unwrap();

        db.delete_intent("spare-intent").unwrap();

        let stored_holder = db.get_intent("holder-intent").unwrap().unwrap();
        assert_eq!(
            stored_holder.fallback_intent, "keeper-intent",
            "删除无人引用的意图不得清空其他行的 fallback"
        );
        assert_eq!(stored_holder.version, 1, "无关行不得被更新");
        assert_eq!(stored_holder.updated_at, TEST_TS, "无关行不得被更新");
        assert!(db
            .get_intent("keeper-intent")
            .unwrap()
            .unwrap()
            .fallback_intent
            .is_empty());
    }

    #[test]
    fn delete_succeeds_when_another_enabled_intent_remains() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("guard-domain", "守卫领域"))
            .unwrap();
        db.insert_intent(&mk_intent("intent-a", "guard-domain", "意图甲"))
            .unwrap();
        db.insert_intent(&mk_intent("intent-b", "guard-domain", "意图乙"))
            .unwrap();

        db.delete_intent("intent-a").unwrap();

        assert!(db.get_intent("intent-a").unwrap().is_none(), "被删除的行必须消失");
        assert_eq!(db.count_enabled_intents("guard-domain").unwrap(), 1);
    }

    #[test]
    fn list_enabled_intents_filters_and_orders() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("list-domain", "列表领域"))
            .unwrap();

        let mut first = mk_intent("intent-10", "list-domain", "意图十");
        first.sort_order = 10;
        let mut second = mk_intent("intent-20", "list-domain", "意图二十");
        second.sort_order = 20;
        second.enabled = false;
        let mut third = mk_intent("intent-30", "list-domain", "意图三十");
        third.sort_order = 30;

        db.insert_intent(&first).unwrap();
        db.insert_intent(&second).unwrap();
        db.insert_intent(&third).unwrap();

        let enabled = db.list_enabled_intents("list-domain").unwrap();
        assert_eq!(enabled.len(), 2, "停用的意图必须被过滤掉");
        assert_eq!(
            enabled.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["intent-10", "intent-30"],
            "启用意图必须按 sort_order 升序"
        );

        assert_eq!(
            db.list_intents("list-domain")
                .unwrap()
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            vec!["intent-10", "intent-20", "intent-30"],
            "list_intents 必须按 sort_order 升序"
        );
    }

    #[test]
    fn disabling_last_enabled_intent_is_rejected() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("guard-domain", "守卫领域"))
            .unwrap();
        insert_enabled_intent(&db, "only-intent", "guard-domain", true);

        let err = db.set_intent_enabled("only-intent", false, TEST_TS).unwrap_err();
        match err {
            AppError::InvalidInput(msg) => assert_eq!(msg, "不能停用领域内最后一个启用的意图"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        assert_eq!(db.count_enabled_intents("guard-domain").unwrap(), 1);
        assert!(db.get_intent("only-intent").unwrap().unwrap().enabled);
    }

    #[test]
    fn deleting_last_enabled_intent_is_rejected() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("guard-domain", "守卫领域"))
            .unwrap();
        insert_enabled_intent(&db, "only-intent", "guard-domain", true);

        let err = db.delete_intent("only-intent").unwrap_err();
        match err {
            AppError::InvalidInput(msg) => assert_eq!(msg, "不能删除领域内最后一个启用的意图"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        assert!(db.get_intent("only-intent").unwrap().is_some());
        assert_eq!(db.count_enabled_intents("guard-domain").unwrap(), 1);
    }

    #[test]
    fn update_cannot_disable_last_enabled_intent() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("guard-domain", "守卫领域"))
            .unwrap();
        let intent = mk_intent("only-intent", "guard-domain", "唯一意图");
        db.insert_intent(&intent).unwrap();

        let mut disabled = intent.clone();
        disabled.enabled = false;

        let err = db.update_intent(&disabled).unwrap_err();
        match err {
            AppError::InvalidInput(msg) => assert_eq!(msg, "不能停用领域内最后一个启用的意图"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        let stored = db.get_intent("only-intent").unwrap().unwrap();
        assert!(stored.enabled, "守卫失败后该意图必须仍然启用");
        assert_eq!(stored.version, 1, "被拒绝的更新不得改动版本");
    }

    #[test]
    fn update_intent_increments_version() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("guard-domain", "守卫领域"))
            .unwrap();

        let mut intent = mk_intent("versioned-intent", "guard-domain", "版本意图");
        db.insert_intent(&intent).unwrap();
        assert_eq!(db.get_intent("versioned-intent").unwrap().unwrap().version, 1);

        intent.name = "改名意图".to_string();
        intent.version = 99; // 调用方传入的版本必须被忽略
        db.update_intent(&intent).unwrap();

        let stored = db.get_intent("versioned-intent").unwrap().unwrap();
        assert_eq!(stored.version, 2, "update_intent 必须写入旧版本 + 1");
        assert_eq!(stored.name, "改名意图");
        assert_eq!(stored.examples, vec!["示例一".to_string()]);
        assert_eq!(stored.allowed_tools, vec!["read_file".to_string()]);
    }

    #[test]
    fn malformed_json_column_reads_as_empty_vec() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("json-domain", "JSON 领域"))
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO intents
                     (id, domain_id, name, examples, allowed_tools, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'not-json', 'not-json', ?4, ?5)",
                rusqlite::params!["broken-intent", "json-domain", "坏数据意图", TEST_TS, TEST_TS],
            )
            .unwrap();

        let intents = db.list_intents("json-domain").unwrap();
        assert_eq!(intents.len(), 1);
        assert!(intents[0].examples.is_empty());
        assert!(intents[0].allowed_tools.is_empty());
    }

    #[test]
    fn insert_intent_domain_atomic_inserts_inactive_domain_with_intents() {
        let db = Database::new_test().unwrap();
        let mut domain = mk_domain("imported-domain", "导入领域");
        domain.is_active = true; // 必须被忽略：导入流程从不激活领域
        let intents = vec![mk_intent("imported-intent", "imported-domain", "导入意图")];

        db.insert_intent_domain_atomic(&domain, &intents).unwrap();

        let stored = db.get_intent_domain("imported-domain").unwrap().unwrap();
        assert!(!stored.is_active, "导入的领域必须是非活跃的");
        assert!(db.get_active_intent_domain().unwrap().is_none());
        assert_eq!(db.count_intents("imported-domain"), 1);
        assert_eq!(
            db.get_intent("imported-intent").unwrap().unwrap().name,
            "导入意图"
        );
    }

    #[test]
    fn insert_intent_domain_atomic_leaves_nothing_on_failure() {
        let db = Database::new_test().unwrap();
        let domain = mk_domain("rollback-domain", "回滚领域");
        // UNIQUE(domain_id, name) 会让第二行在事务中途被拒绝。
        let intents = vec![
            mk_intent("rollback-a", "rollback-domain", "重复意图"),
            mk_intent("rollback-b", "rollback-domain", "重复意图"),
        ];

        let err = db.insert_intent_domain_atomic(&domain, &intents).unwrap_err();
        assert!(
            matches!(err, AppError::Database(_)),
            "重复名称必须以数据库错误返回，实际：{err:?}"
        );

        assert!(db.get_intent_domain("rollback-domain").unwrap().is_none());
        assert_eq!(db.count_intents("rollback-domain"), 0);
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

    fn mk_session(id: &str, domain_id: Option<&str>, mode: &str) -> Session {
        Session {
            id: id.to_string(),
            title: id.to_string(),
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
            intent_domain_id: domain_id.map(str::to_string),
            intent_domain_mode: mode.to_string(),
        }
    }

    #[test]
    fn deleting_domain_clears_session_references_and_resets_mode() {
        let db = Database::new_test().unwrap();
        db.insert_intent_domain(&mk_domain("doomed-domain", "待删领域"))
            .unwrap();
        db.insert_intent_domain(&mk_domain("kept-domain", "保留领域"))
            .unwrap();
        insert_enabled_intent(&db, "doomed-intent", "doomed-domain", true);
        insert_enabled_intent(&db, "kept-intent", "kept-domain", true);

        db.create_session(&mk_session("pinned-session", Some("doomed-domain"), "pinned"))
            .unwrap();
        db.create_session(&mk_session("auto-session", Some("doomed-domain"), "auto"))
            .unwrap();
        db.create_session(&mk_session("kept-session", Some("kept-domain"), "pinned"))
            .unwrap();

        db.delete_intent_domain("doomed-domain").unwrap();

        for id in ["pinned-session", "auto-session"] {
            let session = db.get_session(id).unwrap().unwrap();
            assert!(
                session.intent_domain_id.is_none(),
                "引用被删除领域的会话必须置 NULL"
            );
            assert_eq!(
                session.intent_domain_mode, "auto",
                "引用被删除领域的会话模式必须回落 auto"
            );
        }

        let kept = db.get_session("kept-session").unwrap().unwrap();
        assert_eq!(
            kept.intent_domain_id.as_deref(),
            Some("kept-domain"),
            "未引用被删领域的会话不得被改动"
        );
        assert_eq!(kept.intent_domain_mode, "pinned");
    }
}
