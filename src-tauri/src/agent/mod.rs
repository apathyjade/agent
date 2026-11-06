//! agent 层入口：工具循环、编排器、工具实现、worker 与 critic。
//!
//! 分层约束：agent 可以依赖 `core` 与 `llm`，但不得反向被它们依赖。

pub mod critic;
pub mod fast_loop;
pub mod orchestrator;
pub mod tools;
pub mod workers;
