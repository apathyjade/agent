//! 运行时探测器。
//!
//! 通过以下方式检测已安装的运行时：
//!   1. 用 `where`（Windows）/ `which`（Unix）检查 PATH
//!   2. 运行 `<command> --version` 验证并捕获版本号

use std::process::Stdio;

use crate::runtime::toolchain::{FoundExecutable, PathConflict, RuntimeInfo, RuntimeSource, RuntimeType};

/// 基于系统 PATH 检测运行时。
pub struct RuntimeDetector;

impl Default for RuntimeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeDetector {
    /// 创建探测器。
    pub fn new() -> Self {
        Self
    }

    /// 检测系统 PATH 中已安装的运行时。
    ///
    /// 只做本地命令探测，不访问网络；找不到或命令无法正常响应时返回 `None`。
    pub async fn detect_system(&self, rt: &RuntimeType) -> Option<RuntimeInfo> {
        // 依次尝试每个命令变体（例如先 "python3" 再 "python"）
        for cmd in rt.commands() {
            if let Some(info) = self.try_command(rt, cmd).await {
                return Some(info);
            }
        }
        None
    }

    /// 尝试某个具体命令，找到且可用时返回 `Some`。
    async fn try_command(&self, rt: &RuntimeType, cmd: &str) -> Option<RuntimeInfo> {
        // 1. 先解析可执行文件路径
        let exec_path = self.resolve_path(cmd).await?;

        // 2. 运行版本检查
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
            error: if available {
                None
            } else {
                Some("版本检测失败".to_string())
            },
            available,
        })
    }

    /// 用 `where`（Windows）或 `which`（Unix）解析命令的完整路径。
    async fn resolve_path(&self, cmd: &str) -> Option<String> {
        let which_cmd = if cfg!(target_os = "windows") {
            "where"
        } else {
            "which"
        };

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

    /// 运行 `<command> <version_args>` 获取版本字符串。
    async fn get_version_with_args(&self, cmd: &str, args: &[&str]) -> Option<String> {
        let output = tokio::process::Command::new(cmd)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            // 某些运行时会向 stderr 输出版本（例如某些环境下的 node --version）
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() && stderr.contains(|c: char| c.is_ascii_digit()) {
                return Some(stderr);
            }
            return None;
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if version.is_empty() {
            // 回退到 stderr
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                return Some(stderr);
            }
            return None;
        }

        Some(version)
    }

    // ── PATH 冲突检测 ──

    /// 检测某个运行时的所有 PATH 冲突。
    /// 返回该运行时各命令在 PATH 中找到的全部可执行文件。
    pub async fn detect_path_conflicts(&self, rt: &RuntimeType) -> Vec<PathConflict> {
        let mut all_exes = Vec::new();
        for cmd in rt.commands() {
            let paths = self.resolve_all_paths(cmd).await;
            for path in paths {
                let version = self.get_version_for_exe(&path, rt).await;
                all_exes.push(FoundExecutable {
                    path,
                    version,
                    is_active: false,
                });
            }
        }
        if all_exes.is_empty() {
            return vec![];
        }
        // 判断哪个是“生效”的（PATH 中第一个）
        all_exes[0].is_active = true;
        let conflict = all_exes.len() > 1;
        vec![PathConflict {
            runtime_type: rt.clone(),
            executables: all_exes,
            conflict,
        }]
    }

    /// 解析某个命令的所有路径（不只第一个）。
    async fn resolve_all_paths(&self, cmd: &str) -> Vec<String> {
        let (which_cmd, which_args) = if cfg!(target_os = "windows") {
            ("where", vec![cmd])
        } else {
            ("which", vec!["-a", cmd])
        };
        let output = tokio::process::Command::new(which_cmd)
            .args(&which_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok();

        match output {
            Some(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            _ => vec![],
        }
    }

    /// 获取指定可执行文件路径的版本字符串。
    async fn get_version_for_exe(&self, exe_path: &str, rt: &RuntimeType) -> Option<String> {
        let args = rt.version_args();
        let output = tokio::process::Command::new(exe_path)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() && stderr.contains(|c: char| c.is_ascii_digit()) {
                return Some(stderr);
            }
            return None;
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if version.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                return Some(stderr);
            }
            return None;
        }
        Some(version)
    }
}
