// ── Node.js External Version Manager Integration ──
//
// Detects and wraps existing Node.js version managers (fnm, volta, nvm, nvm-windows).
// Allows the Agent to piggyback on the user's existing toolchain instead of
// managing Node versions independently.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::Command;

use crate::error::Result;

/// External Node.js version managers that Agent can integrate with.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExternalManager {
    /// Fast Node Manager (Rust) — https://fnm.vercel.app
    Fnm,
    /// Volta JS Toolchain Manager (Rust) — https://volta.sh
    Volta,
    /// nvm (Shell) — https://github.com/nvm-sh/nvm
    Nvm,
    /// nvm-windows (Go) — https://github.com/coreybutler/nvm-windows
    NvmWindows,
}

impl ExternalManager {
    pub fn name(&self) -> &'static str {
        match self {
            ExternalManager::Fnm => "fnm",
            ExternalManager::Volta => "volta",
            ExternalManager::Nvm => "nvm",
            ExternalManager::NvmWindows => "nvm-windows",
        }
    }
}

/// Strategy for how Agent resolves Node.js versions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NodeIntegrationStrategy {
    /// Only use Agent's built-in Node downloads (no external wrap).
    Standalone,
    /// Only use external managers; fail if none found.
    WrapExisting,
    /// Prefer external managers, fall back to self-managed.
    Hybrid,
}

impl Default for NodeIntegrationStrategy {
    fn default() -> Self {
        Self::Hybrid
    }
}

/// Result of external manager detection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExternalManagerStatus {
    pub available: Vec<ExternalManager>,
    pub path: Option<String>,
    pub version: Option<String>,
    pub managed_versions: Vec<String>,
}

/// Detects and wraps external Node.js version managers.
pub struct ExternalNodeManager {
    pub strategy: NodeIntegrationStrategy,
}

impl ExternalNodeManager {
    pub fn new(strategy: NodeIntegrationStrategy) -> Self {
        Self { strategy }
    }

    /// Detect which external managers are available on the system.
    pub async fn detect_all() -> Vec<(ExternalManager, bool)> {
        let mut results = Vec::new();

        // fnm: check `fnm --version`
        let fnm_ok = Self::check_command("fnm", &["--version"]).await;
        results.push((ExternalManager::Fnm, fnm_ok));

        // volta: check `volta --version`
        let volta_ok = Self::check_command("volta", &["--version"]).await;
        results.push((ExternalManager::Volta, volta_ok));

        // nvm: check $NVM_DIR/nvm.sh or `nvm --version` (nvm is a shell function)
        let nvm_ok = Self::detect_nvm().await;
        results.push((ExternalManager::Nvm, nvm_ok));

        // nvm-windows: check `nvm version`
        let nvmw_ok = Self::check_command("nvm", &["version"]).await;
        if nvmw_ok && !fnm_ok {
            // Only consider nvm-windows if fnm isn't also present
            // (nvm-windows and fnm both respond to `nvm --version` ambiguity)
            results.push((ExternalManager::NvmWindows, nvmw_ok));
        }

        results
    }

    /// Quick check: does a command exist and respond?
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

    /// nvm detection: check env var and shell function.
    async fn detect_nvm() -> bool {
        // Check NVM_DIR environment variable
        if std::env::var("NVM_DIR").is_ok() {
            return true;
        }
        // Try sourcing nvm.sh and running nvm --version
        // This is inherently fragile, so we also check a common path
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default();
        let candidate_paths = [
            PathBuf::from(&home).join(".nvm").join("nvm.sh"),
            PathBuf::from(&home).join(".nvm").join("nvm-exec"),
            PathBuf::from(&home).join(".config").join("nvm").join("nvm.sh"),
        ];
        candidate_paths.iter().any(|p| p.exists())
    }

    /// Get the version of Node managed by a specific external manager.
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
                    if !ver.is_empty() { Some(ver) } else { None }
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
                    // Parse: "Node.js v20.18.3 (default)"
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
                // nvm is a shell function; invoke via bash -c
                let out = Command::new("bash")
                    .args(["-lc", "nvm current 2>/dev/null"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .ok()?;
                if out.status.success() {
                    let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !ver.is_empty() { Some(ver) } else { None }
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
                    // Find line with "*" marking current
                    output.lines()
                        .find(|l| l.contains('*'))
                        .and_then(|l| {
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

    /// List all Node versions managed by a specific external manager.
    pub async fn list_manager_versions(manager: &ExternalManager) -> Vec<String> {
        let result = match manager {
            ExternalManager::Fnm => {
                Self::run_cmd_parse_lines("fnm", &["list"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { return None; }
                    let ver = trimmed.trim_start_matches('→').trim().trim_start_matches('v');
                    if ver.is_empty() { None } else { Some(ver.to_string()) }
                }).await
            }
            ExternalManager::Volta => {
                Self::run_cmd_parse_lines("volta", &["list", "node", "--format", "plain"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with("Node.js") { return None; }
                    let ver = trimmed.split_whitespace().next()?;
                    let ver = ver.trim_start_matches('v');
                    if ver.is_empty() { None } else { Some(ver.to_string()) }
                }).await
            }
            ExternalManager::Nvm => {
                Self::run_cmd_parse_lines("bash", &["-lc", "nvm list --no-alias 2>/dev/null"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed == "->" { return None; }
                    let ver = trimmed.trim_start_matches("->").trim().trim_start_matches('v');
                    if ver.is_empty() { None } else { Some(ver.to_string()) }
                }).await
            }
            ExternalManager::NvmWindows => {
                Self::run_cmd_parse_lines("nvm", &["list"], |line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.contains("---") || trimmed.contains("Current") {
                        return None;
                    }
                    let ver = trimmed.split_whitespace().find(|w| w.contains('.'))?;
                    Some(ver.to_string())
                }).await
            }
        };
        result.unwrap_or_default()
    }

    /// Helper: run a command, parse stdout lines with a closure.
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

    /// Install a Node version using the preferred external manager.
    pub async fn install_via_manager(manager: &ExternalManager, version: &str) -> Result<()> {
        match manager {
            ExternalManager::Fnm => {
                let status = Command::new("fnm")
                    .args(["install", version])
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .await
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("fnm install 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        "fnm install 返回非零退出码".into()
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
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("volta install 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        "volta install 返回非零退出码".into()
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
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("nvm install 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        "nvm install 返回非零退出码".into()
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
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("nvm install 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        "nvm install 返回非零退出码".into()
                    ));
                }
            }
        }
        Ok(())
    }

    /// Switch active Node version via the preferred manager.
    pub async fn switch_via_manager(manager: &ExternalManager, version: &str) -> Result<()> {
        match manager {
            ExternalManager::Fnm => {
                let status = Command::new("fnm")
                    .args(["use", version])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("fnm use 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        format!("fnm use {} 失败", version)
                    ));
                }
            }
            ExternalManager::Volta => {
                // Volta uses shims — no explicit "use" needed.
                // But we can pin the default via `volta pin node@<ver>`
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
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("nvm use 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        format!("nvm use {} 失败", version)
                    ));
                }
            }
            ExternalManager::NvmWindows => {
                let status = Command::new("nvm")
                    .args(["use", version])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .map_err(|e| crate::error::AppError::InvalidInput(
                        format!("nvm use 失败: {}", e)
                    ))?;
                if !status.success() {
                    return Err(crate::error::AppError::InvalidInput(
                        format!("nvm use {} 失败", version)
                    ));
                }
            }
        }
        Ok(())
    }
}
