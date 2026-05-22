// ── Node.js Toolchain Compatibility Matrix ──
//
// Manages version compatibility between Node.js and its package managers
// (npm, pnpm, yarn). Provides recommendations and verification.

use std::str::FromStr;

/// npm/pnpm/yarn version recommendation for a given Node.js version.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolchainRecommendation {
    pub node_version: String,
    pub npm: Option<String>,
    pub pnpm: Option<String>,
    pub yarn: Option<String>,
}

/// Current status of installed toolchain components.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolchainStatus {
    pub node: Option<String>,
    pub npm: Option<String>,
    pub pnpm: Option<String>,
    pub yarn: Option<String>,
    /// Any mismatches between recommended and actual versions.
    pub mismatches: Vec<ToolchainMismatch>,
}

/// A specific version mismatch warning.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolchainMismatch {
    pub tool: String,
    pub expected: String,
    pub actual: String,
    pub severity: MismatchSeverity,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MismatchSeverity {
    /// Breaking: tool may not work correctly
    Error,
    /// Non-breaking but recommended to fix
    Warning,
    /// Informational
    Info,
}

/// A semver version requirement (lightweight, no external dep needed in this file).
#[derive(Debug, Clone)]
pub struct SemverReq {
    raw: String,
}

impl SemverReq {
    pub fn new(raw: &str) -> Self {
        Self { raw: raw.to_string() }
    }

    /// Very basic semver comparison: checks if `version` satisfies `raw`.
    /// Supports: ">=X.Y.Z", "^X.Y.Z", "~X.Y.Z", ">X.Y.Z", "<X.Y.Z", "X.Y.Z"
    pub fn matches(&self, version: &str) -> bool {
        let raw = self.raw.trim();
        let ver_parts: Vec<u64> = version
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        if ver_parts.len() < 2 {
            return false;
        }

        if let Some(stripped) = raw.strip_prefix(">=") {
            Self::cmp_version(version, stripped) >= std::cmp::Ordering::Greater
                || Self::cmp_version(version, stripped) == std::cmp::Ordering::Equal
        } else if let Some(stripped) = raw.strip_prefix('^') {
            // ^X.Y.Z: first non-zero component is fixed
            let req_parts: Vec<u64> = stripped
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
            if req_parts.is_empty() {
                return false;
            }
            // Major must match
            if req_parts[0] != ver_parts[0] {
                return false;
            }
            // If req has minor, version must be >= req minor
            if req_parts.len() > 1 && ver_parts.len() > 1 {
                if req_parts[1] > ver_parts[1] {
                    return false;
                }
            }
            true
        } else if let Some(stripped) = raw.strip_prefix('~') {
            // ~X.Y.Z: patch-level changes only
            let req_parts: Vec<u64> = stripped
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
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
            // Exact match
            Self::cmp_version(version, raw) == std::cmp::Ordering::Equal
        }
    }

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

/// Compatibility entry: Node.js range → tool version range.
#[derive(Debug, Clone)]
pub struct CompatEntry {
    pub node_range: SemverReq,
    pub tool_range: SemverReq,
}

/// Node.js ↔ npm/pnpm/yarn compatibility matrix.
#[derive(Debug, Clone)]
pub struct NodeToolchainMatrix {
    /// npm version compatibility entries.
    pub npm_compat: Vec<CompatEntry>,
    /// pnpm version compatibility entries.
    pub pnpm_compat: Vec<CompatEntry>,
    /// yarn version compatibility entries.
    pub yarn_compat: Vec<CompatEntry>,
}

impl NodeToolchainMatrix {
    /// Built-in compatibility data (informed by official Node.js + npm release notes).
    ///
    /// Node.js bundled npm versions:
    /// - Node 22.x → npm 10.x
    /// - Node 20.x → npm 10.x (9.x in early 20.x)
    /// - Node 18.x → npm 9.x (8.x in early 18.x)
    /// - Node 16.x → npm 8.x
    ///
    /// pnpm compatibility:
    /// - pnpm 9.x → Node >=18
    /// - pnpm 8.x → Node >=16
    ///
    /// yarn compatibility:
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

    /// Recommend npm/pnpm/yarn versions for a given Node.js version.
    pub fn recommend(&self, node_version: &str) -> ToolchainRecommendation {
        let npm = self.npm_compat.iter()
            .find(|e| e.node_range.matches(node_version))
            .and_then(|_| {
                // Return a reasonable version within the compatible range
                Some(match node_version {
                    v if v.starts_with("22.") => "10.8.2".to_string(),
                    v if v.starts_with("20.") => "10.8.2".to_string(),
                    v if v.starts_with("18.") => "9.8.1".to_string(),
                    v if v.starts_with("16.") => "8.19.4".to_string(),
                    _ => "latest".to_string(),
                })
            });

        let pnpm = self.pnpm_compat.iter()
            .find(|e| e.node_range.matches(node_version))
            .and_then(|_| {
                Some(match node_version {
                    v if v.starts_with("18.") || v.starts_with("20.") || v.starts_with("22.") => "9.15.4".to_string(),
                    v if v.starts_with("16.") => "8.15.8".to_string(),
                    _ => "latest".to_string(),
                })
            });

        let yarn = self.yarn_compat.iter()
            .find(|e| e.node_range.matches(node_version))
            .and_then(|_| {
                Some(match node_version {
                    v if v.starts_with("18.") || v.starts_with("20.") || v.starts_with("22.") => "4.5.0".to_string(),
                    v if v.starts_with("12.") || v.starts_with("14.") || v.starts_with("16.") => "3.8.5".to_string(),
                    _ => "latest".to_string(),
                })
            });

        ToolchainRecommendation {
            node_version: node_version.to_string(),
            npm,
            pnpm,
            yarn,
        }
    }

    /// Check installed toolchain against recommendations.
    pub fn check_status(
        &self,
        node_version: &str,
        npm_version: Option<&str>,
        pnpm_version: Option<&str>,
        yarn_version: Option<&str>,
    ) -> ToolchainStatus {
        let rec = self.recommend(node_version);
        let mut mismatches = Vec::new();

        // Check npm
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

        // Check pnpm
        if let Some(actual) = pnpm_version {
            let compatible = self.pnpm_compat.iter()
                .any(|e| e.node_range.matches(node_version));
            if !compatible {
                mismatches.push(ToolchainMismatch {
                    tool: "pnpm".to_string(),
                    expected: ">=9.0.0 (for Node >=18)".to_string(),
                    actual: actual.to_string(),
                    severity: MismatchSeverity::Error,
                    message: format!(
                        "pnpm {} 可能不兼容 Node {}",
                        actual, node_version
                    ),
                });
            }
        }

        // Check yarn
        if let Some(actual) = yarn_version {
            let compatible = self.yarn_compat.iter()
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

// ── Toolchain Version Detection ──

use tokio::process::Command;
use std::process::Stdio;

/// Detect installed toolchain versions by running CLI commands.
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
        if ver.is_empty() { None } else { Some(ver) }
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
        let status = matrix.check_status(
            "22.14.0",
            Some("10.8.2"),
            Some("9.15.4"),
            Some("4.5.0"),
        );
        assert!(status.mismatches.is_empty());
    }

    #[test]
    fn test_check_status_mismatch() {
        let matrix = NodeToolchainMatrix::builtin();
        let status = matrix.check_status(
            "22.14.0",
            Some("9.8.1"), // npm 9 on Node 22 → mismatch
            None,
            None,
        );
        assert_eq!(status.mismatches.len(), 1);
        assert_eq!(status.mismatches[0].tool, "npm");
    }
}
