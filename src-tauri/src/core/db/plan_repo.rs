//! 执行计划（execution plans）与计划步骤的读写。

use rusqlite::{params, OptionalExtension};

use crate::core::db::models::{ExecutionPlanRecord, PlanStepRecord};
use crate::core::db::repository::Database;
use crate::core::error::Result;

impl Database {
    // ── 执行计划 CRUD ──

    /// 插入执行计划记录；`id` 冲突会返回数据库错误。
    pub fn insert_execution_plan(&self, plan: &ExecutionPlanRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO execution_plans (id, session_id, source, goal, plan_json, status, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![plan.id, plan.session_id, plan.source, plan.goal, plan.plan_json, plan.status, plan.created_at, plan.finished_at],
        )?;
        Ok(())
    }

    /// 更新计划状态与完成时间；`finished_at` 为 `None` 表示尚未完成（写入 NULL）。
    pub fn update_execution_plan_status(&self, id: &str, status: &str, finished_at: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE execution_plans SET status = ?2, finished_at = ?3 WHERE id = ?1",
            params![id, status, finished_at],
        )?;
        Ok(())
    }

    /// 按 id 取计划；不存在返回 `None`。
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

    /// 取某会话的全部计划，按 `created_at` 倒序。
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

    /// 以 `id` 为键写入或更新步骤。
    ///
    /// 更新时只覆盖 `status`、`result_json`、`error`、`duration_ms`，
    /// 步骤序号、标签与类型保持不变。
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

    /// 取计划的全部步骤，按 `step_index` 升序（即执行顺序）。
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
