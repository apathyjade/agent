//! 编排层：把目标转成计划、按步骤执行，并驱动子任务 DAG。
//!
//! 子模块职责：
//! - `planner`：自然语言目标 → `ExecutionPlan`；
//! - `runtime`：执行计划、落库状态、上报进度；
//! - `agent` / `task_graph` / `dispatcher`：DAG 编排与 worker 分发；
//! - `pipeline_adapter`：YAML 工作流 → 计划。

pub mod agent;
pub mod dispatcher;
pub mod event_bridge;
pub mod pipeline_adapter;
pub mod plan_error;
pub mod plan_types;
pub mod planner;
pub mod runtime;
pub mod task_graph;

pub use plan_types::*;
