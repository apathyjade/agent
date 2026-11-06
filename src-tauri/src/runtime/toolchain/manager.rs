//! 运行时管理器：检测、版本切换与安装的统一入口。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use super::{
    detector::RuntimeDetector, installer::RuntimeInstaller, read_manifest, write_manifest,
    AvailableVersion, InstallProgress, InstalledVersion, RuntimeInfo, RuntimeManifest,
    RuntimeSource, RuntimeType,
};
use crate::core::error::Result;

// ── 管理器 ──

/// 本地运行时检测、版本管理与安装的中心管理器。
pub struct RuntimeManager {
    install_dir: Arc<Mutex<PathBuf>>,
    detector: RuntimeDetector,
    installer: Arc<Mutex<RuntimeInstaller>>,
    cache: Arc<Mutex<HashMap<RuntimeType, RuntimeInfo>>>,
    /// 记录正在安装的运行时，避免并发安装同一个运行时。
    installing: Arc<Mutex<HashSet<RuntimeType>>>,
}

impl RuntimeManager {
    /// 用指定的安装根目录创建管理器。
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

    /// 更新安装目录。
    pub async fn set_install_dir(&self, new_dir: PathBuf) {
        let mut dir = self.install_dir.lock().await;
        *dir = new_dir.clone();
        {
            let mut inst = self.installer.lock().await;
            inst.set_runtimes_dir(new_dir);
        }
        self.cache.lock().await.clear();
    }

    /// 获取当前安装目录。
    pub async fn get_install_dir(&self) -> PathBuf {
        self.install_dir.lock().await.clone()
    }

    // ── 检测 ──

    /// 检测所有运行时：先查系统，再扫描本地已安装版本。
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

    /// 检测某个运行时。
    ///
    /// 显式激活的内置版本优先于 PATH；否则上报系统安装。这正是
    /// `switch_version` 能被观察到的原因：应用优先使用用户选择的版本。
    pub async fn detect(&self, rt: &RuntimeType) -> RuntimeInfo {
        let install_dir = self.install_dir.lock().await.clone();
        let installed = self.list_installed_versions(rt).await;
        let active_version = installed
            .iter()
            .find(|version| version.is_active)
            .map(|version| version.version.clone());

        if let Some(version) = active_version.as_deref() {
            let info = built_in_info(&install_dir, rt, version, installed.clone());
            if info.available {
                return info;
            }
        }

        if let Some(system) = self.detector.detect_system(rt).await {
            if system.available {
                return RuntimeInfo {
                    installed_versions: installed,
                    ..system
                };
            }
        }

        // 即使生效版本的可执行文件已损坏，也继续上报该版本，
        // 以便 UI 说明运行时为何不可用。
        if let Some(version) = active_version {
            return built_in_info(&install_dir, rt, &version, installed);
        }

        RuntimeInfo {
            runtime_type: rt.clone(),
            display_name: rt.display_name().to_string(),
            source: RuntimeSource::Unavailable,
            version: None,
            installed_versions: installed,
            executable_path: None,
            error: Some(format!("{} 未安装", rt.display_name())),
            available: false,
        }
    }

    // ── 版本管理 ──

    /// 列出某个运行时在本地已安装的所有版本。
    pub async fn list_installed_versions(&self, rt: &RuntimeType) -> Vec<InstalledVersion> {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        let manifest_path = rt_dir.join(".manifest.json");

        // 存在清单时读取清单
        if let Some(manifest) = read_manifest(&manifest_path) {
            let active_version = manifest.active_version.clone();
            return manifest
                .versions
                .into_iter()
                .map(move |(ver, info)| {
                    let is_active = Some(ver.clone()) == active_version;
                    InstalledVersion {
                        version: ver,
                        path: info.path,
                        installed_at: info.installed_at,
                        is_active,
                    }
                })
                .collect();
        }

        // 回退：扫描目录中的版本文件夹
        let mut versions = Vec::new();
        if rt_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&rt_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy().to_string();
                    if name_str.starts_with('.') {
                        continue;
                    }
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

    /// 切换某个运行时的生效版本。
    pub async fn switch_version(&self, rt: &RuntimeType, version: &str) -> Result<()> {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        let manifest_path = rt_dir.join(".manifest.json");

        // 确认版本目录存在
        let ver_dir = rt_dir.join(version);
        if !ver_dir.exists() {
            return Err(crate::core::error::AppError::NotFound(format!(
                "版本 {} 未安装",
                version
            )));
        }

        // 更新清单
        let mut manifest = read_manifest(&manifest_path).unwrap_or_else(|| RuntimeManifest {
            active_version: None,
            versions: HashMap::new(),
        });

        manifest.active_version = Some(version.to_string());

        // 确保版本已记录在清单中
        manifest
            .versions
            .entry(version.to_string())
            .or_insert_with(|| crate::runtime::toolchain::manifest::VersionInfo {
                path: version.to_string(),
                installed_at: Utc::now().to_rfc3339(),
            });

        write_manifest(&manifest_path, &manifest)?;

        // 更新缓存
        let mut cache = self.cache.lock().await;
        cache.remove(rt);

        Ok(())
    }

    /// 卸载指定版本。
    pub async fn uninstall_version(&self, rt: &RuntimeType, version: &str) -> Result<()> {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        let ver_dir = rt_dir.join(version);

        if ver_dir.exists() {
            std::fs::remove_dir_all(&ver_dir)?;
        }

        // 更新清单
        let manifest_path = rt_dir.join(".manifest.json");
        if let Some(mut manifest) = read_manifest(&manifest_path) {
            manifest.versions.remove(version);
            if manifest.active_version.as_deref() == Some(version) {
                manifest.active_version = manifest.versions.keys().next().cloned();
            }
            write_manifest(&manifest_path, &manifest)?;
        }

        // 使该运行时的版本缓存失效，下次读取时重新检测
        let mut cache = self.cache.lock().await;
        cache.remove(rt);

        Ok(())
    }

    // ── 安装 ──

    /// 安装某个运行时的指定版本。
    pub async fn install_runtime(
        &self,
        rt: &RuntimeType,
        version: Option<String>,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        let install_dir = self.install_dir.lock().await.clone();
        let installer = self.installer.lock().await;
        installer
            .install(rt, version, install_dir, on_progress)
            .await
    }

    /// 尝试开始安装某个运行时；若已在安装中则返回错误。
    pub async fn try_begin_install(&self, rt: &RuntimeType) -> std::result::Result<(), String> {
        let mut set = self.installing.lock().await;
        if set.contains(rt) {
            return Err(format!("{} 正在安装中", rt.display_name()));
        }
        set.insert(rt.clone());
        Ok(())
    }

    /// 标记某个运行时的安装已结束（无论成功或失败）。
    pub async fn end_install(&self, rt: &RuntimeType) {
        let mut set = self.installing.lock().await;
        set.remove(rt);
    }

    /// 获取可下载的版本列表（离线静态清单，不访问网络）。
    pub async fn list_available_versions(&self, rt: &RuntimeType) -> Vec<AvailableVersion> {
        super::installer::available_versions(rt)
    }

    // ── 校验（供 MCP 模块使用）──

    /// 校验某个运行时是否可用。
    /// 可用时返回 `Ok(())`，否则返回带安装指引的错误字符串。
    pub async fn validate_runtime(&self, rt: &RuntimeType) -> std::result::Result<(), String> {
        let info = self.detect(rt).await;
        if info.available {
            return Ok(());
        }
        Err(format!(
            "❌ {} 未安装\n\n💡 请通过「运行时管理」安装",
            rt.display_name()
        ))
    }

    // ── 缓存 ──

    /// 获取某个运行时的缓存检测结果。
    pub async fn get_cached(&self, rt: &RuntimeType) -> Option<RuntimeInfo> {
        self.cache.lock().await.get(rt).cloned()
    }

    /// 获取所有缓存的检测结果，按显示名排序。
    pub async fn get_all_cached(&self) -> Vec<RuntimeInfo> {
        let cache = self.cache.lock().await;
        let mut result: Vec<RuntimeInfo> = cache.values().cloned().collect();
        result.sort_by(|a, b| {
            a.runtime_type
                .display_name()
                .cmp(b.runtime_type.display_name())
        });
        result
    }

    // ── 磁盘占用 ──

    /// 获取某个运行时的总磁盘占用（字节）。
    pub async fn get_disk_usage(&self, rt: &RuntimeType) -> u64 {
        let install_dir = self.install_dir.lock().await.clone();
        let rt_dir = install_dir.join(rt.dir_name());
        if !rt_dir.exists() {
            return 0;
        }
        let installed = self.list_installed_versions(rt).await;
        let mut total = 0u64;
        for v in &installed {
            let ver_dir = rt_dir.join(&v.version);
            if ver_dir.exists() {
                total += dir_size(&ver_dir);
            }
        }
        total
    }

    /// 获取所有运行时的磁盘占用，按大小降序排列。
    pub async fn get_all_disk_usage(&self) -> Vec<DiskUsageItem> {
        let mut items = Vec::new();
        for rt in RuntimeType::all() {
            let size = self.get_disk_usage(rt).await;
            let info = self.get_cached(rt).await.unwrap_or_else(|| RuntimeInfo {
                runtime_type: rt.clone(),
                display_name: rt.display_name().to_string(),
                source: RuntimeSource::Unavailable,
                version: None,
                installed_versions: vec![],
                executable_path: None,
                error: None,
                available: false,
            });
            items.push(DiskUsageItem {
                runtime_type: rt.clone(),
                display_name: rt.display_name().to_string(),
                size_bytes: size,
                installed_count: info.installed_versions.len(),
                active_version: info.version.clone(),
            });
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
        items
    }
}

/// 单个运行时的磁盘占用统计。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiskUsageItem {
    pub runtime_type: RuntimeType,
    pub display_name: String,
    pub size_bytes: u64,
    pub installed_count: usize,
    pub active_version: Option<String>,
}

/// 递归计算目录的总大小。
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// 为已激活的内置版本构造 `RuntimeInfo`。
fn built_in_info(
    install_dir: &std::path::Path,
    rt: &RuntimeType,
    version: &str,
    installed: Vec<InstalledVersion>,
) -> RuntimeInfo {
    let exe_path = find_exe_in_version(install_dir, rt, version);
    let available = exe_path
        .as_ref()
        .map(|path| check_exe_works(path, rt))
        .unwrap_or(false);

    RuntimeInfo {
        runtime_type: rt.clone(),
        display_name: rt.display_name().to_string(),
        source: RuntimeSource::BuiltIn,
        version: Some(version.to_string()),
        installed_versions: installed,
        executable_path: exe_path,
        error: if available {
            None
        } else {
            Some("内置版本不可用".to_string())
        },
        available,
    }
}

/// 在版本目录中查找可执行文件路径。
/// 同时检查根目录与 `bin/` 子目录（Go 把二进制放在 `bin/`）。
fn find_exe_in_version(base: &std::path::Path, rt: &RuntimeType, version: &str) -> Option<String> {
    let ver_dir = base.join(rt.dir_name()).join(version);
    let exe_name = rt.primary_command();
    #[cfg(target_os = "windows")]
    let candidates = vec![
        ver_dir.join(format!("{}.exe", exe_name)),
        ver_dir.join("bin").join(format!("{}.exe", exe_name)),
    ];
    #[cfg(not(target_os = "windows"))]
    let candidates = vec![ver_dir.join(exe_name), ver_dir.join("bin").join(exe_name)];
    for c in &candidates {
        if c.exists() {
            return Some(c.to_string_lossy().to_string());
        }
    }
    None
}

/// 快速检查可执行文件是否能响应 `--version`。
fn check_exe_works(path: &str, rt: &RuntimeType) -> bool {
    let output = std::process::Command::new(path)
        .args(rt.version_args())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    matches!(output, Ok(o) if o.status.success())
}
