//! Node.js 工具链兼容矩阵。
//!
//! 维护 Node.js 与包管理器（npm、pnpm、yarn）之间的版本兼容关系，提供
//! 版本推荐与安装状态检查。

use std::str::FromStr;

/// 针对某个 Node.js 版本推荐的 npm / pnpm / yarn 版本。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolchainRecommendation {
    pub node_version: String,
    pub npm: Option<String>,
    pub pnpm: Option<String>,
    pub yarn: Option<String>,
}

/// 当前已安装工具链组件的状态。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolchainStatus {
    pub node: Option<String>,
    pub npm: Option<String>,
    pub pnpm: Option<String>,
    pub yarn: Option<String>,
    /// 推荐版本与实际版本之间的所有不一致。
    pub mismatches: Vec<ToolchainMismatch>,
}

/// 单条版本不一致警告。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolchainMismatch {
    pub tool: String,
    pub expected: String,
    pub actual: String,
    pub severity: MismatchSeverity,
    pub message: String,
}

/// 不一致的严重程度。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MismatchSeverity {
    /// 破坏性：工具可能无法正常工作
    Error,
    /// 非破坏性，但建议修复
    Warning,
    /// 仅提示信息
    Info,
}

/// 轻量的 semver 版本约束（本文件不引入额外依赖）。
#[derive(Debug, Clone)]
pub struct SemverReq {
    raw: String,
}

impl SemverReq {
    /// 用原始约束字符串构造。
    pub fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
        }
    }

    /// 非常基础的 semver 匹配：判断 `version` 是否满足 `raw`。
    ///
    /// 支持 `>=X.Y.Z`、`^X.Y.Z`、`~X.Y.Z`、`>X.Y.Z`、`<X.Y.Z` 与精确版本
    /// `X.Y.Z`。该实现只比较数字段，不处理预发布标签，仅用于兼容性建议。
    pub fn matches(&self, version: &str) -> bool {
        let raw = self.raw.trim();
        let ver_parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
        if ver_parts.len() < 2 {
            return false;
        }

        if let Some(stripped) = raw.strip_prefix(">=") {
            Self::cmp_version(version, stripped) >= std::cmp::Ordering::Greater
                || Self::cmp_version(version, stripped) == std::cmp::Ordering::Equal
        } else if let Some(stripped) = raw.strip_prefix('^') {
            // ^X.Y.Z：第一个非零段固定，其余段只要求不小于约束
            let req_parts: Vec<u64> = stripped.split('.').filter_map(|s| s.parse().ok()).collect();
            if req_parts.is_empty() {
                return false;
            }
            // 主版本必须一致
            if req_parts[0] != ver_parts[0] {
                return false;
            }
            // 约束带次版本时，实际次版本不能更小
            if req_parts.len() > 1 && ver_parts.len() > 1 && req_parts[1] > ver_parts[1] {
                return false;
            }
            true
        } else if let Some(stripped) = raw.strip_prefix('~') {
            // ~X.Y.Z：只允许补丁级变化
            let req_parts: Vec<u64> = stripped.split('.').filter_map(|s| s.parse().ok()).collect();
            if req_parts.len() >= 2 && ver_parts.len() >= 2 {
                req_parts[0] == ver_parts[0] && req_parts[1] == ver_parts[1]
            } else {
                false
            }
        } else if let Some(stripped) = raw.strip_prefix('>') {
            Self::cmp_version(version, stripped) == std::cmp::Ordering::Greater
        } else if let Some(stripped) = raw.strip_prefix('<') {
            Self::cmp_version(version, stripped) == std::cmp::Ordering::Less
        } else {
            // 精确匹配
            Self::cmp_version(version, raw) == std::cmp::Ordering::Equal
        }
    }

    /// 逐段按数值比较两个版本号（升序语义，缺失段按 0 处理）。
    fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
        let a_parts: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
        let b_parts: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
        for i in 0..a_parts.len().max(b_parts.len()) {
            let av = a_parts.get(i).copied().unwrap_or(0);
            let bv = b_parts.get(i).copied().unwrap_or(0);
            match av.cmp(&bv) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    }
}

impl FromStr for SemverReq {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(SemverReq::new(s))
    }
}

/// 兼容条目：Node.js 版本区间 → 工具版本区间。
#[derive(Debug, Clone)]
pub struct CompatEntry {
    pub node_range: SemverReq,
    pub tool_range: SemverReq,
}

/// Node.js ↔ npm/pnpm/yarn 兼容矩阵。
#[derive(Debug, Clone)]
pub struct NodeToolchainMatrix {
    /// npm 版本兼容条目。
    pub npm_compat: Vec<CompatEntry>,
    /// pnpm 版本兼容条目。
    pub pnpm_compat: Vec<CompatEntry>,
    /// yarn 版本兼容条目。
    pub yarn_compat: Vec<CompatEntry>,
}

impl NodeToolchainMatrix {
    /// 内置兼容数据（依据 Node.js 与 npm 官方发布说明整理）。
    ///
    /// Node.js 自带 npm 版本：
    /// - Node 22.x → npm 10.x
    /// - Node 20.x → npm 10.x（20.x 早期为 9.x）
    /// - Node 18.x → npm 9.x（18.x 早期为 8.x）
    /// - Node 16.x → npm 8.x
    ///
    /// pnpm 兼容性：
    /// - pnpm 9.x → Node >=18
    /// - pnpm 8.x → Node >=16
    ///
    /// yarn 兼容性：
    /// - yarn 4.x → Node >=18.12
    /// - yarn 3.x → Node >=12
    pub fn builtin() -> Self {
        Self {
            npm_compat: vec![
                CompatEntry {
                    node_range: SemverReq::new(">=22.0.0"),
                    tool_range: SemverReq::new(">=10.0.0 <11.0.0"),
                },
                CompatEntry {
                    node_range: SemverReq::new(">=20.0.0 <22.0.0"),
                    tool_range: SemverReq::new(">=10.0.0 <11.0.0"),
                },
                CompatEntry {
                    node_range: SemverReq::new(">=18.0.0 <20.0.0"),
                    tool_range: SemverReq::new(">=9.0.0 <10.0.0"),
                },
                CompatEntry {
                    node_range: SemverReq::new(">=16.0.0 <18.0.0"),
                    tool_range: SemverReq::new(">=8.0.0 <9.0.0"),
                },
            ],
            pnpm_compat: vec![
                CompatEntry {
                    node_range: SemverReq::new(">=18.0.0"),
                    tool_range: SemverReq::new(">=9.0.0"),
                },
                CompatEntry {
                    node_range: SemverReq::new(">=16.18.0 <18.0.0"),
                    tool_range: SemverReq::new(">=8.0.0 <9.0.0"),
                },
            ],
            yarn_compat: vec![
                CompatEntry {
                    node_range: SemverReq::new(">=18.12.0"),
                    tool_range: SemverReq::new(">=4.0.0"),
                },
                CompatEntry {
                    node_range: SemverReq::new(">=12.0.0 <18.0.0"),
                    tool_range: SemverReq::new(">=3.0.0 <4.0.0"),
                },
            ],
        }
    }

    /// 针对给定的 Node.js 版本推荐 npm/pnpm/yarn 版本。
    pub fn recommend(&self, node_version: &str) -> ToolchainRecommendation {
        let npm = self
            .npm_compat
            .iter()
            .find(|e| e.node_range.matches(node_version))
            .map(|_| {
                // 在兼容区间内返回一个合理版本
                match node_version {
                    v if v.starts_with("22.") => "10.8.2".to_string(),
                    v if v.starts_with("20.") => "10.8.2".to_string(),
                    v if v.starts_with("18.") => "9.8.1".to_string(),
                    v if v.starts_with("16.") => "8.19.4".to_string(),
                    _ => "latest".to_string(),
                }
            });

        let pnpm = self
            .pnpm_compat
            .iter()
            .find(|e| e.node_range.matches(node_version))
            .map(|_| {
                match node_version {
                    v if v.starts_with("18.") || v.starts_with("20.") || v.starts_with("22.") => {
                        "9.15.4".to_string()
                    }
                    v if v.starts_with("16.") => "8.15.8".to_string(),
                    _ => "latest".to_string(),
                }
            });

        let yarn = self
            .yarn_compat
            .iter()
            .find(|e| e.node_range.matches(node_version))
            .map(|_| {
                match node_version {
                    v if v.starts_with("18.") || v.starts_with("20.") || v.starts_with("22.") => {
                        "4.5.0".to_string()
                    }
                    v if v.starts_with("12.") || v.starts_with("14.") || v.starts_with("16.") => {
                        "3.8.5".to_string()
                    }
                    _ => "latest".to_string(),
                }
            });

        ToolchainRecommendation {
            node_version: node_version.to_string(),
            npm,
            pnpm,
            yarn,
        }
    }

    /// 对照推荐值检查已安装的工具链版本。
    pub fn check_status(
        &self,
        node_version: &str,
        npm_version: Option<&str>,
        pnpm_version: Option<&str>,
        yarn_version: Option<&str>,
    ) -> ToolchainStatus {
        let rec = self.recommend(node_version);
        let mut mismatches = Vec::new();

        // 检查 npm
        if let Some(actual) = npm_version {
            if let Some(ref expected) = rec.npm {
                if actual != expected.as_str() {
                    mismatches.push(ToolchainMismatch {
                        tool: "npm".to_string(),
                        expected: expected.clone(),
                        actual: actual.to_string(),
                        severity: MismatchSeverity::Warning,
                        message: format!(
                            "npm {} 可能不与 Node {} 捆绑的版本 ({}) 完全兼容",
                            actual, node_version, expected
                        ),
                    });
                }
            }
        }

        // 检查 pnpm
        if let Some(actual) = pnpm_version {
            let compatible = self
                .pnpm_compat
                .iter()
                .any(|e| e.node_range.matches(node_version));
            if !compatible {
                mismatches.push(ToolchainMismatch {
                    tool: "pnpm".to_string(),
                    expected: ">=9.0.0 (for Node >=18)".to_string(),
                    actual: actual.to_string(),
                    severity: MismatchSeverity::Error,
                    message: format!("pnpm {} 可能不兼容 Node {}", actual, node_version),
                });
            }
        }

        // 检查 yarn
        if let Some(actual) = yarn_version {
            let compatible = self
                .yarn_compat
                .iter()
                .any(|e| e.node_range.matches(node_version));
            if !compatible {
                mismatches.push(ToolchainMismatch {
                    tool: "yarn".to_string(),
                    expected: ">=4.0.0 (for Node >=18.12)".to_string(),
                    actual: actual.to_string(),
                    severity: MismatchSeverity::Warning,
                    message: format!(
                        "yarn {} 可能不兼容 Node {}，建议升级 yarn",
                        actual, node_version
                    ),
                });
            }
        }

        ToolchainStatus {
            node: Some(node_version.to_string()),
            npm: npm_version.map(|s| s.to_string()),
            pnpm: pnpm_version.map(|s| s.to_string()),
            yarn: yarn_version.map(|s| s.to_string()),
            mismatches,
        }
    }
}

// ── 工具链版本检测 ──

use std::process::Stdio;
use tokio::process::Command;

/// 通过运行 CLI 命令检测已安装的工具链版本。
///
/// 会依次执行 `node --version`、`npm --version` 等命令；命令缺失或执行失败
/// 时对应字段为 `None`，不会返回错误。
pub async fn detect_toolchain_versions() -> ToolchainStatus {
    let node_ver = run_and_parse(&["node", "--version"]).await;
    let npm_ver = run_and_parse(&["npm", "--version"]).await;
    let pnpm_ver = run_and_parse(&["pnpm", "--version"]).await;
    let yarn_ver = run_and_parse(&["yarn", "--version"]).await;

    let matrix = NodeToolchainMatrix::builtin();
    matrix.check_status(
        node_ver.as_deref().unwrap_or("unknown"),
        npm_ver.as_deref(),
        pnpm_ver.as_deref(),
        yarn_ver.as_deref(),
    )
}

async fn run_and_parse(cmd_and_args: &[&str]) -> Option<String> {
    let (cmd, args) = cmd_and_args.split_first()?;
    let out = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if out.status.success() {
        let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let ver = ver.trim_start_matches('v').to_string();
        if ver.is_empty() {
            None
        } else {
            Some(ver)
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_req_basic() {
        let req = SemverReq::new(">=20.0.0");
        assert!(req.matches("20.0.0"));
        assert!(req.matches("22.14.0"));
        assert!(!req.matches("18.20.7"));
    }

    #[test]
    fn test_semver_req_caret() {
        let req = SemverReq::new("^18.0.0");
        assert!(req.matches("18.20.7"));
        assert!(req.matches("18.5.0"));
        assert!(!req.matches("20.0.0"));
    }

    #[test]
    fn test_semver_req_tilde() {
        let req = SemverReq::new("~20.18.0");
        assert!(req.matches("20.18.3"));
        assert!(!req.matches("20.19.0"));
    }

    #[test]
    fn test_recommend_for_node_22() {
        let matrix = NodeToolchainMatrix::builtin();
        let rec = matrix.recommend("22.14.0");
        assert_eq!(rec.npm.as_deref(), Some("10.8.2"));
        assert_eq!(rec.pnpm.as_deref(), Some("9.15.4"));
        assert_eq!(rec.yarn.as_deref(), Some("4.5.0"));
    }

    #[test]
    fn test_recommend_for_node_18() {
        let matrix = NodeToolchainMatrix::builtin();
        let rec = matrix.recommend("18.20.7");
        assert_eq!(rec.npm.as_deref(), Some("9.8.1"));
        assert_eq!(rec.pnpm.as_deref(), Some("9.15.4"));
        assert_eq!(rec.yarn.as_deref(), Some("4.5.0"));
    }

    #[test]
    fn test_check_status_match() {
        let matrix = NodeToolchainMatrix::builtin();
        let status = matrix.check_status("22.14.0", Some("10.8.2"), Some("9.15.4"), Some("4.5.0"));
        assert!(status.mismatches.is_empty());
    }

    #[test]
    fn test_check_status_mismatch() {
        let matrix = NodeToolchainMatrix::builtin();
        let status = matrix.check_status(
            "22.14.0",
            Some("9.8.1"), // Node 22 上使用 npm 9 → 不一致
            None,
            None,
        );
        assert_eq!(status.mismatches.len(), 1);
        assert_eq!(status.mismatches[0].tool, "npm");
    }
}
