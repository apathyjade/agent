//! 版本解析器：把版本规格解析为确切版本。

use std::sync::Arc;

use semver::{Version, VersionReq};

use crate::core::error::{AppError, Result};
use crate::runtime::toolchain::registry::RuntimeRegistry;
use crate::runtime::toolchain::version::compare_versions;
use crate::runtime::toolchain::RuntimeType;

/// 把版本规格（如 "20"、">=18"、"lts"、"stable"）解析为确切版本。
pub struct VersionResolver {
    registry: Arc<RuntimeRegistry>,
}

impl VersionResolver {
    /// 基于版本注册表创建解析器。
    pub fn new(registry: Arc<RuntimeRegistry>) -> Self {
        Self { registry }
    }

    /// 把版本规格解析为确切版本。
    ///
    /// - 精确版本 "20.18.3" → 原样返回
    /// - 别名 "latest" → 拉取版本列表，返回最新的稳定版
    /// - 别名 "lts" → 拉取版本列表，返回最新的 LTS 版本
    /// - 别名 "stable" → 拉取版本列表，返回最新的稳定版
    /// - 范围 ">=18" / "20.x" / "^20.0.0" → 用 semver crate 匹配可用版本
    /// - 仅大版本 "20" → 找到最新的 20.x
    /// - 兜底：没有匹配项时原样返回规格字符串
    ///
    /// 需要版本列表时会访问注册表；注册表内部有 TTL 缓存，因此重复解析
    /// 通常不会重复发起网络请求。
    pub async fn resolve(&self, rt: &RuntimeType, spec: &str) -> Result<String> {
        let spec = spec.trim();

        // 空规格
        if spec.is_empty() {
            return Err(AppError::InvalidInput("版本规格不能为空".into()));
        }

        // 已经是精确版本（X.Y.Z 或 X.Y）
        if is_exact_version(spec) {
            return Ok(spec.to_string());
        }

        // 从注册表拉取可用版本
        let versions = self.registry.get_versions(rt).await?;
        if versions.is_empty() {
            return Err(AppError::NotFound(format!(
                "没有可用的 {} 版本信息",
                rt.display_name()
            )));
        }

        // 处理别名
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
                // 找到带 LTS 代号的最新版本
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

        // 处理 "20"（仅大版本）—— 找到该大版本下的最新版本
        if let Ok(major) = spec.parse::<u64>() {
            if let Some(found) = versions
                .iter()
                .filter(|v| {
                    v.version
                        .split('.')
                        .next()
                        .and_then(|s| s.parse::<u64>().ok())
                        .is_some_and(|m| m == major)
                })
                .max_by(|a, b| compare_versions(&a.version, &b.version))
            {
                return Ok(found.version.clone());
            }
        }

        // 处理 semver 范围：">=18"、"20.x"、"^20.0.0"、">=18 <19" 等
        if let Ok(req) = VersionReq::parse(spec) {
            let matching: Vec<&crate::runtime::toolchain::registry::RuntimeVersion> = versions
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

        // 兜底：原样返回规格
        Ok(spec.to_string())
    }

    /// 判断某个版本是否匹配给定规格。
    pub async fn matches(&self, rt: &RuntimeType, spec: &str, version: &str) -> Result<bool> {
        let spec = spec.trim();
        let version = version.trim();

        // 完全相等
        if spec == version {
            return Ok(true);
        }

        // 别名判断
        match spec.to_lowercase().as_str() {
            "latest" | "stable" => {
                let resolved = self.resolve(rt, spec).await?;
                return Ok(resolved == version);
            }
            "lts" => {
                // 检查注册表中该版本是否带 LTS 标记
                let versions = self.registry.get_versions(rt).await?;
                return Ok(versions
                    .iter()
                    .any(|v| v.version == version && v.lts.is_some()));
            }
            _ => {}
        }

        // 仅大版本："20" 匹配 "20.18.3"
        if let Ok(major) = spec.parse::<u64>() {
            if let Some(v_major) = version
                .split('.')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
            {
                if v_major == major {
                    return Ok(true);
                }
            }
        }

        // semver 范围
        if let Ok(req) = VersionReq::parse(spec) {
            if let Ok(parsed) = Version::parse(version) {
                return Ok(req.matches(&parsed));
            }
        }

        // 尝试部分版本匹配："20.18" 匹配 "20.18.3"
        if version.starts_with(spec) && !spec.ends_with('.') {
            let next = version.chars().nth(spec.len());
            if next == Some('.') || next.is_none() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 建议最佳升级目标。
    /// 返回比当前版本更新的最新版本；已经是最新版时返回 `None`。
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
                    Ok(None) // 已是最新
                }
            }
            None => Ok(None),
        }
    }
}

/// 判断字符串是否形如精确版本（X.Y.Z 或 X.Y）。
fn is_exact_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u64>().is_ok())
}
