//! 版本号比较与排序工具。
//!
//! 这里集中了工具链模块中所有版本号比较逻辑，避免同一算法散落在各个
//! 版本源里。两套语义必须严格区分：
//!
//! - [`compare_version_desc`] / [`sort_versions_desc`]：逐段按数值比较，
//!   用于把版本列表排成“从新到旧”。它不理解预发布标签，`20.18.3` 与
//!   `20.9.0` 会按数值正确排序，这是 `semver` crate 做不到的（后者要求
//!   完整的三段式版本号）。
//! - [`compare_versions`]：优先使用 `semver` crate，解析失败时退化为
//!   字符串比较，用于判断“当前版本 vs 最新版本”这类宽松场景。

use std::cmp::Ordering;

use semver::Version;

use super::registry::RuntimeVersion;

/// 按“逐段数值”语义比较两个版本号，返回降序排序所需的顺序。
///
/// 规则与历史实现完全一致：按 `.` 切分，只保留能解析为 `u64` 的段，
/// 逐段比较；第一个不相等的段决定结果，且数值大的排在前面。所有公共段
/// 都相等时，段数多的版本排在前面（例如 `20.18.0` 先于 `20.18`）。
///
/// 非数字段会被直接丢弃（例如 `1.2.0-beta` 等价于 `1.2`），因此该函数
/// 只适合做展示排序，不用于精确的语义化版本判定。
pub fn compare_version_desc(a: &str, b: &str) -> Ordering {
    let a_parts: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let b_parts: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for (av, bv) in a_parts.iter().zip(b_parts.iter()) {
        if av != bv {
            return bv.cmp(av);
        }
    }
    b_parts.len().cmp(&a_parts.len())
}

/// 就地把版本列表按版本号从新到旧排序。
///
/// 使用 [`compare_version_desc`] 的逐段数值语义，排序结果与各版本源原先
/// 内联的排序代码逐字节一致。`sort_by` 是稳定排序：比较结果相等的版本
/// 会保持输入顺序。
pub fn sort_versions_desc(versions: &mut [RuntimeVersion]) {
    versions.sort_by(|a, b| compare_version_desc(&a.version, &b.version));
}

/// 比较两个版本字符串：优先按 `semver` 解析，失败时退化为字符串比较。
///
/// 该函数只用于“谁更新”这类宽松判断，不保证对 `20.18.3` 之外的非常规
/// 版本号（如 `stable`、`nightly`、`3.12.x`）有语义，此时两侧都会解析
/// 失败并走字符串比较。
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::toolchain::RuntimeType;

    fn version(value: &str) -> RuntimeVersion {
        RuntimeVersion {
            runtime_type: RuntimeType::Node,
            version: value.to_string(),
            display_name: value.to_string(),
            url: String::new(),
            lts: None,
            is_stable: true,
            release_date: None,
            file_size: None,
        }
    }

    #[test]
    fn sort_orders_versions_descending() {
        let mut versions = vec![
            version("20.9.0"),
            version("20.18.3"),
            version("18.20.7"),
        ];
        sort_versions_desc(&mut versions);
        let ordered: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(ordered, vec!["20.18.3", "20.9.0", "18.20.7"]);
    }

    #[test]
    fn sort_handles_different_component_counts() {
        let mut versions = vec![version("20.18"), version("20.18.0"), version("20.9")];
        sort_versions_desc(&mut versions);
        let ordered: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        // 公共段相同时，段数更多的版本排在前面。
        assert_eq!(ordered, vec!["20.18.0", "20.18", "20.9"]);
    }

    #[test]
    fn sort_ignores_non_numeric_components() {
        let mut versions = vec![version("1.2.0-beta"), version("1.2.1")];
        sort_versions_desc(&mut versions);
        let ordered: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        // "1.2.0-beta" 的末段不是数字，被丢弃后等价于 "1.2"。
        assert_eq!(ordered, vec!["1.2.1", "1.2.0-beta"]);
    }

    #[test]
    fn compare_versions_prefers_semver_then_falls_back() {
        assert_eq!(compare_versions("20.18.3", "20.9.0"), Ordering::Greater);
        // 非 semver 版本退化为字符串比较："nightly" < "stable"。
        assert_eq!(compare_versions("nightly", "stable"), Ordering::Less);
    }
}
