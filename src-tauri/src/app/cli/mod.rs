// ── CLI entry point ──
//
// `agent` without a subcommand starts the desktop app. Local toolchain
// management is a separate concern: `agent toolchain ...` (with `runtime` as a
// compatibility alias) manages Node/Python/... on this machine. The command
// handlers live in `cli::toolchain` and share `RuntimeServices` with the Tauri
// commands.

pub mod toolchain;

use clap::{Parser, Subcommand};

use crate::core::error::Result;

#[derive(Debug, Parser)]
#[command(
    name = "agent",
    version,
    about = "Agent — desktop AI client",
    long_about = "Run without a subcommand to start the desktop app.\n\
                  Use `agent toolchain` to manage local language runtimes from a terminal."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 管理本机语言运行时（检测、安装、切换、项目绑定）
    #[command(visible_alias = "runtime")]
    Toolchain {
        #[command(subcommand)]
        action: toolchain::ToolchainAction,
    },
}

/// Dispatch a parsed CLI. Without a subcommand this launches the desktop app.
pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Toolchain { action }) => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(toolchain::execute(action))
        }
        None => {
            crate::run();
            Ok(())
        }
    }
}
