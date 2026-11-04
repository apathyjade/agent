// ── Runtime Services: shared composition root ──
//
// The app and the CLI both build the toolchain stack through this module, so
// install/switch/uninstall semantics cannot drift between the two frontends.
// This module deliberately has no dependency on the app's config, database or
// UI state; callers pass a plain `RuntimeOptions` value.

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

/// Settings needed to build the toolchain stack.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
    /// Custom install directory. Defaults to `<data>/agent/runtimes`.
    pub install_dir: Option<PathBuf>,
    /// Optional HTTP proxy for downloads.
    pub download_proxy: Option<String>,
    /// Persisted default version per runtime, keyed by runtime id.
    pub defaults: HashMap<String, String>,
}

/// Runtime manager, version registry, resolver and alias store.
#[derive(Clone)]
pub struct RuntimeServices {
    pub manager: Arc<RuntimeManager>,
    pub registry: Arc<RuntimeRegistry>,
    pub resolver: Arc<VersionResolver>,
    pub aliases: Arc<AliasManager>,
}

impl RuntimeServices {
    /// Build the toolchain stack from plain options.
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

    pub async fn detect(&self, rt: &RuntimeType) -> RuntimeInfo {
        self.manager.detect(rt).await
    }

    pub async fn detect_all(&self) -> Vec<RuntimeInfo> {
        self.manager.detect_all().await
    }

    pub async fn resolve(&self, rt: &RuntimeType, spec: &str) -> Result<String> {
        self.resolver.resolve(rt, spec).await
    }

    /// Install a runtime version.
    ///
    /// - An external manager (fnm/nvm/uv/pyenv/rustup) is used only when one is
    ///   active *and* a version was requested. Otherwise the built-in installer
    ///   runs.
    /// - `active_manager = Some("built-in")` always takes the built-in path.
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

    /// Activate an installed version (or delegate to the active external manager).
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

    /// Uninstall a version (or delegate to the active external manager).
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

    pub async fn check_updates(&self) -> Result<Vec<VersionUpdate>> {
        super::upgrade::check_updates(&self.manager, &self.registry).await
    }

    pub async fn install_dir(&self) -> std::path::PathBuf {
        self.manager.get_install_dir().await
    }

    pub async fn defaults(&self) -> HashMap<RuntimeType, String> {
        self.aliases.list_defaults().await
    }
}

/// Return the external manager id, if one is configured.
///
/// `"built-in"` is not an external manager; it explicitly selects the built-in
/// installer and must not be passed to `manager_executor`.
pub fn external_manager(active_manager: Option<&str>) -> Option<&str> {
    active_manager.filter(|manager| !manager.is_empty() && *manager != "built-in")
}

/// Resolve the configured runtime directory, falling back to `<data>/agent/runtimes`.
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
