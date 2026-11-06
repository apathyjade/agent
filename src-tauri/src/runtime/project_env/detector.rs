//! 项目探测器：扫描项目目录中的运行时版本需求。

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::core::error::Result;
use crate::runtime::toolchain::RuntimeType;

/// 绑定到运行时版本管理的项目。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoundProject {
    pub id: String,
    pub path: String,
    pub name: String,
    pub auto_sync: bool,
    pub last_scan: Option<String>,
    pub requirements: Vec<ProjectRuntimeRequirement>,
    pub created_at: String,
    pub updated_at: String,
}

/// 项目中的单条运行时需求。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectRuntimeRequirement {
    pub runtime_type: RuntimeType,
    pub version_spec: String, // ">=18"、"3.12.x"、"stable"、"20.18.3"
    pub source_file: String,  // ".nvmrc"、"go.mod"、".runtime-version"
    pub resolved_version: Option<String>,
}

/// 扫描项目目录的结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectScanResult {
    pub project_path: String,
    pub project_name: String,
    pub requirements: Vec<ProjectRuntimeRequirement>,
    pub errors: Vec<String>,
}

/// 同步项目运行时版本的结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncResult {
    pub project_id: String,
    pub actions: Vec<SyncAction>,
    pub success: bool,
    pub error: Option<String>,
}

/// 单条同步动作。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncAction {
    pub runtime_type: RuntimeType,
    pub action: String, // "install"、"switch"、"already_matched"、"skipped"
    pub from_version: Option<String>,
    pub to_version: String,
    pub success: bool,
}

// ── .runtime-version YAML 结构 ──

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RuntimeVersionFile {
    version: Option<u32>,
    runtimes: Option<HashMap<String, String>>,
}

/// 从项目配置文件中检测运行时版本需求。
/// 扫描 `.runtime-version`（权威来源）、`.nvmrc`、`.python-version`、
/// `go.mod`、`package.json`（engines.node）与 `Cargo.toml`。
pub struct ProjectDetector;

impl ProjectDetector {
    /// 扫描目录中的所有版本配置文件。
    ///
    /// 优先级从高到低：`.runtime-version`，其次是各独立配置文件
    /// （`.nvmrc`、`.python-version`、`go.mod`、`package.json`、`Cargo.toml`）。
    /// 找到 `.runtime-version` 时会跳过其余检查（它是权威来源）。
    /// 该函数只读取本地文件，不访问网络。
    pub async fn scan(path: &Path) -> Result<ProjectScanResult> {
        let project_name = Self::project_name_from_path(path);
        let mut requirements = Vec::new();
        let mut errors = Vec::new();

        // 1. 先检查权威的 .runtime-version
        let runtime_version_path = path.join(".runtime-version");
        if runtime_version_path.exists() {
            match Self::parse_runtime_version(&runtime_version_path) {
                Some(reqs) => {
                    requirements = reqs;
                    return Ok(ProjectScanResult {
                        project_path: path.to_string_lossy().to_string(),
                        project_name,
                        requirements,
                        errors,
                    });
                }
                None => {
                    errors.push(format!("无法解析 {} 中的 .runtime-version", path.display()));
                }
            }
        }

        // 2. 检查独立配置文件（按优先级排序）
        // .nvmrc
        let nvmrc_path = path.join(".nvmrc");
        if nvmrc_path.exists() {
            if let Some(spec) = Self::parse_nvmrc(&nvmrc_path) {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Node,
                    version_spec: spec,
                    source_file: ".nvmrc".into(),
                    resolved_version: None,
                });
            }
        }

        // .python-version
        let python_version_path = path.join(".python-version");
        if python_version_path.exists() {
            if let Some(spec) = Self::parse_python_version(&python_version_path) {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Python,
                    version_spec: spec,
                    source_file: ".python-version".into(),
                    resolved_version: None,
                });
            }
        }

        // go.mod
        let go_mod_path = path.join("go.mod");
        if go_mod_path.exists() {
            if let Some(spec) = Self::parse_go_mod(&go_mod_path) {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Go,
                    version_spec: spec,
                    source_file: "go.mod".into(),
                    resolved_version: None,
                });
            }
        }

        // package.json 的 engines.node 字段
        let package_json_path = path.join("package.json");
        if package_json_path.exists() {
            if let Some(spec) = Self::parse_package_json_engines(&package_json_path) {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: RuntimeType::Node,
                    version_spec: spec,
                    source_file: "package.json".into(),
                    resolved_version: None,
                });
            }
        }

        // Cargo.toml（意味着需要 Rust）
        if Self::has_cargo_toml(path) {
            requirements.push(ProjectRuntimeRequirement {
                runtime_type: RuntimeType::Rust,
                version_spec: "stable".into(),
                source_file: "Cargo.toml".into(),
                resolved_version: None,
            });
        }

        Ok(ProjectScanResult {
            project_path: path.to_string_lossy().to_string(),
            project_name,
            requirements,
            errors,
        })
    }

    /// 解析 .runtime-version YAML 文件。
    /// 结构：
    /// ```yaml
    /// version: 1
    /// runtimes:
    ///   node: "20.18.3"
    ///   python: "3.12.x"
    ///   go: "1.22.4"
    /// ```
    fn parse_runtime_version(path: &Path) -> Option<Vec<ProjectRuntimeRequirement>> {
        let content = std::fs::read_to_string(path).ok()?;
        let file: RuntimeVersionFile = serde_yaml::from_str(&content).ok()?;
        let runtimes = file.runtimes?;

        let mut requirements = Vec::new();
        for (key, spec) in runtimes {
            if let Ok(rt) = key.parse::<RuntimeType>() {
                requirements.push(ProjectRuntimeRequirement {
                    runtime_type: rt,
                    version_spec: spec,
                    source_file: ".runtime-version".into(),
                    resolved_version: None,
                });
            }
        }

        Some(requirements)
    }

    /// 读取 .nvmrc，返回版本规格。
    fn parse_nvmrc(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let spec = content.trim().to_string();
        if spec.is_empty() {
            None
        } else {
            Some(spec)
        }
    }

    /// 读取 .python-version 文件。
    fn parse_python_version(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let spec = content.trim().to_string();
        if spec.is_empty() {
            None
        } else {
            Some(spec)
        }
    }

    /// 解析 go.mod 中的 `go X.Y` 指令。
    fn parse_go_mod(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        // 在行首查找 "go X.Y"
        let re = regex::Regex::new(r"(?m)^go\s+(\d+\.\d+(?:\.\d+)?)").ok()?;
        let cap = re.captures(&content)?;
        Some(cap.get(1)?.as_str().to_string())
    }

    /// 读取 package.json 中的 engines.node。
    fn parse_package_json_engines(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let engines = json.get("engines")?;
        let node = engines.get("node")?;
        node.as_str().map(|s| s.to_string())
    }

    /// 检查 Cargo.toml 是否存在。
    fn has_cargo_toml(path: &Path) -> bool {
        path.join("Cargo.toml").exists()
    }

    /// 从路径中提取项目名（目录名）。
    fn project_name_from_path(path: &Path) -> String {
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_go_mod() {
        let dir = std::env::temp_dir().join("test_go_mod");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("go.mod");
        std::fs::write(&path, "module example.com/foo\ngo 1.22.4\n").ok();
        assert_eq!(ProjectDetector::parse_go_mod(&path), Some("1.22.4".into()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
