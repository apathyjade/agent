#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use agent_lib::environment::cli::{Cli, AgentCommand, RuntimeCli};

fn main() {
    let cli = <Cli as clap::Parser>::parse_from(std::env::args());

    match &cli.command {
        Some(AgentCommand::Runtime { action }) => {
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| {
                eprintln!("Failed to create async runtime: {}", e);
                std::process::exit(1);
            });
            let app = rt.block_on(RuntimeCli::new()).unwrap_or_else(|e| {
                eprintln!("初始化失败: {}", e);
                std::process::exit(1);
            });
            rt.block_on(app.run(action)).unwrap_or_else(|e| {
                eprintln!("错误: {}", e);
                std::process::exit(1);
            });
        }
        None => {
            agent_lib::run();
        }
    }
}
