//! 数据库层：连接与表结构（`repository`）、领域模型（`models`），
//! 以及按聚合拆分的各仓储实现（`session_repo`、`memory_repo`、`persona_repo`、
//! `plan_repo`、`intent_repo`）。
//!
//! 拆分只影响文件组织：所有方法都是 `impl Database` 的固有方法，
//! 外部调用路径保持不变。

pub mod intent_repo;
pub mod memory_repo;
pub mod models;
pub mod persona_repo;
pub mod plan_repo;
pub mod repository;
pub mod session_repo;
