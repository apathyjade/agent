// ── Toolchain CLI commands ──
//
// Commands in this module only touch the standalone toolchain engine. The
// app-specific project binding service is loaded lazily and only for
// `project` subcommands, so `ls/use/install/status/check` never open the app
// database.

use std::sync::Arc;

use clap::Subcommand;
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::core::db::repository::Database;
use crate::core::error::Result;
use crate::runtime::project_env::{service as project_service, ProjectRuntimeRequirement};
use crate::runtime::toolchain::RuntimeServices;
use crate::runtime::toolchain::{RuntimeInfo, RuntimeSource, RuntimeType};

#[derive(Debug, Subcommand)]
pub enum ToolchainAction {
    /// 列出运行时（默认仅显示本地检测结果）
    Ls {
        /// 只显示指定运行时（node, python, rust, ...）
        runtime_type: Option<String>,
        /// 同时显示远程可用版本（需要网络）
        #[arg(long)]
        remote: bool,
        /// JSON 输出
        #[arg(long)]
        json: bool,
    },
    /// 切换运行时版本（仅在本应用内生效，不修改系统 PATH）
    Use {
        runtime_type: String,
        version: String,
    },
    /// 安装运行时版本
    Install {
        runtime_type: String,
        /// 省略时安装最新版
        version: Option<String>,
    },
    /// 卸载已安装版本
    Uninstall {
        runtime_type: String,
        version: String,
    },
    /// 查看或设置默认版本（持久化到配置文件）
    Default {
        runtime_type: Option<String>,
        version: Option<String>,
    },
    /// 项目运行时绑定管理
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// 运行时健康总览
    Status {
        #[arg(long)]
        json: bool,
    },
    /// 检查版本更新（EOL / 维护中的运行时）
    Check {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProjectAction {
    /// 列出绑定项目
    Ls,
    /// 扫描并绑定项目
    Add { path: String },
    /// 移除绑定项目
    Remove { id: String },
    /// 同步项目运行时版本（缺失版本会先安装）
    Sync { id: String },
}

/// Shared toolchain context. It deliberately has no database handle.
struct ToolchainContext {
    services: RuntimeServices,
    config: AppConfig,
}

impl ToolchainContext {
    async fn new() -> Result<Self> {
        let config = AppConfig::load()?;
        let services = RuntimeServices::new(config.runtime_options())?;
        Ok(Self { services, config })
    }

    fn active_manager(&self, rt: &RuntimeType) -> Option<&str> {
        self.config
            .active_managers
            .get(&rt.to_string())
            .map(String::as_str)
    }

    async fn run(mut self, action: ToolchainAction) -> Result<()> {
        match action {
            ToolchainAction::Ls {
                runtime_type,
                remote,
                json,
            } => self.ls(runtime_type.as_deref(), remote, json).await,
            ToolchainAction::Use {
                runtime_type,
                version,
            } => self.r#use(&runtime_type, &version).await,
            ToolchainAction::Install {
                runtime_type,
                version,
            } => self.install(&runtime_type, version.as_deref()).await,
            ToolchainAction::Uninstall {
                runtime_type,
                version,
            } => self.uninstall(&runtime_type, &version).await,
            ToolchainAction::Default {
                runtime_type,
                version,
            } => {
                self.default(runtime_type.as_deref(), version.as_deref())
                    .await
            }
            ToolchainAction::Project { action } => {
                // Only project management needs the app database.
                let db = Arc::new(Mutex::new(Database::new()?));
                project(&self, &db, action).await
            }
            ToolchainAction::Status { json } => self.status(json).await,
            ToolchainAction::Check { json } => self.check(json).await,
        }
    }

    // `serde_json::json!` expands to code that clippy sees as an unwrap.
    #[allow(clippy::disallowed_methods)]
    async fn ls(&self, runtime_type: Option<&str>, remote: bool, json: bool) -> Result<()> {
        let runtimes = match runtime_type {
            Some(filter) => {
                let rt: RuntimeType = filter.parse()?;
                vec![self.services.detect(&rt).await]
            }
            None => self.services.detect_all().await,
        };

        if json {
            if remote {
                let mut entries = Vec::with_capacity(runtimes.len());
                for info in &runtimes {
                    let mut entry = serde_json::json!({
                        "runtime_type": info.runtime_type,
                        "display_name": info.display_name,
                        "version": info.version,
                        "available": info.available,
                        "source": info.source,
                        "executable_path": info.executable_path,
                    });
                    if let Ok(versions) = self
                        .services
                        .registry
                        .get_versions(&info.runtime_type)
                        .await
                    {
                        entry["remote_versions"] =
                            serde_json::to_value(versions).unwrap_or_default();
                    }
                    entries.push(entry);
                }
                print_json(&entries)?;
            } else {
                print_json(&runtimes)?;
            }
            return Ok(());
        }

        println!("{}", format_runtime_table(&runtimes));
        if remote {
            for info in &runtimes {
                match self
                    .services
                    .registry
                    .get_versions(&info.runtime_type)
                    .await
                {
                    Ok(versions) => {
                        println!("\n  {} 可用版本:", info.display_name);
                        for version in versions.iter().take(8) {
                            let lifecycle = crate::runtime::toolchain::lifecycle::for_runtime(
                                &info.runtime_type,
                                &version.version,
                                version.lts.as_deref(),
                            );
                            println!(
                                "    {}  {:<20} {}",
                                lifecycle.emoji(),
                                version.version,
                                version.display_name
                            );
                        }
                    }
                    Err(error) => {
                        println!("\n  {} 版本查询失败: {}", info.display_name, error);
                    }
                }
            }
        }
        Ok(())
    }

    async fn r#use(&self, runtime_type: &str, version: &str) -> Result<()> {
        let rt: RuntimeType = runtime_type.parse()?;
        let info = self
            .services
            .switch(&rt, version, self.active_manager(&rt))
            .await?;
        println!(
            "✅ {} 已切换到 {}（仅本应用内生效，未修改系统 PATH）",
            info.display_name, version
        );
        Ok(())
    }

    async fn install(&self, runtime_type: &str, version: Option<&str>) -> Result<()> {
        let rt: RuntimeType = runtime_type.parse()?;
        let target = version.unwrap_or("latest");
        println!("正在安装 {} {}...", rt.display_name(), target);

        let info = self
            .services
            .install(
                &rt,
                version.map(str::to_string),
                self.active_manager(&rt),
                |_| {},
            )
            .await?;

        if info.available {
            println!(
                "✅ {} {} 安装完成（来源: {}）",
                info.display_name,
                info.version.as_deref().unwrap_or(target),
                source_label(&info.source)
            );
        } else {
            println!("⚠️ {}", info.error.as_deref().unwrap_or("安装可能未完成"));
        }
        Ok(())
    }

    async fn uninstall(&self, runtime_type: &str, version: &str) -> Result<()> {
        let rt: RuntimeType = runtime_type.parse()?;
        self.services
            .uninstall(&rt, version, self.active_manager(&rt))
            .await?;
        println!("✅ {} {} 已卸载", rt.display_name(), version);
        Ok(())
    }

    async fn default(&mut self, runtime_type: Option<&str>, version: Option<&str>) -> Result<()> {
        match (runtime_type, version) {
            (Some(runtime_type), Some(version)) => {
                let rt: RuntimeType = runtime_type.parse()?;
                self.services
                    .aliases
                    .set_default(&rt, version.to_string())
                    .await;
                self.config
                    .runtime_defaults
                    .insert(rt.to_string(), version.to_string());
                self.config.save()?;
                println!("✅ {} 默认版本已设置为 {}", rt.display_name(), version);
            }
            (Some(runtime_type), None) => {
                let rt: RuntimeType = runtime_type.parse()?;
                match self.services.aliases.get_default(&rt).await {
                    Some(version) => println!("{} 默认版本: {}", rt.display_name(), version),
                    None => println!("{} 未设置默认版本", rt.display_name()),
                }
            }
            (None, _) => {
                let defaults = self.services.defaults().await;
                if defaults.is_empty() {
                    println!("未设置默认版本");
                } else {
                    let mut entries: Vec<_> = defaults.into_iter().collect();
                    entries.sort_by_key(|(rt, _)| rt.display_name());
                    for (rt, version) in entries {
                        println!("{}: {}", rt.display_name(), version);
                    }
                }
            }
        }
        Ok(())
    }

    // `serde_json::json!` expands to code that clippy sees as an unwrap.
    #[allow(clippy::disallowed_methods)]
    async fn status(&self, json: bool) -> Result<()> {
        let runtimes = self.services.detect_all().await;
        let available = runtimes.iter().filter(|info| info.available).count();
        let total = runtimes.len();
        let install_dir = self.services.install_dir().await;
        let defaults = self.services.defaults().await;

        if json {
            let defaults_map: std::collections::BTreeMap<_, _> = defaults
                .into_iter()
                .map(|(rt, version)| (rt.to_string(), version))
                .collect();
            print_json(&serde_json::json!({
                "install_dir": install_dir.to_string_lossy(),
                "available": available,
                "total": total,
                "runtimes": runtimes,
                "defaults": defaults_map,
                "active_managers": self.config.active_managers,
            }))?;
            return Ok(());
        }

        println!("{}", format_runtime_table(&runtimes));
        println!("\n可用: {}/{}", available, total);
        println!("安装目录: {}", install_dir.display());
        if !defaults.is_empty() {
            let mut entries: Vec<_> = defaults.into_iter().collect();
            entries.sort_by_key(|(rt, _)| rt.display_name());
            let rendered: Vec<String> = entries
                .into_iter()
                .map(|(rt, version)| format!("{}={}", rt.display_name(), version))
                .collect();
            println!("默认版本: {}", rendered.join(", "));
        }
        Ok(())
    }

    async fn check(&self, json: bool) -> Result<()> {
        let updates = self.services.check_updates().await?;
        if json {
            print_json(&updates)?;
            return Ok(());
        }
        if updates.is_empty() {
            println!("✅ 未发现需要升级的运行时");
            return Ok(());
        }
        for update in &updates {
            let lifecycle = crate::runtime::toolchain::lifecycle::for_runtime(
                &update.runtime_type,
                &update.current_version,
                None,
            );
            println!(
                "{} {} {} → {}",
                lifecycle.emoji(),
                update.runtime_type.display_name(),
                update.current_version,
                update.latest_version
            );
        }
        Ok(())
    }
}

async fn project(
    ctx: &ToolchainContext,
    db: &Arc<Mutex<Database>>,
    action: ProjectAction,
) -> Result<()> {
    match action {
        ProjectAction::Ls => {
            let projects = project_service::list(db).await?;
            if projects.is_empty() {
                println!("暂无绑定项目");
                return Ok(());
            }
            for project in projects {
                println!("📁 {}  ({})", project.name, project.path);
                println!("   id: {}", project.id);
                print_requirements(&project.requirements);
            }
        }
        ProjectAction::Add { path } => {
            let project = project_service::bind(db, &path).await?;
            println!("✅ 项目已添加: {} ({})", project.name, project.id);
            print_requirements(&project.requirements);
        }
        ProjectAction::Remove { id } => {
            project_service::remove(db, &id).await?;
            println!("✅ 项目已移除: {}", id);
        }
        ProjectAction::Sync { id } => {
            println!("正在同步项目 {}...", id);
            let result =
                project_service::sync(&ctx.services, db, &id, &ctx.config.active_managers).await?;

            for action in &result.actions {
                let mark = if action.success { "✅" } else { "⚠️" };
                println!(
                    "{} {}: {} {} → {}",
                    mark,
                    action.runtime_type.display_name(),
                    action.action,
                    action.from_version.as_deref().unwrap_or("-"),
                    action.to_version
                );
            }
            if result.success {
                println!("✅ 同步完成");
            } else {
                println!(
                    "⚠️ {}",
                    result.error.as_deref().unwrap_or("部分运行时同步失败")
                );
            }
        }
    }
    Ok(())
}

pub async fn execute(action: ToolchainAction) -> Result<()> {
    ToolchainContext::new().await?.run(action).await
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let rendered = serde_json::to_string_pretty(value)?;
    println!("{}", rendered);
    Ok(())
}

fn print_requirements(requirements: &[ProjectRuntimeRequirement]) {
    for requirement in requirements {
        println!(
            "   {}: {} ({})",
            requirement.runtime_type.display_name(),
            requirement.version_spec,
            requirement.source_file
        );
    }
}

fn source_label(source: &RuntimeSource) -> &'static str {
    match source {
        RuntimeSource::System => "系统",
        RuntimeSource::BuiltIn => "内置",
        RuntimeSource::Unavailable => "无",
    }
}

fn format_runtime_table(runtimes: &[RuntimeInfo]) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{:<4} {:<12} {:<12} {:<8} {}\n",
        "", "运行时", "版本", "来源", "路径"
    ));
    output.push_str(&format!("{}\n", "-".repeat(84)));
    for info in runtimes {
        let status = if info.available { "✅" } else { "❌" };
        output.push_str(&format!(
            "{:<4} {:<12} {:<12} {:<8} {}\n",
            status,
            info.display_name,
            info.version.as_deref().unwrap_or("-"),
            source_label(&info.source),
            info.executable_path.as_deref().unwrap_or("-")
        ));
    }
    output
}
