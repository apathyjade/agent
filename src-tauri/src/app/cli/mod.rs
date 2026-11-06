//! CLI 入口。
//!
//! 不带子命令的 `agent` 启动桌面应用。本机工具链管理是另一件事：
//! `agent toolchain ...`（兼容别名 `runtime`）用于管理本机的 Node/Python
//! 等运行时。命令处理在 `cli::toolchain`，与 Tauri 命令共享同一个
//! `RuntimeServices`。

pub mod toolchain;

use clap::{Parser, Subcommand};

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
