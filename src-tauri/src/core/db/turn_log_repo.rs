//! 会话回合日志（`session_turn_logs`）的读写。
//!
//! 条目以 JSON 整体存在 `entries_json`：写入发生在回合结束时，查询按需
//! 反序列化，不做条目级查询。方法沿用 `impl Database` 的 `&self` 模式。

use rusqlite::{params, OptionalExtension};

use crate::core::db::models::{TurnLogEntry, TurnLogRecord, TurnLogSummary};
use crate::core::db::repository::Database;
use crate::core::error::Result;

impl Database {
    /// 写入（或覆盖）一条回合日志。
    ///
    /// 同一 `turn_id` 重复写入是幂等的：正常流程中 `finish` 只会调用一次，
    /// 这里用 `INSERT OR REPLACE` 兜住极端重试。
    pub fn insert_turn_log(&self, record: &TurnLogRecord) -> Result<()> {
        let entries_json = serde_json::to_string(&record.entries)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO session_turn_logs (
                turn_id, session_id, status, path, question, model_id, persona_name,
                started_at, finished_at, total_ms, entry_count, entries_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                record.turn_id,
                record.session_id,
                record.status,
                record.path,
                record.question,
                record.model_id,
                record.persona_name,
                record.started_at,
                record.finished_at,
                record.total_ms,
                record.entry_count,
                entries_json,
            ],
        )?;
        Ok(())
    }

    /// 列出某会话的回合摘要，按开始时间倒序（最新在前）。
    pub fn list_turn_logs(&self, session_id: &str) -> Result<Vec<TurnLogSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT turn_id, status, path, question, model_id, persona_name,
                    started_at, total_ms, entry_count
             FROM session_turn_logs
             WHERE session_id = ?1
             ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(TurnLogSummary {
                turn_id: row.get(0)?,
                status: row.get(1)?,
                path: row.get(2)?,
                question: row.get(3)?,
                model_id: row.get(4)?,
                persona_name: row.get(5)?,
                started_at: row.get(6)?,
                total_ms: row.get(7)?,
                entry_count: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 读取单个回合的完整日志；不存在时返回 `None`。
    pub fn get_turn_log(&self, turn_id: &str) -> Result<Option<TurnLogRecord>> {
        let record = self
            .conn
            .query_row(
                "SELECT turn_id, session_id, status, path, question, model_id, persona_name,
                        started_at, finished_at, total_ms, entry_count, entries_json
                 FROM session_turn_logs WHERE turn_id = ?1",
                params![turn_id],
                |row| {
                    let entries_json: String = row.get(11)?;
                    let entries: Vec<TurnLogEntry> =
                        serde_json::from_str(&entries_json).unwrap_or_default();
                    Ok(TurnLogRecord {
                        turn_id: row.get(0)?,
                        session_id: row.get(1)?,
                        status: row.get(2)?,
                        path: row.get(3)?,
                        question: row.get(4)?,
                        model_id: row.get(5)?,
                        persona_name: row.get(6)?,
                        started_at: row.get(7)?,
                        finished_at: row.get(8)?,
                        total_ms: row.get(9)?,
                        entry_count: row.get(10)?,
                        entries,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(turn_id: &str, session_id: &str, started_at: &str) -> TurnLogRecord {
        TurnLogRecord {
            turn_id: turn_id.to_string(),
            session_id: session_id.to_string(),
            status: "completed".to_string(),
            path: "fast".to_string(),
            question: "你好".to_string(),
            model_id: Some("model-a".to_string()),
            persona_name: None,
            started_at: started_at.to_string(),
            finished_at: Some(started_at.to_string()),
            total_ms: Some(120),
            entry_count: 1,
            entries: vec![TurnLogEntry {
                seq: 1,
                at: started_at.to_string(),
                stage: "answer".to_string(),
                level: "info".to_string(),
                title: "输出回答".to_string(),
                summary: "1,234 字".to_string(),
                elapsed_ms: Some(120),
                detail: Some(serde_json::json!({ "preview": "hello" })),
            }],
        }
    }

    #[test]
    fn round_trip_preserves_entries() {
        let db = Database::new_test().unwrap();
        db.insert_turn_log(&record("t1", "s1", "2026-01-01T00:00:00Z"))
            .unwrap();

        let got = db.get_turn_log("t1").unwrap().expect("must exist");
        assert_eq!(got.session_id, "s1");
        assert_eq!(got.entries.len(), 1);
        assert_eq!(got.entries[0].title, "输出回答");
        assert_eq!(got.entry_count, 1);
    }

    #[test]
    fn list_orders_by_started_at_desc() {
        let db = Database::new_test().unwrap();
        db.insert_turn_log(&record("old", "s1", "2026-01-01T00:00:00Z"))
            .unwrap();
        db.insert_turn_log(&record("new", "s1", "2026-02-01T00:00:00Z"))
            .unwrap();
        db.insert_turn_log(&record("other", "s2", "2026-03-01T00:00:00Z"))
            .unwrap();

        let logs = db.list_turn_logs("s1").unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].turn_id, "new");
        assert_eq!(logs[1].turn_id, "old");
    }

    #[test]
    fn unknown_turn_returns_none() {
        let db = Database::new_test().unwrap();
        assert!(db.get_turn_log("missing").unwrap().is_none());
    }

    #[test]
    fn delete_session_cascades_turn_logs() {
        let db = Database::new_test().unwrap();
        db.insert_turn_log(&record("t1", "s1", "2026-01-01T00:00:00Z"))
            .unwrap();
        db.delete_session("s1").unwrap();
        assert!(db.list_turn_logs("s1").unwrap().is_empty());
    }
}
