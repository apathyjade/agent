// ── Toolchain Module: Local Runtime Detection, Version Management & Installation ──
//
// Standalone toolchain manager for language runtimes (Node, Python, ...).
// It must not depend on the desktop app, its database or its UI state.

pub mod alias;
mod detector;
pub mod http_client;
mod installer;
pub mod lifecycle;
pub mod manager;
pub mod manager_detector;
pub mod manager_executor;
mod manifest;
pub mod node_integration;
pub mod node_toolchain;
pub mod registry;
pub mod resolver;
pub mod services;
pub mod sources;
mod upgrade;

pub use alias::AliasManager;
pub use detector::RuntimeDetector;
pub use installer::RuntimeInstaller;
pub use lifecycle::VersionLifecycle;
pub use manager::{DiskUsageItem, RuntimeManager};
pub use manifest::{read_manifest, write_manifest, RuntimeManifest};
pub use registry::{CachedVersions, RuntimeRegistry, RuntimeVersion, VersionSource};
pub use resolver::VersionResolver;
pub use services::{external_manager, RuntimeOptions, RuntimeServices};
pub use upgrade::{check_updates, VersionUpdate};

// ── Runtime Types ──

/// Supported runtime environments.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeType {
    Node,
    Python,
    Docker,
    Uv,
    Go,
    Rust,
    Java,
    Deno,
    Bun,
    Ruby,
}

impl std::fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeType::Node => write!(f, "node"),
            RuntimeType::Python => write!(f, "python"),
            RuntimeType::Docker => write!(f, "docker"),
            RuntimeType::Uv => write!(f, "uv"),
            RuntimeType::Go => write!(f, "go"),
            RuntimeType::Rust => write!(f, "rust"),
            RuntimeType::Java => write!(f, "java"),
            RuntimeType::Deno => write!(f, "deno"),
            RuntimeType::Bun => write!(f, "bun"),
            RuntimeType::Ruby => write!(f, "ruby"),
        }
    }
}

impl std::str::FromStr for RuntimeType {
    type Err = crate::core::error::AppError;

    fn from_str(s: &str) -> crate::core::error::Result<Self> {
        let normalized = s.trim().to_lowercase();
        let runtime = match normalized.as_str() {
            "node" | "nodejs" | "npx" | "npm" => RuntimeType::Node,
            "python" | "python3" | "py" | "pip" | "pip3" => RuntimeType::Python,
            "docker" => RuntimeType::Docker,
            "uv" | "uvx" => RuntimeType::Uv,
            "go" | "golang" => RuntimeType::Go,
            "rust" | "rustc" | "cargo" | "rustup" => RuntimeType::Rust,
            "java" | "jdk" | "jre" | "javac" => RuntimeType::Java,
            "deno" => RuntimeType::Deno,
            "bun" => RuntimeType::Bun,
            "ruby" | "irb" | "gem" | "bundler" => RuntimeType::Ruby,
            _ => return Err(crate::core::error::AppError::InvalidInput(format!(
                "未知运行时: {}. 支持: node, python, docker, uv, go, rust, java, deno, bun, ruby",
                s
            ))),
        };
        Ok(runtime)
    }
}

impl RuntimeType {
    pub fn display_name(&self) -> &'static str {
        match self {
            RuntimeType::Node => "Node.js",
            RuntimeType::Python => "Python",
            RuntimeType::Docker => "Docker",
            RuntimeType::Uv => "uv",
            RuntimeType::Go => "Go",
            RuntimeType::Rust => "Rust",
            RuntimeType::Java => "Java (JDK)",
            RuntimeType::Deno => "Deno",
            RuntimeType::Bun => "Bun",
            RuntimeType::Ruby => "Ruby",
        }
    }

    /// CLI commands for this runtime (used for PATH detection).
    pub fn commands(&self) -> &[&'static str] {
        match self {
            RuntimeType::Node => &["node", "npx"],
            RuntimeType::Python => &["python3", "python"],
            RuntimeType::Docker => &["docker"],
            RuntimeType::Uv => &["uv", "uvx"],
            RuntimeType::Go => &["go"],
            RuntimeType::Rust => &["rustc", "cargo"],
            RuntimeType::Java => &["java", "javac"],
            RuntimeType::Deno => &["deno"],
            RuntimeType::Bun => &["bun"],
            RuntimeType::Ruby => &["ruby", "irb"],
        }
    }

    /// Primary command name.
    pub fn primary_command(&self) -> &'static str {
        self.commands()[0]
    }

    /// Version check arguments (e.g. ["--version"] or ["version"] for Go).
    pub fn version_args(&self) -> &'static [&'static str] {
        match self {
            RuntimeType::Go => &["version"],
            RuntimeType::Java => &["-version"],
            _ => &["--version"],
        }
    }

    /// Infer runtime type from an executable command (ignores arguments).
    pub fn infer_from_command(cmd: &str) -> Option<Self> {
        let executable = cmd.split_whitespace().next()?;
        let base = std::path::Path::new(executable)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(executable);
        base.parse().ok()
    }

    /// All variants as a slice.
    pub fn all() -> &'static [RuntimeType] {
        &[
            RuntimeType::Node,
            RuntimeType::Python,
            RuntimeType::Docker,
            RuntimeType::Uv,
            RuntimeType::Go,
            RuntimeType::Rust,
            RuntimeType::Java,
            RuntimeType::Deno,
            RuntimeType::Bun,
            RuntimeType::Ruby,
        ]
    }

    /// Directory name used for storing versions on disk.
    pub fn dir_name(&self) -> &'static str {
        match self {
            RuntimeType::Node => "node",
            RuntimeType::Python => "python",
            RuntimeType::Docker => "docker",
            RuntimeType::Uv => "uv",
            RuntimeType::Go => "go",
            RuntimeType::Rust => "rust",
            RuntimeType::Java => "java",
            RuntimeType::Deno => "deno",
            RuntimeType::Bun => "bun",
            RuntimeType::Ruby => "ruby",
        }
    }
}

// ── Installation Source ──

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSource {
    System,
    BuiltIn,
    /// No usable installation was found. Kept as `"none"` on the wire for the UI.
    #[serde(rename = "none")]
    Unavailable,
}

// ── Installed Version ──

/// A single installed version of a runtime.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstalledVersion {
    pub version: String,
    pub path: String,
    pub installed_at: String,
    pub is_active: bool,
}

// ── Runtime Info ──

/// Detailed info about a detected runtime.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeInfo {
    pub runtime_type: RuntimeType,
    pub display_name: String,
    pub source: RuntimeSource,
    /// Currently active version string (if installed).
    pub version: Option<String>,
    /// All locally installed versions.
    pub installed_versions: Vec<InstalledVersion>,
    /// Path to the active executable.
    pub executable_path: Option<String>,
    pub error: Option<String>,
    pub available: bool,
}

// ── PATH Conflict Detection ──

/// A single executable found on PATH.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FoundExecutable {
    pub path: String,
    pub version: Option<String>,
    #[serde(default)]
    pub is_active: bool,
}

/// PATH conflict info for a runtime type.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathConflict {
    pub runtime_type: RuntimeType,
    pub executables: Vec<FoundExecutable>,
    pub conflict: bool,
}

// ── Install Progress ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallProgress {
    pub runtime_type: RuntimeType,
    pub stage: String,
    pub progress: f64,
    pub message: String,
}

// ── Available Version (for download) ──

/// A version available for download from the internet.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AvailableVersion {
    pub version: String,
    pub display_name: String,
    pub url: String,
}

// ── Manager extracted to manager.rs ──
