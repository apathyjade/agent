//! Tauri 应用入口。
//!
//! 本模块只负责两件事：装配 [`AppState`] 并启动桌面运行时，以及把全部
//! `#[tauri::command]` 注册进 `invoke_handler`。业务逻辑一律下沉到
//! `core` / `runtime` / `llm` / `agent` / `features` 层，这里不做决策。
//!
//! 启动分两个阶段：同步阶段构造状态（失败则应用直接退出），异步阶段执行
//! 可失败的初始化（失败只记日志，不阻塞窗口出现）。

#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod core;
pub mod agent;
pub mod llm;
pub mod runtime;
pub mod features;
pub mod app;