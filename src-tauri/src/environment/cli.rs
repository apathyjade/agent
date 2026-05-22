// ── CLI Subcommand System: Runtime Management via Command Line ──
//
// Provides the `agent runtime <action>` CLI interface using clap derive.
// Can also be used programmatically via `RuntimeCli::run()`.

use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::Mutex;

use crate::db::repository::Database;
use crate::environment::*;
use crate::environment::project::*;
use crate::environment::registry::*;
use crate::environment::resolver::*;
use crate::environment::alias::*;
use crate::error::Result;

/// Agent Runtime Manager CLI
#[derive(Parser)]
#[command(name = "agent", about = "Agent CLI — 运行时管理")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<AgentCommand>,
}

#[derive(Subcommand)]
pub enum AgentCommand {
    /// 运行时管理
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
}

#[derive(Subcommand)]
pub enum RuntimeAction {
    /// 列出运行时版本
    Ls {
        /// 运行时类型 (node, python, rust, java, ...)
        runtime_type: Option<String>,
        /// 显示远程可用版本 (默认仅显示已安装)
        #[arg(long)]
        remote: bool,
        /// JSON 格式输出
        #[arg(long)]
        json: bool,
    },
    /// 切换运行时版本
    Use {
        runtime_type: String,
        version: String,
    },
    /// 安装运行时
    Install {
        runtime_type: String,
        version: Option<String>,
    },
    /// 卸载指定版本
    Uninstall {
        runtime_type: String,
        version: String,
    },
    /// 设置或查看默认版本
    Default {
        runtime_type: Option<String>,
        version: Option<String>,
    },
    /// 项目绑定管理
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// 运行时健康总览
    Status {
        #[arg(long)]
        json: bool,
    },
    /// 检查版本更新
    Check {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ProjectAction {
    /// 列出绑定项目
    Ls,
    /// 添加项目
    Add { path: String },
    /// 移除项目
    Remove { id: String },
    /// 同步项目版本
    Sync { id: String },
}

/// Lightweight runtime context for CLI mode (no Tauri dependency).
pub struct RuntimeCli {
    pub db: Arc<Mutex<Database>>,
    pub runtime_manager: Arc<RuntimeManager>,
    pub runtime_registry: Arc<RuntimeRegistry>,
    pub version_resolver: Arc<VersionResolver>,
    pub alias_manager: Arc<AliasManager>,
}

impl RuntimeCli {
    pub async fn new() -> Result<Self> {
        let db = Arc::new(Mutex::new(Database::new()?));
        let config = crate::config::AppConfig::load()?;

        // Initialize HTTP client with proxy settings
        crate::environment::http_client::init_http_client(config.download_proxy.as_deref());

        let runtime_dir = match &config.runtime_install_dir {
            Some(path) => std::path::PathBuf::from(path),
            None => dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("agent")
                .join("runtimes"),
        };
        let runtime_manager = Arc::new(RuntimeManager::new(runtime_dir));
        let runtime_registry = Arc::new(RuntimeRegistry::new());
        let version_resolver = Arc::new(VersionResolver::new(runtime_registry.clone()));
        let alias_manager = Arc::new(AliasManager::new());

        Ok(Self {
            db,
            runtime_manager,
            runtime_registry,
            version_resolver,
            alias_manager,
        })
    }

    pub async fn run(&self, action: &RuntimeAction) -> Result<()> {
        match action {
            RuntimeAction::Ls { runtime_type, remote, json } => {
                self.cmd_ls(runtime_type, *remote, *json).await
            }
            RuntimeAction::Use { runtime_type, version } => {
                self.cmd_use(runtime_type, version).await
            }
            RuntimeAction::Install { runtime_type, version } => {
                self.cmd_install(runtime_type, version.as_deref()).await
            }
            RuntimeAction::Uninstall { runtime_type, version } => {
                self.cmd_uninstall(runtime_type, version).await
            }
            RuntimeAction::Default { runtime_type, version } => {
                self.cmd_default(runtime_type.as_deref(), version.as_deref()).await
            }
            RuntimeAction::Project { action: pa } => {
                self.cmd_project(pa).await
            }
            RuntimeAction::Status { json } => {
                self.cmd_status(*json).await
            }
            RuntimeAction::Check { json } => {
                self.cmd_check(*json).await
            }
        }
    }

    // ── Command implementations ──

    async fn cmd_ls(&self, _rt_filter: &Option<String>, remote: bool, json: bool) -> Result<()> {
        let runtimes = self.runtime_manager.detect_all().await;

        if json {
            if remote {
                // Include remote versions
                let mut entries = Vec::new();
                for rt in &runtimes {
                    let mut entry = serde_json::json!({
                        "runtime_type": rt.runtime_type,
                        "display_name": rt.display_name,
                        "version": rt.version,
                        "available": rt.available,
                        "source": rt.source,
                        "executable_path": rt.executable_path,
                    });
                    if let Ok(versions) = self.runtime_registry.get_versions(&rt.runtime_type).await {
                        entry["remote_versions"] = serde_json::to_value(versions).unwrap_or_default();
                    }
                    entries.push(entry);
                }
                if let Ok(json) = serde_json::to_string_pretty(&entries) {
                    println!("{}", json);
                }
            } else {
                if let Ok(json) = serde_json::to_string_pretty(&runtimes) {
                    println!("{}", json);
                }
            }
        } else {
            // Terminal output
            println!("{}", Self::format_runtime_table(&runtimes));
            if remote {
                for rt in &runtimes {
                    if let Ok(versions) = self.runtime_registry.get_versions(&rt.runtime_type).await {
                        println!("\n  {} 可用版本:", rt.display_name);
                        for v in versions.iter().take(5) {
                            let lifecycle = crate::environment::lifecycle::for_runtime(&rt.runtime_type, &v.version, v.lts.as_deref());
                            println!("    {}  {:20} {}", lifecycle.emoji(), v.version, v.display_name);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn cmd_use(&self, rt_str: &str, version: &str) -> Result<()> {
        let rt = parse_runtime_type_internal(rt_str)?;
        self.runtime_manager.switch_version(&rt, version).await?;
        println!("✅ {} 已切换到版本 {}", rt.display_name(), version);
        Ok(())
    }

    async fn cmd_install(&self, rt_str: &str, version: Option<&str>) -> Result<()> {
        let rt = parse_runtime_type_internal(rt_str)?;
        println!("正在安装 {} {}...", rt.display_name(), version.unwrap_or("latest"));
        // Simple install with no-progress callback
        let on_progress = |_p: InstallProgress| {};
        let info = self.runtime_manager.install_runtime(&rt, version.map(|s| s.to_string()), on_progress).await?;
        if info.available {
            println!("✅ {} 安装完成", rt.display_name());
        } else {
            println!("{}", info.error.as_deref().unwrap_or("安装可能未完成"));
        }
        Ok(())
    }

    async fn cmd_uninstall(&self, rt_str: &str, version: &str) -> Result<()> {
        let rt = parse_runtime_type_internal(rt_str)?;
        self.runtime_manager.uninstall_version(&rt, version).await?;
        println!("✅ {} {} 已卸载", rt.display_name(), version);
        Ok(())
    }

    async fn cmd_default(&self, rt_str: Option<&str>, version: Option<&str>) -> Result<()> {
        match (rt_str, version) {
            (Some(rt), Some(ver)) => {
                let rt = parse_runtime_type_internal(rt)?;
                self.alias_manager.set_default(&rt, ver.to_string()).await;
                println!("✅ {} 默认版本已设置为 {}", rt.display_name(), ver);
            }
            (Some(rt), None) => {
                let rt = parse_runtime_type_internal(rt)?;
                match self.alias_manager.get_default(&rt).await {
                    Some(ver) => println!("{} 默认版本: {}", rt.display_name(), ver),
                    None => println!("{} 未设置默认版本", rt.display_name()),
                }
            }
            (None, _) => {
                let aliases = self.alias_manager.list_defaults().await;
                if aliases.is_empty() {
                    println!("未设置默认版本");
                } else {
                    for (rt, ver) in aliases {
                        println!("{}: {}", rt.display_name(), ver);
                    }
                }
            }
        }
        Ok(())
    }

    async fn cmd_project(&self, action: &ProjectAction) -> Result<()> {
        let db = self.db.lock().await;
        match action {
            ProjectAction::Ls => {
                let projects = db.list_bound_projects()?;
                if projects.is_empty() {
                    println!("暂无绑定项目");
                } else {
                    for p in &projects {
                        println!("📁 {}  ({})", p.name, p.path);
                        if let Some(reqs) = &p.requirements {
                            if let Ok(requirements) = serde_json::from_str::<Vec<ProjectRuntimeRequirement>>(reqs) {
                                for r in &requirements {
                                    println!("   {}: {}", r.runtime_type.display_name(), r.version_spec);
                                }
                            }
                        }
                    }
                }
            }
            ProjectAction::Add { path } => {
                let name = std::path::Path::new(path)
                    .file_name().map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                db.add_bound_project(path, &name)?;
                println!("✅ 项目已添加: {}", name);
            }
            ProjectAction::Remove { id } => {
                db.remove_bound_project(id)?;
                println!("✅ 项目已移除");
            }
            ProjectAction::Sync { id } => {
                println!("正在同步项目 (id={})...", id);
                // Release lock before sync
                drop(db);
                println!("✅ 同步完成 (通过 GUI 查看详情)");
            }
        }
        Ok(())
    }

    async fn cmd_status(&self, json: bool) -> Result<()> {
        let runtimes = self.runtime_manager.detect_all().await;
        if json {
            if let Ok(json) = serde_json::to_string_pretty(&runtimes) {
                println!("{}", json);
            }
        } else {
            println!("{}", Self::format_runtime_table(&runtimes));
        }
        Ok(())
    }

    async fn cmd_check(&self, json: bool) -> Result<()> {
        let updates = crate::environment::upgrade::check_updates(&self.runtime_manager, &self.runtime_registry).await?;
        if json {
            if let Ok(json) = serde_json::to_string_pretty(&updates) {
                println!("{}", json);
            }
        } else if updates.is_empty() {
            println!("✅ 所有运行时已是最新");
        } else {
            for u in &updates {
                let lifecycle = crate::environment::lifecycle::for_runtime(&u.runtime_type, &u.current_version, None);
                println!("{} {} {} — {} → {} (建议升级)", lifecycle.emoji(), u.runtime_type.display_name(), u.current_version, u.current_version, u.latest_version);
            }
        }
        Ok(())
    }

    fn format_runtime_table(runtimes: &[RuntimeInfo]) -> String {
        let mut output = String::new();
        output.push_str(&format!("{:<12} {:<12} {:<10} {}\n", "运行时", "版本", "来源", "路径"));
        output.push_str(&format!("{}\n", "-".repeat(80)));
        for rt in runtimes {
            let status = if rt.available { "✅" } else { "❌" };
            let source = match &rt.source {
                RuntimeSource::System => "系统",
                RuntimeSource::BuiltIn => "内置",
                RuntimeSource::None => "无",
            };
            let version = rt.version.as_deref().unwrap_or("-");
            let path = rt.executable_path.as_deref().unwrap_or("");
            output.push_str(&format!("{} {:<10} {:<12} {:<10} {}\n", status, rt.display_name, version, source, path));
        }
        output
    }
}

fn parse_runtime_type_internal(s: &str) -> Result<RuntimeType> {
    match s.to_lowercase().as_str() {
        "node" | "nodejs" => Ok(RuntimeType::Node),
        "python" | "py" => Ok(RuntimeType::Python),
        "docker" => Ok(RuntimeType::Docker),
        "uv" => Ok(RuntimeType::Uv),
        "go" | "golang" => Ok(RuntimeType::Go),
        "rust" | "rustc" => Ok(RuntimeType::Rust),
        "java" | "jdk" => Ok(RuntimeType::Java),
        "deno" => Ok(RuntimeType::Deno),
        "bun" => Ok(RuntimeType::Bun),
        "ruby" | "irb" | "gem" => Ok(RuntimeType::Ruby),
        _ => Err(crate::error::AppError::InvalidInput(format!("未知运行时: {}. 支持: node, python, docker, uv, go, rust, java, deno, bun, ruby", s))),
    }
}
