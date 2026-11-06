//! 记忆（memories）的读写，以及基于 FTS5 的全文检索。
//!
//! FTS 索引由 `memories` 表上的触发器同步维护，这里只负责查询与普通 CRUD。

use rusqlite::{params, OptionalExtension};

use crate::core::db::models::MemoryRecord;
use crate::core::db::repository::Database;
use crate::core::error::Result;

impl Database {
    // ── 记忆 CRUD ──

    /// 插入一条记忆；FTS 索引由表上的触发器同步维护，无需手工写入。
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

    /// 按 id 取记忆；不存在返回 `None`。
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

    /// 列出全部记忆，先按 `relevance` 倒序，再按 `last_accessed_at` 倒序。
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

    /// 清洗用户输入，使其可安全用于 FTS5 的 `MATCH` 查询。
    /// 去掉运算符字符与保留关键字以避免语法错误，词项之间用 `AND` 连接。
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
                // 过滤 FTS5 保留关键字（不区分大小写）
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

    /// 按 FTS5 全文匹配搜索记忆（索引列为 `content` 与 `tags`）。
    /// 按 FTS5 全文匹配搜索记忆（索引列为 `content` 与 `tags`）。
    ///
    /// - `memory_type` / `scope` 为 `Some` 时追加过滤；`scope` 按前缀匹配。
    /// - 查询串清洗后为空时退化为“列出全部记忆”，仍应用上述过滤。
    /// - 结果按 `relevance` 倒序、`last_accessed_at` 倒序。
    pub fn search_memories(&self, query: &str, memory_type: Option<&str>, scope: Option<&str>) -> Result<Vec<MemoryRecord>> {
        let fts_query = Self::sanitize_fts_query(query);

        if fts_query.is_empty() {
            // 退化路径：不做全文过滤，返回全部记忆（仍应用类型/范围过滤）
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

        // FTS5 全文检索：content 与 tags 两列已建立索引
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

    /// 更新记忆的可编辑字段；不刷新 `last_accessed_at`，也不改动 `access_count`。
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

    /// 删除记忆；FTS 索引由触发器同步清理，记录不存在时静默成功。
    pub fn delete_memory(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 记录一次访问：把 `last_accessed_at` 置为当前时间，并让 `access_count` 自增 1。
    pub fn touch_memory(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE memories SET last_accessed_at = datetime('now'), access_count = access_count + 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// 用 FTS5 全文检索挑出与上下文相关的记忆，供提示词注入使用。
    /// 整个上下文清洗后作为单条 FTS5 `MATCH` 查询传入。
    /// 用 FTS5 检索与上下文相关的记忆，供提示词注入使用。
    ///
    /// 只匹配 `scope = 'global'` 的记忆；上下文为空或清洗后无有效词项时返回空列表。
    /// 结果按 `relevance`、`access_count`、`last_accessed_at` 依次倒序，最多 `limit` 条。
    pub fn retrieve_relevant(&self, context: &str, limit: i64) -> Result<Vec<MemoryRecord>> {
        if context.trim().is_empty() {
            return Ok(vec![]);
        }

        let fts_query = Self::sanitize_fts_query(context);
        if fts_query.is_empty() {
            return Ok(vec![]);
        }

        let sql = "SELECT m.id, m.content, m.memory_type, m.scope, m.source, m.relevance, m.tags, m.created_at, m.updated_at, m.last_accessed_at, m.access_count
             FROM memories m
             JOIN memories_fts fts ON m.rowid = fts.rowid
             WHERE memories_fts MATCH ?1
               AND m.scope = 'global'
             ORDER BY m.relevance DESC, m.access_count DESC, m.last_accessed_at DESC
             LIMIT ?2".to_string();

        let mut stmt = self.conn.prepare(&sql)?;
        let memories = stmt.query_map(params![fts_query, limit], Self::map_memory_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(memories)
    }

    /// 把 SQL 行映射为 [`MemoryRecord`]（搜索与检索共用，列顺序必须一致）。
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
}
