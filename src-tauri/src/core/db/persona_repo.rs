//! 人格（personas）及其与记忆、项目绑定关系的读写。

use rusqlite::{params, OptionalExtension};

use crate::core::db::models::PersonaRecord;
use crate::core::db::repository::Database;
use crate::core::error::Result;

impl Database {
    /// 把 SQL 行映射为 [`PersonaRecord`]；所有查询必须使用同一列顺序。
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

    // ── 人格 CRUD ──

    /// 插入人格记录。`name` 唯一，重复会返回数据库错误；不会清理其他默认标记。
    pub fn insert_persona(&self, p: &PersonaRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO personas (id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![p.id, p.name, p.title, p.emoji, p.description, p.system_prompt, p.temperature, p.response_style, p.model_provider, p.model_name, p.is_default, p.created_at, p.updated_at],
        )?;
        Ok(())
    }

    /// 列出全部人格，按 `name` 升序。
    pub fn list_personas(&self) -> Result<Vec<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas ORDER BY name ASC"
        )?;
        let personas = stmt.query_map([], Self::map_persona_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(personas)
    }

    /// 按 id 取人格；不存在返回 `None`。
    pub fn get_persona(&self, id: &str) -> Result<Option<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas WHERE id = ?1"
        )?;
        let p = stmt.query_row(params![id], Self::map_persona_row).optional()?;
        Ok(p)
    }

    /// 按唯一名称取人格；不存在返回 `None`。
    pub fn get_persona_by_name(&self, name: &str) -> Result<Option<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas WHERE name = ?1"
        )?;
        let p = stmt.query_row(params![name], Self::map_persona_row).optional()?;
        Ok(p)
    }

    /// 取默认人格；没有默认项时返回 `None`。
    pub fn get_default_persona(&self) -> Result<Option<PersonaRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, title, emoji, description, system_prompt, temperature, response_style, model_provider, model_name, is_default, created_at, updated_at
             FROM personas WHERE is_default = 1 LIMIT 1"
        )?;
        let p = stmt.query_row([], Self::map_persona_row).optional()?;
        Ok(p)
    }

    /// 覆盖人格的可编辑字段（含 `is_default`），不修改 `created_at`。
    pub fn update_persona(&self, p: &PersonaRecord) -> Result<()> {
        self.conn.execute(
            "UPDATE personas SET name=?2, title=?3, emoji=?4, description=?5, system_prompt=?6, temperature=?7, response_style=?8, model_provider=?9, model_name=?10, is_default=?11, updated_at=?12
             WHERE id=?1",
            params![p.id, p.name, p.title, p.emoji, p.description, p.system_prompt, p.temperature, p.response_style, p.model_provider, p.model_name, p.is_default, p.updated_at],
        )?;
        Ok(())
    }

    /// 删除人格。
    ///
    /// `persona_memories` / `persona_projects` 依赖外键级联，但本项目默认关闭外键，
    /// 因此关联行需要调用方自行清理。
    pub fn delete_persona(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM personas WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 清空所有默认标记，使系统回到“没有默认人格”的状态。
    pub fn clear_persona_default(&self) -> Result<()> {
        self.conn.execute("UPDATE personas SET is_default = 0 WHERE is_default = 1", [])?;
        Ok(())
    }

    // ── 人格与记忆的关联 ──

    /// 建立人格与记忆的关联；已存在时静默忽略（幂等）。
    pub fn link_memory_to_persona(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO persona_memories (persona_id, memory_id) VALUES (?1, ?2)",
            params![persona_id, memory_id],
        )?;
        Ok(())
    }

    /// 解除人格与记忆的关联；不存在时静默成功。
    pub fn unlink_memory_from_persona(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM persona_memories WHERE persona_id = ?1 AND memory_id = ?2",
            params![persona_id, memory_id],
        )?;
        Ok(())
    }

    /// 取人格关联的全部记忆 id，**无序**。
    pub fn get_persona_memory_ids(&self, persona_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT memory_id FROM persona_memories WHERE persona_id = ?1"
        )?;
        let ids = stmt.query_map(params![persona_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    // ── 人格与项目的绑定 ──

    /// 建立人格与项目路径的绑定；同一 (`persona_id`, `project_path`) 会被覆盖（含 `auto_select`）。
    pub fn bind_persona_to_project(&self, persona_id: &str, project_path: &str, auto_select: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO persona_projects (persona_id, project_path, auto_select) VALUES (?1, ?2, ?3)",
            params![persona_id, project_path, auto_select],
        )?;
        Ok(())
    }

    /// 解除人格与项目路径的绑定；不存在时静默成功。
    pub fn unbind_persona_from_project(&self, persona_id: &str, project_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM persona_projects WHERE persona_id = ?1 AND project_path = ?2",
            params![persona_id, project_path],
        )?;
        Ok(())
    }

    /// 取绑定到某项目路径的人格：`auto_select` 为真者优先，其余按 `name` 升序。
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
}
