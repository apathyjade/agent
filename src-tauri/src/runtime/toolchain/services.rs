//! 运行时服务：共享的组合根。
//!
//! 应用与 CLI 都通过本模块构建工具链栈，因此安装/切换/卸载语义不会在两个
//! 前端之间出现分歧。本模块刻意不依赖应用的配置、数据库或 UI 状态；调用方
//! 只需传入一个纯值 `RuntimeOptions`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::core::error::{AppError, Result};

use super::alias::AliasManager;
use super::manager::RuntimeManager;
use super::manager_executor;
use super::registry::RuntimeRegistry;
use super::resolver::VersionResolver;
use super::{InstallProgress, RuntimeInfo, RuntimeType, VersionUpdate};

/// 构建工具链栈所需的配置。
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
    /// 自定义安装目录。默认为 `<data>/agent/runtimes`。
    pub install_dir: Option<PathBuf>,
    /// 下载时可选使用的 HTTP 代理。
    pub download_proxy: Option<String>,
    /// 每个运行时的持久化默认版本，以运行时 id 为键。
    pub defaults: HashMap<String, String>,
}

/// 运行时管理器、版本注册表、解析器与别名存储。
#[derive(Clone)]
pub struct RuntimeServices {
    pub manager: Arc<RuntimeManager>,
    pub registry: Arc<RuntimeRegistry>,
    pub resolver: Arc<VersionResolver>,
    pub aliases: Arc<AliasManager>,
}

impl RuntimeServices {
    /// 用纯配置构建工具链栈。
    pub fn new(options: RuntimeOptions) -> Result<Self> {
        let install_dir = resolve_install_dir(options.install_dir.as_deref());
        crate::runtime::toolchain::http_client::init_http_client(options.download_proxy.as_deref());

        let registry = Arc::new(RuntimeRegistry::new());
        let resolver = Arc::new(VersionResolver::new(registry.clone()));
        let aliases = Arc::new(AliasManager::with_defaults(parse_stored_defaults(
            &options.defaults,
        )));

        Ok(Self {
            manager: Arc::new(RuntimeManager::new(install_dir)),
            registry,
            resolver,
            aliases,
        })
    }

    /// 检测单个运行时。
    pub async fn detect(&self, rt: &RuntimeType) -> RuntimeInfo {
        self.manager.detect(rt).await
    }

    /// 检测所有运行时。
    pub async fn detect_all(&self) -> Vec<RuntimeInfo> {
        self.manager.detect_all().await
    }

    /// 把版本规格解析为确切版本。
    pub async fn resolve(&self, rt: &RuntimeType, spec: &str) -> Result<String> {
        self.resolver.resolve(rt, spec).await
    }

    /// 安装运行时版本。
    ///
    /// - 仅当存在激活的外部管理器（fnm/nvm/uv/pyenv/rustup）**且**指定了版本
    ///   时才走外部管理器，否则使用内置安装器。
    /// - `active_manager = Some("built-in")` 始终使用内置路径。
    pub async fn install(
        &self,
        rt: &RuntimeType,
        version: Option<String>,
        active_manager: Option<&str>,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        if let Some(manager) = external_manager(active_manager) {
            let version = version.ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "{} 由 {} 管理，安装时必须指定版本",
                    rt.display_name(),
                    manager
                ))
            })?;
            manager_executor::install_version(manager, rt, &version)
                .map_err(AppError::InvalidInput)?;
            return Ok(self.detect(rt).await);
        }

        self.manager
            .try_begin_install(rt)
            .await
            .map_err(AppError::InvalidInput)?;
        let result = self.manager.install_runtime(rt, version, on_progress).await;
        self.manager.end_install(rt).await;

        let info = result?;
        if info.available {
            Ok(self.detect(rt).await)
        } else {
            Ok(info)
        }
    }

    /// 激活已安装版本（或委托给当前生效的外部管理器）。
    pub async fn switch(
        &self,
        rt: &RuntimeType,
        version: &str,
        active_manager: Option<&str>,
    ) -> Result<RuntimeInfo> {
        if let Some(manager) = external_manager(active_manager) {
            manager_executor::switch_version(manager, rt, version)
                .map_err(AppError::InvalidInput)?;
        } else {
            self.manager.switch_version(rt, version).await?;
        }
        Ok(self.detect(rt).await)
    }

    /// 卸载某个版本（或委托给当前生效的外部管理器）。
    pub async fn uninstall(
        &self,
        rt: &RuntimeType,
        version: &str,
        active_manager: Option<&str>,
    ) -> Result<RuntimeInfo> {
        if let Some(manager) = external_manager(active_manager) {
            manager_executor::uninstall_version(manager, rt, version)
                .map_err(AppError::InvalidInput)?;
        } else {
            self.manager.uninstall_version(rt, version).await?;
        }
        Ok(self.detect(rt).await)
    }

    /// 检查所有运行时的版本更新。
    pub async fn check_updates(&self) -> Result<Vec<VersionUpdate>> {
        super::upgrade::check_updates(&self.manager, &self.registry).await
    }

    /// 返回当前运行时安装目录。
    pub async fn install_dir(&self) -> std::path::PathBuf {
        self.manager.get_install_dir().await
    }

    /// 返回所有运行时的默认版本。
    pub async fn defaults(&self) -> HashMap<RuntimeType, String> {
        self.aliases.list_defaults().await
    }
}

/// 返回配置的外部管理器 id（若存在）。
///
/// `"built-in"` 不是外部管理器；它显式选择内置安装器，不能传给
/// `manager_executor`。
pub fn external_manager(active_manager: Option<&str>) -> Option<&str> {
    active_manager.filter(|manager| !manager.is_empty() && *manager != "built-in")
}

/// 解析配置的运行时目录，缺省为 `<data>/agent/runtimes`。
pub fn resolve_install_dir(configured: Option<&Path>) -> PathBuf {
    match configured {
        Some(path) => path.to_path_buf(),
        None => dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("agent")
            .join("runtimes"),
    }
}

fn parse_stored_defaults(stored: &HashMap<String, String>) -> HashMap<RuntimeType, String> {
    stored
        .iter()
        .filter_map(|(runtime, version)| {
            runtime
                .parse::<RuntimeType>()
                .ok()
                .map(|rt| (rt, version.clone()))
        })
        .collect()
}
