//! 业务功能模块。
//!
//! 每个子模块封装一个面向用户的能力域：意图管理、MCP 接入、记忆、人格、
//! 会话生命周期、技能、工作区与工作流。上层 `app` 通过 IPC 命令调用它们，
//! 本层不直接依赖 Tauri。

pub mod intent_management;
pub mod mcp;
pub mod memory;
pub mod persona;
pub mod session;
pub mod skills;
pub mod workspace;
pub mod workflow;
