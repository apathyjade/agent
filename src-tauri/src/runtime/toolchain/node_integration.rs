//! Node.js 外部版本管理器集成。
//!
//! 检测并包装用户已有的 Node.js 版本管理器（fnm、volta、nvm、nvm-windows），
//! 让应用可以复用现有工具链，而不是另起一套 Node 版本管理。

use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::Command;

use crate::core::error::Result;

/// 应用可集成的外部 Node.js 版本管理器。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExternalManager {
    /// Fast Node Manager（Rust 实现）—— https://fnm.vercel.app
    Fnm,
    /// Volta JS 工具链管理器（Rust 实现）—— https://volta.sh
    Volta,
    /// nvm（Shell 实现）—— https://github.com/nvm-sh/nvm
    Nvm,
    /// nvm-windows（Go 实现）—— https://github.com/coreybutler/nvm-windows
    NvmWindows,
}

impl ExternalManager {
    /// 管理器的短名称（用于日志与配置）。
    pub fn name(&self) -> &'static str {
        match self {
            ExternalManager::Fnm => "fnm",
            ExternalManager::Volta => "volta",
            ExternalManager::Nvm => "nvm",
            ExternalManager::NvmWindows => "nvm-windows",
        }
    }
}

/// 应用解析 Node.js 版本的集成策略。
///
/// 三种策略的取舍：
/// - [`NodeIntegrationStrategy::Standalone`]：完全使用应用内置下载，适合干净
///   环境，行为可预测但会重复占用磁盘。
/// - [`NodeIntegrationStrategy::WrapExisting`]：只使用外部管理器；检测不到时
///   直接报错，适合用户已明确用 fnm/volta/nvm 管理版本的项目。
/// - [`NodeIntegrationStrategy::Hybrid`]（默认）：优先外部管理器，失败时回退
///   到内置下载，兼顾复用与可用性。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum NodeIntegrationStrategy {
    /// 只使用应用内置的 Node 下载（不包装外部管理器）。
    Standalone,
    /// 只使用外部管理器；一个都没检测到时失败。
    WrapExisting,
    /// 优先使用外部管理器，失败时回退到应用自管理。
    #[default]
    Hybrid,
}

/// 外部管理器检测结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExternalManagerStatus {
    pub available: Vec<ExternalManager>,
    pub path: Option<String>,
    pub version: Option<String>,
    pub managed_versions: Vec<String>,
}

/// 检测并包装外部 Node.js 版本管理器。
pub struct ExternalNodeManager {
    pub strategy: NodeIntegrationStrategy,
}

impl ExternalNodeManager {
    /// 用指定策略创建管理器。
    pub fn new(strategy: NodeIntegrationStrategy) -> Self {
        Self { strategy }
    }

    /// 检测系统上有哪些外部管理器可用。
    ///
    /// 会依次执行各管理器的版本命令；`nvm` 是 Shell 函数，改用环境变量与
    /// 常见安装路径判断。该函数只做本地探测，不访问网络。
    pub async fn detect_all() -> Vec<(ExternalManager, bool)> {
        let mut results = Vec::new();

        // fnm：检查 `fnm --version`
        let fnm_ok = Self::check_command("fnm", &["--version"]).await;
        results.push((ExternalManager::Fnm, fnm_ok));

        // volta：检查 `volta --version`
        let volta_ok = Self::check_command("volta", &["--version"]).await;
        results.push((ExternalManager::Volta, volta_ok));

        // nvm：检查 $NVM_DIR/nvm.sh 或 `nvm --version`（nvm 是 Shell 函数）
        let nvm_ok = Self::detect_nvm().await;
        results.push((ExternalManager::Nvm, nvm_ok));

        // nvm-windows：检查 `nvm version`
        let nvmw_ok = Self::check_command("nvm", &["version"]).await;
        if nvmw_ok && !fnm_ok {
            // 只有 fnm 不存在时才认为这是 nvm-windows
            // （两者都会响应 `nvm --version`，存在歧义）
            results.push((ExternalManager::NvmWindows, nvmw_ok));
        }

        results
    }

    /// 快速检查：命令是否存在且能正常响应。
    async fn check_command(cmd: &str, args: &[&str]) -> bool {
        Command::new(cmd)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// nvm 检测：检查环境变量与 Shell 函数。
    async fn detect_nvm() -> bool {
        // 检查 NVM_DIR 环境变量
        if std::env::var("NVM_DIR").is_ok() {
            return true;
        }
        // 尝试 source nvm.sh 并运行 nvm --version
        // 这种做法本身不稳定，因此同时检查常见安装路径
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let candidate_paths = [
            PathBuf::from(&home).join(".nvm").join("nvm.sh"),
            PathBuf::from(&home).join(".nvm").join("nvm-exec"),
            PathBuf::from(&home)
                .join(".config")
                .join("nvm")
                .join("nvm.sh"),
        ];
        candidate_paths.iter().any(|p| p.exists())
    }

    /// 获取某个外部管理器当前使用的 Node 版本。
    pub async fn get_manager_node_version(manager: &ExternalManager) -> Option<String> {
        match manager {
            ExternalManager::Fnm => {
                let out = Command::new("fnm")
                    .args(["current"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .ok()?;
                if out.status.success() {
                    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !ver.is_empty() {
                        Some(ver)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ExternalManager::Volta => {
                let out = Command::new("volta")
                    .args(["list", "node", "--format", "plain"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .ok()?;
                if out.status.success() {
                    let output = String::from_utf8_lossy(&out.stdout);
                    // 解析："Node.js v20.18.3 (default)"
                    output.lines().next().and_then(|line| {
                        line.split_whitespace()
                            .nth(1)
                            .map(|s| s.trim_start_matches('v').to_string())
                    })
                } else {
                    None
                }
            }
            ExternalManager::Nvm => {
                // nvm 是 Shell 函数，需通过 bash -c 调用
                let out = Command::new("bash")
                    .args(["-lc", "nvm current 2>/dev/null"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .ok()?;
                if out.status.success() {
                    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !ver.is_empty() {
                        Some(ver)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            ExternalManager::NvmWindows => {
                let out = Command::new("nvm")
                    .args(["list"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .ok()?;
                if out.status.success() {
                    let output = String::from_utf8_lossy(&out.stdout);
                    // 找到带 "*" 标记的当前版本行
                    output.lines().find(|l| l.contains('*')).and_then(|l| {
                        l.split_whitespace()
                            .find(|w| w.contains('.'))
                            .map(|s| s.to_string())
                    })
                } else {
                    None
                }
            }
        }
    }

    /// 列出某个外部管理器管理的所有 Node 版本。
    pub async fn list_manager_versions(manager: &ExternalManager) -> Vec<String> {
        let result = match manager {
            ExternalManager::Fnm => {
                Self::run_cmd_parse_lines("fnm", &["list"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let ver = trimmed
                        .trim_start_matches('→')
                        .trim()
                        .trim_start_matches('v');
                    if ver.is_empty() {
                        None
                    } else {
                        Some(ver.to_string())
                    }
                })
                .await
            }
            ExternalManager::Volta => {
                Self::run_cmd_parse_lines("volta", &["list", "node", "--format", "plain"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with("Node.js") {
                        return None;
                    }
                    let ver = trimmed.split_whitespace().next()?;
                    let ver = ver.trim_start_matches('v');
                    if ver.is_empty() {
                        None
                    } else {
                        Some(ver.to_string())
                    }
                })
                .await
            }
            ExternalManager::Nvm => {
                Self::run_cmd_parse_lines(
                    "bash",
                    &["-lc", "nvm list --no-alias 2>/dev/null"],
                    |line| {
                        let trimmed = line.trim();
                        if trimmed.is_empty() || trimmed == "->" {
                            return None;
                        }
                        let ver = trimmed
                            .trim_start_matches("->")
                            .trim()
                            .trim_start_matches('v');
                        if ver.is_empty() {
                            None
                        } else {
                            Some(ver.to_string())
                        }
                    },
                )
                .await
            }
            ExternalManager::NvmWindows => {
                Self::run_cmd_parse_lines("nvm", &["list"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.contains("---") || trimmed.contains("Current")
                    {
                        return None;
                    }
                    let ver = trimmed.split_whitespace().find(|w| w.contains('.'))?;
                    Some(ver.to_string())
                })
                .await
            }
        };
        result.unwrap_or_default()
    }

    /// 辅助函数：执行命令并用闭包解析 stdout 的每一行。
    async fn run_cmd_parse_lines<F>(cmd: &str, args: &[&str], parser: F) -> Option<Vec<String>>
    where
        F: Fn(&str) -> Option<String>,
    {
        let out = Command::new(cmd)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?;
        if out.status.success() {
            let output = String::from_utf8_lossy(&out.stdout);
            Some(output.lines().filter_map(parser).collect())
        } else {
            None
        }
    }

    /// 使用首选的外部管理器安装 Node 版本。
    pub async fn install_via_manager(manager: &ExternalManager, version: &str) -> Result<()> {
        match manager {
            ExternalManager::Fnm => {
                let status = Command::new("fnm")
                    .args(["install", version])
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("fnm install 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(
                        "fnm install 返回非零退出码".into(),
                    ));
                }
            }
            ExternalManager::Volta => {
                let install_arg = format!("node@{}", version);
                let status = Command::new("volta")
                    .args(["install", &install_arg])
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("volta install 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(
                        "volta install 返回非零退出码".into(),
                    ));
                }
            }
            ExternalManager::Nvm => {
                let status = Command::new("bash")
                    .args(["-lc", &format!("nvm install {} 2>/dev/null", version)])
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("nvm install 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(
                        "nvm install 返回非零退出码".into(),
                    ));
                }
            }
            ExternalManager::NvmWindows => {
                let status = Command::new("nvm")
                    .args(["install", version])
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("nvm install 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(
                        "nvm install 返回非零退出码".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// 通过首选管理器切换当前 Node 版本。
    pub async fn switch_via_manager(manager: &ExternalManager, version: &str) -> Result<()> {
        match manager {
            ExternalManager::Fnm => {
                let status = Command::new("fnm")
                    .args(["use", version])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("fnm use 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(format!(
                        "fnm use {} 失败",
                        version
                    )));
                }
            }
            ExternalManager::Volta => {
                // Volta 使用 shim —— 无需显式 "use"。
                // 但可以通过 `volta pin node@<ver>` 固定默认版本
                let pin_arg = format!("node@{}", version);
                let _ = Command::new("volta")
                    .args(["pin", &pin_arg])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
            }
            ExternalManager::Nvm => {
                let status = Command::new("bash")
                    .args(["-lc", &format!("nvm use {} 2>/dev/null", version)])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("nvm use 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(format!(
                        "nvm use {} 失败",
                        version
                    )));
                }
            }
            ExternalManager::NvmWindows => {
                let status = Command::new("nvm")
                    .args(["use", version])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .map_err(|e| {
                        crate::core::error::AppError::InvalidInput(format!("nvm use 失败: {}", e))
                    })?;
                if !status.success() {
                    return Err(crate::core::error::AppError::InvalidInput(format!(
                        "nvm use {} 失败",
                        version
                    )));
                }
            }
        }
        Ok(())
    }
}
