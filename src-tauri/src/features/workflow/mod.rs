//! 工作流：YAML 定义的步骤编排。
//!
//! `models` 是序列化结构，`scanner` 从用户目录发现定义文件，`engine` 负责
//! 顺序执行、重试与进度上报。

pub mod engine;
pub mod models;
pub mod scanner;

pub use engine::PipelineEngine;
pub use models::{StepDef, WorkflowDef, WorkflowInfo, WorkflowRunRecord};
pub use scanner::{list_workflows, scan_workflow_files};
