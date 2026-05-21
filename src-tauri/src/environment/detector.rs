// ── Runtime Detector ──
//
// Detects installed runtimes by:
//   1. Checking PATH via `where` (Windows) / `which` (Unix)
//   2. Running `<command> --version` to verify and capture version

use std::process::Stdio;

use crate::environment::{RuntimeInfo, RuntimeSource, RuntimeType};

pub struct RuntimeDetector;

impl RuntimeDetector {
    pub fn new() -> Self {
        Self
    }

    /// Detect a runtime installed on the system (via PATH).
    pub async fn detect_system(&self, rt: &RuntimeType) -> Option<RuntimeInfo> {
        // Try each command variant (e.g. "python3" then "python")
        for cmd in rt.commands() {
            if let Some(info) = self.try_command(rt, cmd).await {
                return Some(info);
            }
        }
        None
    }

    /// Try a specific command, return Some if found and functional.
    async fn try_command(&self, rt: &RuntimeType, cmd: &str) -> Option<RuntimeInfo> {
        // 1. First, resolve the executable path
        let exec_path = self.resolve_path(cmd).await?;

        // 2. Run version check
        let args = rt.version_args();
        let version = self.get_version_with_args(cmd, args).await;

        let available = version.is_some();

        Some(RuntimeInfo {
            runtime_type: rt.clone(),
            display_name: rt.display_name().to_string(),
            source: RuntimeSource::System,
            version: version.clone(),
            installed_versions: Vec::new(),
            executable_path: Some(exec_path),
            error: if available { None } else { Some("版本检测失败".to_string()) },
            available,
        })
    }

    /// Resolve the full path of a command using `where` (Windows) or `which` (Unix).
    async fn resolve_path(&self, cmd: &str) -> Option<String> {
        let which_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };

        let output = tokio::process::Command::new(which_cmd)
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;

        Some(path)
    }

    /// Get version string by running `<command> <version_args>`.
    async fn get_version_with_args(&self, cmd: &str, args: &[&str]) -> Option<String> {
        let output = tokio::process::Command::new(cmd)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            // Some runtimes output version to stderr (e.g., node --version on some setups)
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() && stderr.contains(|c: char| c.is_ascii_digit()) {
                return Some(stderr);
            }
            return None;
        }

        let version = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if version.is_empty() {
            // Try stderr as fallback
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                return Some(stderr);
            }
            return None;
        }

        Some(version)
    }
}
