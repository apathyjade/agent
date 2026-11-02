#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//
// Note: the `windows_subsystem` attribute hides the console in release builds.
// Toolchain CLI output therefore requires a debug build (`cargo run -- toolchain status`).

use agent_lib::cli::Cli;
use clap::Parser;

fn main() {
    let cli: Cli = Cli::parse();

    if let Err(error) = agent_lib::cli::run(cli) {
        eprintln!("错误: {}", error);
        std::process::exit(1);
    }
}
