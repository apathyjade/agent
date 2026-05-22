// ── Version Resolver: Resolve version specs to exact versions ──

use std::sync::Arc;

use semver::{Version, VersionReq};

use crate::environment::registry::RuntimeRegistry;
use crate::environment::RuntimeType;
use crate::error::{AppError, Result};

/// Resolves version specifications (e.g. "20", ">=18", "lts", "stable") to exact versions.
pub struct VersionResolver {
    registry: Arc<RuntimeRegistry>,
}

impl VersionResolver {
    pub fn new(registry: Arc<RuntimeRegistry>) -> Self {
        Self { registry }
    }

    /// Resolve a version spec to an exact version.
    ///
    /// - Exact version "20.18.3" → return as-is
    /// - Alias "latest" → fetch versions, return newest
    /// - Alias "lts" → fetch versions, find one with lts field, return newest LTS
    /// - Alias "stable" → fetch versions, return newest stable
    /// - Range ">=18" or "20.x" or "^20.0.0" → use semver crate to match against available versions
    /// - Major only "20" → find latest 20.x
    /// - Fallback: if nothing matches, return spec as-is
    pub async fn resolve(&self, rt: &RuntimeType, spec: &str) -> Result<String> {
        let spec = spec.trim();

        // Empty spec
        if spec.is_empty() {
            return Err(AppError::InvalidInput("版本规格不能为空".into()));
        }

        // Already looks like an exact version (X.Y.Z or X.Y)
        if is_exact_version(spec) {
            return Ok(spec.to_string());
        }

        // Fetch available versions from registry
        let versions = self.registry.get_versions(rt).await?;
        if versions.is_empty() {
            return Err(AppError::NotFound(format!(
                "没有可用的 {} 版本信息",
                rt.display_name()
            )));
        }

        // Handle aliases
        match spec.to_lowercase().as_str() {
            "latest" => {
                return versions
                    .iter()
                    .filter(|v| v.is_stable)
                    .max_by(|a, b| compare_versions(&a.version, &b.version))
                    .map(|v| v.version.clone())
                    .ok_or_else(|| AppError::NotFound("未找到最新版本".into()));
            }
            "lts" => {
                // Find versions with LTS codename, pick newest
                return versions
                    .iter()
                    .filter(|v| v.lts.is_some())
                    .max_by(|a, b| compare_versions(&a.version, &b.version))
                    .map(|v| v.version.clone())
                    .ok_or_else(|| AppError::NotFound("未找到 LTS 版本".into()));
            }
            "stable" => {
                return versions
                    .iter()
                    .filter(|v| v.is_stable)
                    .max_by(|a, b| compare_versions(&a.version, &b.version))
                    .map(|v| v.version.clone())
                    .ok_or_else(|| AppError::NotFound("未找到稳定版本".into()));
            }
            _ => {}
        }

        // Handle "20" (major only) — find latest in that major
        if let Ok(major) = spec.parse::<u64>() {
            if let Some(found) = versions
                .iter()
                .filter(|v| {
                    v.version
                        .split('.')
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .map_or(false, |m| m == major)
                })
                .max_by(|a, b| compare_versions(&a.version, &b.version))
            {
                return Ok(found.version.clone());
            }
        }

        // Handle semver ranges: ">=18", "20.x", "^20.0.0", ">=18 <19", etc.
        if let Ok(req) = VersionReq::parse(spec) {
            let matching: Vec<&crate::environment::registry::RuntimeVersion> = versions
                .iter()
                .filter(|v| {
                    Version::parse(&v.version)
                        .map(|parsed| req.matches(&parsed))
                        .unwrap_or(false)
                })
                .collect();

            if let Some(best) = matching
                .iter()
                .max_by(|a, b| compare_versions(&a.version, &b.version))
            {
                return Ok(best.version.clone());
            }
        }

        // Fallback: return spec as-is
        Ok(spec.to_string())
    }

    /// Check if a version matches a spec.
    pub async fn matches(&self, rt: &RuntimeType, spec: &str, version: &str) -> Result<bool> {
        let spec = spec.trim();
        let version = version.trim();

        // Exact match
        if spec == version {
            return Ok(true);
        }

        // Alias checks
        match spec.to_lowercase().as_str() {
            "latest" | "stable" => {
                let resolved = self.resolve(rt, spec).await?;
                return Ok(resolved == version);
            }
            "lts" => {
                // Check if version has an LTS tag in the registry
                let versions = self.registry.get_versions(rt).await?;
                return Ok(versions.iter().any(|v| v.version == version && v.lts.is_some()));
            }
            _ => {}
        }

        // Major-only: "20" matches "20.18.3"
        if let Ok(major) = spec.parse::<u64>() {
            if let Some(v_major) = version.split('.').next().and_then(|s| s.parse::<u64>().ok()) {
                if v_major == major {
                    return Ok(true);
                }
            }
        }

        // Semver range
        if let Ok(req) = VersionReq::parse(spec) {
            if let Ok(parsed) = Version::parse(version) {
                return Ok(req.matches(&parsed));
            }
        }

        // Try partial version matching: "20.18" matches "20.18.3"
        if version.starts_with(spec) && !spec.ends_with('.') {
            let next = version.chars().nth(spec.len());
            if next == Some('.') || next == None {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Suggest the best upgrade target.
    /// Returns the newest version that's newer than current, or None if already on latest.
    pub async fn suggest_upgrade(&self, rt: &RuntimeType, current: &str) -> Result<Option<String>> {
        let versions = self.registry.get_versions(rt).await?;
        if versions.is_empty() {
            return Ok(None);
        }

        let newest = versions
            .iter()
            .filter(|v| v.is_stable)
            .max_by(|a, b| compare_versions(&a.version, &b.version));

        match newest {
            Some(nv) => {
                if compare_versions(&nv.version, current).is_gt() {
                    Ok(Some(nv.version.clone()))
                } else {
                    Ok(None) // Already on latest
                }
            }
            None => Ok(None),
        }
    }
}

/// Check if a string looks like an exact version (X.Y.Z or X.Y).
fn is_exact_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u64>().is_ok())
}

/// Compare two version strings using semver, falling back to string comparison.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    }
}
