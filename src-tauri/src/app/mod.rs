//! 应用装配层：把 `core` / `runtime` / `llm` / `agent` / `features` 组合成
//! Tauri 能直接使用的形态。
//!
//! 这一层只做「接线」和 IPC 适配，不实现业务规则；具体能力都在下层 crate
//! 模块里。`cli` 与 `commands` 是同一套 `RuntimeServices` 的两个入口。

pub mod cli;
pub mod commands;
pub mod config;
pub mod state;

use crate::core::error::Result;
use cli::{Cli, Command, toolchain};

/// 分发解析好的 CLI。没有子命令时启动桌面应用。
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Toolchain { action }) => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(toolchain::execute(action))
        }
        None => {
            // setup();
            Ok(())
        }
    }
}
// fn main() {
//     let cli: Cli = Cli::parse();

//     if let Err(error) = agent_lib::app::run(cli) {
//         eprintln!("错误: {}", error);
//         std::process::exit(1);
//     }
// }
