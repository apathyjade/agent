use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use super::{
    detector::RuntimeDetector, installer::RuntimeInstaller, read_manifest, write_manifest,
    AvailableVersion, FoundExecutable, InstallProgress, InstalledVersion,
    PathConflict, RuntimeInfo, RuntimeSource, RuntimeType, RuntimeManifest,
};
use crate::error::Result;

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
        super::installer::available_versions(rt)
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

    // ── Disk Usage ──

    /// Get total disk usage for a specific runtime.
    pub async fn get_disk_usage(&self, rt: &RuntimeType) -> u64 {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        if !rt_dir.exists() { return 0; }
        let installed = self.list_installed_versions(rt).await;
        let mut total = 0u64;
        for v in &installed {
            let ver_dir = rt_dir.join(&v.version);
            if ver_dir.exists() { total += dir_size(&ver_dir); }
        }
        total
    }

    /// Get disk usage for all runtimes, sorted by size descending.
    pub async fn get_all_disk_usage(&self) -> Vec<DiskUsageItem> {
        let mut items = Vec::new();
        for rt in RuntimeType::all() {
            let size = self.get_disk_usage(rt).await;
            let info = self.get_cached(rt).await.unwrap_or_else(|| RuntimeInfo {
                runtime_type: rt.clone(), display_name: rt.display_name().to_string(),
                source: RuntimeSource::None, version: None, installed_versions: vec![],
                executable_path: None, error: None, available: false,
            });
            items.push(DiskUsageItem {
                runtime_type: rt.clone(), display_name: rt.display_name().to_string(), size_bytes: size, installed_count: info.installed_versions.len(), active_version: info.version.clone(),
            });
        }
        items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        items
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiskUsageItem {
    pub runtime_type: RuntimeType,
    pub display_name: String,
    pub size_bytes: u64,
    pub installed_count: usize,
    pub active_version: Option<String>,
}

/// Recursively compute total size of a directory.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() { total += dir_size(&path); }
            else if let Ok(meta) = entry.metadata() { total += meta.len(); }
        }
    }
    total
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
