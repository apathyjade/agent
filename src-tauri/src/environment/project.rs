// ── Project Detector: Scan project directories for runtime version requirements ──

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::environment::RuntimeType;
use crate::error::Result;

/// A project bound to runtime version management.
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

/// A single runtime requirement from a project.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectRuntimeRequirement {
    pub runtime_type: RuntimeType,
    pub version_spec: String,         // ">=18", "3.12.x", "stable", "20.18.3"
    pub source_file: String,          // ".nvmrc", "go.mod", ".runtime-version"
    pub resolved_version: Option<String>,
}

/// Result of scanning a project directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectScanResult {
    pub project_path: String,
    pub project_name: String,
    pub requirements: Vec<ProjectRuntimeRequirement>,
    pub errors: Vec<String>,
}

/// Result of syncing a project's runtime versions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncResult {
    pub project_id: String,
    pub actions: Vec<SyncAction>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncAction {
    pub runtime_type: RuntimeType,
    pub action: String,     // "install", "switch", "already_matched", "skipped"
    pub from_version: Option<String>,
    pub to_version: String,
    pub success: bool,
}

// ── .runtime-version YAML schema ──

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RuntimeVersionFile {
    version: Option<u32>,
    runtimes: Option<HashMap<String, String>>,
}

/// Detects runtime version requirements from project configuration files.
/// Scans for .runtime-version (authoritative), .nvmrc, .python-version,
/// go.mod, package.json (engines.node), and Cargo.toml.
pub struct ProjectDetector;

impl ProjectDetector {
    /// Scan a directory for all version config files.
    /// Priority: .runtime-version > individual config files (.nvmrc > .python-version > go.mod > package.json > Cargo.toml).
    /// If .runtime-version is found, skip all other checks (it's authoritative).
    pub async fn scan(path: &Path) -> Result<ProjectScanResult> {
        let project_name = Self::project_name_from_path(path);
        let mut requirements = Vec::new();
        let mut errors = Vec::new();

        // 1. Check authoritative .runtime-version first
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
                    errors.push(format!(
                        "无法解析 {} 中的 .runtime-version",
                        path.display()
                    ));
                }
            }
        }

        // 2. Check individual config files (ordered by priority)
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

        // package.json (engines.node)
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

        // Cargo.toml (implies Rust)
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

    /// Parse .runtime-version YAML file.
    /// Schema:
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
            if let Some(rt) = RuntimeType::from_str(&key) {
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

    /// Read .nvmrc, return version spec.
    fn parse_nvmrc(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let spec = content.trim().to_string();
        if spec.is_empty() { None } else { Some(spec) }
    }

    /// Read .python-version file.
    fn parse_python_version(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let spec = content.trim().to_string();
        if spec.is_empty() { None } else { Some(spec) }
    }

    /// Parse go.mod for `go X.Y` directive.
    fn parse_go_mod(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        // Look for "go X.Y" at the start of a line
        let re = regex::Regex::new(r"(?m)^go\s+(\d+\.\d+(?:\.\d+)?)").ok()?;
        let cap = re.captures(&content)?;
        Some(cap.get(1)?.as_str().to_string())
    }

    /// Read engines.node from package.json.
    fn parse_package_json_engines(path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let engines = json.get("engines")?;
        let node = engines.get("node")?;
        node.as_str().map(|s| s.to_string())
    }

    /// Check if Cargo.toml exists.
    fn has_cargo_toml(path: &Path) -> bool {
        path.join("Cargo.toml").exists()
    }

    /// Extract project name from path (directory name).
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
        assert_eq!(
            ProjectDetector::parse_go_mod(&path),
            Some("1.22.4".into())
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
