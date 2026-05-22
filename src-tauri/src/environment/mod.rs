// ── Environment Module: Runtime Detection, Version Management & Installation ──
//
// General-purpose local runtime manager. Not specific to MCP.
// Supports: system detection, version management, configurable install dir.

mod detector;
pub mod http_client;
mod installer;
pub mod lifecycle;
mod manifest;
pub mod registry;
pub mod sources;
pub mod project;
pub mod resolver;
pub mod alias;
pub mod cli;
mod upgrade;
pub mod node_integration;
pub mod node_toolchain;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::error::Result;

pub use alias::AliasManager;
pub use detector::RuntimeDetector;
pub use installer::RuntimeInstaller;
pub use lifecycle::VersionLifecycle;
pub use manifest::{read_manifest, write_manifest, RuntimeManifest};
pub use project::{BoundProject, ProjectDetector, ProjectRuntimeRequirement, ProjectScanResult, SyncAction, SyncResult};
pub use registry::{CachedVersions, RuntimeRegistry, RuntimeVersion, VersionSource};
pub use resolver::VersionResolver;
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

impl RuntimeType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "node" | "nodejs" | "npx" => Some(RuntimeType::Node),
            "python" | "python3" => Some(RuntimeType::Python),
            "docker" => Some(RuntimeType::Docker),
            "uv" | "uvx" => Some(RuntimeType::Uv),
            "go" | "golang" => Some(RuntimeType::Go),
            "rust" | "rustc" | "cargo" => Some(RuntimeType::Rust),
            "java" | "jdk" | "jre" => Some(RuntimeType::Java),
            "deno" => Some(RuntimeType::Deno),
            "bun" => Some(RuntimeType::Bun),
            "ruby" | "irb" | "gem" | "bundler" => Some(RuntimeType::Ruby),
            _ => None,
        }
    }

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

    /// Infer runtime type from a CLI command string.
    pub fn infer_from_command(cmd: &str) -> Option<Self> {
        let lower = cmd.to_lowercase();
        let base = std::path::Path::new(&lower)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&lower);
        match base {
            "node" | "npx" | "npm" => Some(RuntimeType::Node),
            "python" | "python3" | "pip" | "pip3" => Some(RuntimeType::Python),
            "docker" => Some(RuntimeType::Docker),
            "uv" | "uvx" => Some(RuntimeType::Uv),
            "go" | "golang" => Some(RuntimeType::Go),
            "rustc" | "cargo" | "rustup" => Some(RuntimeType::Rust),
            "java" | "javac" | "jdk" => Some(RuntimeType::Java),
            "deno" => Some(RuntimeType::Deno),
            "bun" => Some(RuntimeType::Bun),
            "ruby" | "irb" | "gem" | "bundler" => Some(RuntimeType::Ruby),
            _ => None,
        }
    }

    /// All variants as a slice.
    pub fn all() -> &'static [RuntimeType] {
        &[Node, Python, Docker, Uv, Go, Rust, Java, Deno, Bun, Ruby]
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

use RuntimeType::*;

// ── Installation Source ──

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSource {
    System,
    BuiltIn,
    None,
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

// ── Manager ──

/// Central manager for local runtime detection, version management, and installation.
pub struct RuntimeManager {
    install_dir: Arc<Mutex<PathBuf>>,
    detector: RuntimeDetector,
    installer: Arc<Mutex<RuntimeInstaller>>,
    cache: Arc<Mutex<HashMap<RuntimeType, RuntimeInfo>>>,
    /// Tracks currently installing runtimes to prevent concurrent installations.
    installing: Arc<Mutex<HashSet<RuntimeType>>>,
}

impl RuntimeManager {
    pub fn new(install_dir: PathBuf) -> Self {
        let installer = RuntimeInstaller::new(install_dir.clone());
        Self {
            install_dir: Arc::new(Mutex::new(install_dir)),
            detector: RuntimeDetector::new(),
            installer: Arc::new(Mutex::new(installer)),
            cache: Arc::new(Mutex::new(HashMap::new())),
            installing: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Update the install directory.
    pub async fn set_install_dir(&self, new_dir: PathBuf) {
        let mut dir = self.install_dir.lock().await;
        *dir = new_dir.clone();
        {
            let mut inst = self.installer.lock().await;
            inst.set_runtimes_dir(new_dir);
        }
        self.cache.lock().await.clear();
    }

    /// Get the current install directory.
    pub async fn get_install_dir(&self) -> PathBuf {
        self.install_dir.lock().await.clone()
    }

    // ── Detection ──

    /// Detect all runtimes: check system, then scan local installed versions.
    pub async fn detect_all(&self) -> Vec<RuntimeInfo> {
        let mut results = Vec::new();
        for rt in RuntimeType::all() {
            let info = self.detect(rt).await;
            results.push(info);
        }
        let mut cache = self.cache.lock().await;
        for info in &results {
            cache.insert(info.runtime_type.clone(), info.clone());
        }
        results
    }

    /// Detect a specific runtime (system first, then local installed versions).
    pub async fn detect(&self, rt: &RuntimeType) -> RuntimeInfo {
        let install_dir = self.install_dir.lock().await.clone();
        let installed = self.list_installed_versions(rt).await;

        // Try system detection
        if let Some(sys_info) = self.detector.detect_system(rt).await {
            if sys_info.available {
                return RuntimeInfo {
                    installed_versions: installed,
                    ..sys_info
                };
            }
        }

        // Check if any installed version is active
        let active = installed.iter().find(|v| v.is_active);
        if let Some(ver) = active {
            let exe_path = find_exe_in_version(&install_dir, rt, &ver.version);
            let version_str = Some(ver.version.clone());
            let available = exe_path.as_ref().map(|p| check_exe_works(p, rt)).unwrap_or(false);

            return RuntimeInfo {
                runtime_type: rt.clone(),
                display_name: rt.display_name().to_string(),
                source: RuntimeSource::BuiltIn,
                version: version_str,
                installed_versions: installed,
                executable_path: exe_path,
                error: if available { None } else { Some("内置版本不可用".to_string()) },
                available,
            };
        }

        // No installed versions, not available on system
        RuntimeInfo {
            runtime_type: rt.clone(),
            display_name: rt.display_name().to_string(),
            source: RuntimeSource::None,
            version: None,
            installed_versions: installed,
            executable_path: None,
            error: Some(format!("{} 未安装", rt.display_name())),
            available: false,
        }
    }

    // ── Version Management ──

    /// List all locally installed versions for a runtime.
    pub async fn list_installed_versions(&self, rt: &RuntimeType) -> Vec<InstalledVersion> {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        let manifest_path = rt_dir.join(".manifest.json");

        // Read manifest if exists
        if let Some(manifest) = read_manifest(&manifest_path) {
            let active_version = manifest.active_version.clone();
            return manifest.versions.into_iter().map(move |(ver, info)| {
                let is_active = Some(ver.clone()) == active_version;
                InstalledVersion {
                    version: ver,
                    path: info.path,
                    installed_at: info.installed_at,
                    is_active,
                }
            }).collect();
        }

        // Fallback: scan directory for version folders
        let mut versions = Vec::new();
        if rt_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&rt_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy().to_string();
                    if name_str.starts_with('.') { continue; }
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        versions.push(InstalledVersion {
                            version: name_str.clone(),
                            path: name_str,
                            installed_at: String::new(),
                            is_active: false,
                        });
                    }
                }
            }
        }
        versions
    }

    /// Switch the active version for a runtime.
    pub async fn switch_version(&self, rt: &RuntimeType, version: &str) -> Result<()> {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        let manifest_path = rt_dir.join(".manifest.json");

        // Verify the version directory exists
        let ver_dir = rt_dir.join(version);
        if !ver_dir.exists() {
            return Err(crate::error::AppError::NotFound(
                format!("版本 {} 未安装", version)
            ));
        }

        // Update manifest
        let mut manifest = read_manifest(&manifest_path).unwrap_or_else(|| RuntimeManifest {
            active_version: None,
            versions: HashMap::new(),
        });

        manifest.active_version = Some(version.to_string());

        // Ensure version exists in manifest
        manifest.versions.entry(version.to_string()).or_insert_with(|| {
            crate::environment::manifest::VersionInfo {
                path: version.to_string(),
                installed_at: Utc::now().to_rfc3339(),
            }
        });

        write_manifest(&manifest_path, &manifest)?;

        // Update cache
        let mut cache = self.cache.lock().await;
        cache.remove(rt);

        Ok(())
    }

    /// Uninstall a specific version.
    pub async fn uninstall_version(&self, rt: &RuntimeType, version: &str) -> Result<()> {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        let ver_dir = rt_dir.join(version);

        if ver_dir.exists() {
            std::fs::remove_dir_all(&ver_dir)?;
        }

        // Update manifest
        let manifest_path = rt_dir.join(".manifest.json");
        if let Some(mut manifest) = read_manifest(&manifest_path) {
            manifest.versions.remove(version);
            if manifest.active_version.as_deref() == Some(version) {
                manifest.active_version = manifest.versions.keys().next().cloned();
            }
            write_manifest(&manifest_path, &manifest)?;
        }

        // Update cache
        let mut cache = self.cache.lock().await;
        cache.remove(rt);

        Ok(())
    }

    // ── Installation ──

    /// Install a specific version of a runtime.
    pub async fn install_runtime(
        &self,
        rt: &RuntimeType,
        version: Option<String>,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        let install_dir = self.install_dir.lock().await.clone();
        let installer = self.installer.lock().await;
        installer.install(rt, version, install_dir, on_progress).await
    }

    /// Try to begin installing a runtime. Returns an error if already installing.
    pub async fn try_begin_install(&self, rt: &RuntimeType) -> std::result::Result<(), String> {
        let mut set = self.installing.lock().await;
        if set.contains(rt) {
            return Err(format!("{} 正在安装中", rt.display_name()));
        }
        set.insert(rt.clone());
        Ok(())
    }

    /// Mark a runtime installation as finished (success or failure).
    pub async fn end_install(&self, rt: &RuntimeType) {
        let mut set = self.installing.lock().await;
        set.remove(rt);
    }

    /// Get available versions for download.
    pub async fn list_available_versions(&self, rt: &RuntimeType) -> Vec<AvailableVersion> {
        installer::available_versions(rt)
    }

    // ── Validation (used by MCP module) ──

    /// Validate that a runtime is available.
    /// Returns Ok(()) if available, or an error with guidance.
    pub async fn validate_runtime(&self, rt: &RuntimeType) -> std::result::Result<(), String> {
        let info = self.detect(rt).await;
        if info.available {
            return Ok(());
        }
        Err(format!("❌ {} 未安装\n\n💡 请通过「运行时管理」安装", rt.display_name()))
    }

    // ── Cache ──

    pub async fn get_cached(&self, rt: &RuntimeType) -> Option<RuntimeInfo> {
        self.cache.lock().await.get(rt).cloned()
    }

    pub async fn get_all_cached(&self) -> Vec<RuntimeInfo> {
        let cache = self.cache.lock().await;
        let mut result: Vec<RuntimeInfo> = cache.values().cloned().collect();
        result.sort_by(|a, b| a.runtime_type.display_name().cmp(b.runtime_type.display_name()));
        result
    }
}

/// Find the executable path within a version directory.
/// Checks both the root and `bin/` subdirectory (Go places binaries in `bin/`).
fn find_exe_in_version(base: &PathBuf, rt: &RuntimeType, version: &str) -> Option<String> {
    let ver_dir = base.join(rt.dir_name()).join(version);
    let exe_name = rt.primary_command();
    #[cfg(target_os = "windows")]
    let candidates = vec![
        ver_dir.join(format!("{}.exe", exe_name)),
        ver_dir.join("bin").join(format!("{}.exe", exe_name)),
    ];
    #[cfg(not(target_os = "windows"))]
    let candidates = vec![
        ver_dir.join(exe_name),
        ver_dir.join("bin").join(exe_name),
    ];
    for c in &candidates {
        if c.exists() {
            return Some(c.to_string_lossy().to_string());
        }
    }
    None
}

/// Quick check that an executable responds to --version.
fn check_exe_works(path: &str, rt: &RuntimeType) -> bool {
    let output = std::process::Command::new(path)
        .args(rt.version_args())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    matches!(output, Ok(o) if o.status.success())
}
