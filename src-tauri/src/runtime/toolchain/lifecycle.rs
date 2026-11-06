//! 版本生命周期：判断每个版本所处的支持阶段。
//!
//! 每个运行时都有已知的支持生命周期。本模块把版本映射到生命周期阶段
//! （Latest、LTS、Active、Maintenance、EOL）。

// 这些公开 API 由前端通过 IPC 使用。
#![allow(dead_code)]

use crate::runtime::toolchain::RuntimeType;

/// 运行时版本的生命周期阶段。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum VersionLifecycle {
    /// 最新稳定版本。
    Latest,
    /// 长期支持版本。
    Lts { codename: String },
    /// 活跃支持期（尚未进入 LTS）。用于 Node.js 的奇数大版本。
    Active,
    /// 维护期，临近 EOL。
    Maintenance { eol_date: Option<String> },
    /// 已停止支持。
    Eol { eol_date: String },
}

impl VersionLifecycle {
    /// 面向用户的中文标签。
    pub fn label(&self) -> &'static str {
        match self {
            VersionLifecycle::Latest => "最新",
            VersionLifecycle::Lts { .. } => "LTS",
            VersionLifecycle::Active => "活跃",
            VersionLifecycle::Maintenance { .. } => "维护期",
            VersionLifecycle::Eol { .. } => "已停止支持",
        }
    }

    /// 生命周期阶段对应的 emoji。
    pub fn emoji(&self) -> &'static str {
        match self {
            VersionLifecycle::Latest => "🆕",
            VersionLifecycle::Lts { .. } => "✅",
            VersionLifecycle::Active => "🟢",
            VersionLifecycle::Maintenance { .. } => "🟡",
            VersionLifecycle::Eol { .. } => "🔴",
        }
    }

    /// UI 展示用的 CSS 颜色类名。
    pub fn color_class(&self) -> &'static str {
        match self {
            VersionLifecycle::Latest => "text-green-600",
            VersionLifecycle::Lts { .. } => "text-blue-600",
            VersionLifecycle::Active => "text-green-500",
            VersionLifecycle::Maintenance { .. } => "text-yellow-600",
            VersionLifecycle::Eol { .. } => "text-red-600",
        }
    }
}

// ── Node.js 生命周期 ──
//
// Node.js 的 LTS 计划是可预测的：
// - 偶数大版本（18、20、22）会进入 LTS
// - 奇数大版本（19、21、23）只停留在 Current/Active
// - LTS 版本有代号

/// Node.js 已知 LTS 代号及其大版本。
///
/// 按“最新 → 最旧”排列，因此第一项被当作最新 LTS 大版本。元组含义为
/// `(大版本, 代号, EOL 日期)`。
const NODE_LTS_CODENAMES: &[(u64, &str, &str)] = &[
    // (大版本, 代号, EOL 日期)
    (22, "Jod", "2027-04-30"),
    (20, "Iron", "2026-04-30"),
    (18, "Hydrogen", "2025-04-30"),
    (16, "Gallium", "2024-09-11"),
    (14, "Fermium", "2023-04-30"),
    (12, "Erbium", "2022-04-30"),
    (10, "Dubnium", "2021-04-30"),
];

/// Node.js 进入维护期的 LTS 大版本及其 EOL 日期。
///
/// 元组含义为 `(大版本, 维护期 EOL 日期)`；用于区分“维护期”与“已停止支持”。
const NODE_EOL_MAINTENANCE: &[(u64, &str)] = &[
    // (大版本, 维护期 EOL 日期)
    (18, "2025-04-30"),
    (20, "2026-04-30"),
    (22, "2027-04-30"),
];

/// 根据版本号与 LTS 代号判断 Node.js 生命周期阶段。
pub fn node_lifecycle(version: &str, lts_codename: Option<&str>) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    let major = parts.first().copied().unwrap_or(0);
    let latest_lts_major = NODE_LTS_CODENAMES.first().map(|(m, _, _)| *m).unwrap_or(0);

    // 带代号的 LTS 版本
    if let Some(codename) = lts_codename {
        // 判断是否为最新 LTS
        if major == latest_lts_major {
            return VersionLifecycle::Lts {
                codename: codename.to_string(),
            };
        }

        // 检查维护期 / EOL 状态
        for &(eol_major, eol_date) in NODE_EOL_MAINTENANCE {
            if major == eol_major {
                if major < latest_lts_major - 2 {
                    return VersionLifecycle::Eol {
                        eol_date: eol_date.to_string(),
                    };
                }
                return VersionLifecycle::Maintenance {
                    eol_date: Some(eol_date.to_string()),
                };
            }
        }

        // 不在维护期表中的旧 LTS（例如 Node 16 Gallium）
        // —— 落后最新版超过 2 个大版本时视为 EOL
        if major < latest_lts_major - 2 {
            return VersionLifecycle::Eol {
                eol_date: "已结束".to_string(),
            };
        }

        return VersionLifecycle::Lts {
            codename: codename.to_string(),
        };
    }

    // 没有 LTS 代号：偶数大版本为 Active，奇数大版本为 EOL
    if major % 2 == 0 {
        VersionLifecycle::Active
    } else {
        VersionLifecycle::Eol {
            eol_date: "已结束".to_string(),
        }
    }
}

// ── Python 生命周期 ──
//
// Python 发布节奏：
// - 每个 major.minor 先经历约 18 个月的 bugfix 发布，再进入约 18 个月的安全修复
// - 每个 minor 版本总支持期约 3 年

/// Python 各 minor 版本的 EOL 日期。
///
/// 元组含义为 `(minor, EOL 日期)`，仅比较版本号的第二段。
const PYTHON_EOL_DATES: &[(u64, &str)] = &[
    // (minor, EOL 日期)
    (7, "2023-06-27"),
    (8, "2024-10-07"),
    (9, "2025-10-05"),
    (10, "2026-10-04"),
    (11, "2027-10-24"),
    (12, "2028-10-02"),
    (13, "2029-10-01"),
];

/// Python 各 minor 版本进入安全维护期的日期。
///
/// 元组含义为 `(minor, 维护期开始日期)`。
const PYTHON_MAINTENANCE_START: &[(u64, &str)] = &[
    // (minor, 维护期开始日期)
    (10, "2025-04-07"),
    (11, "2025-04-24"),
    (12, "2026-04-02"),
    (13, "2027-04-01"),
];

/// 根据版本号判断 Python 生命周期阶段。
pub fn python_lifecycle(version: &str) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    let minor = parts.get(1).copied().unwrap_or(0);

    // 当前最新 Python 版本是 3.13
    let latest_minor = 13;
    if minor == latest_minor {
        return VersionLifecycle::Latest;
    }

    // 检查 EOL 状态
    for &(eol_minor, eol_date) in PYTHON_EOL_DATES {
        if minor == eol_minor {
            return VersionLifecycle::Eol {
                eol_date: eol_date.to_string(),
            };
        }
    }

    // 检查维护期状态
    for &(maint_minor, _) in PYTHON_MAINTENANCE_START {
        if minor == maint_minor {
            return VersionLifecycle::Maintenance {
                eol_date: PYTHON_EOL_DATES
                    .iter()
                    .find(|(m, _)| *m == minor)
                    .map(|(_, d)| d.to_string()),
            };
        }
    }

    // 活跃支持期
    VersionLifecycle::Active
}

// ── Go 生命周期 ──
//
// Go 发布策略：最近两个大版本仍接收安全更新。
// 截至 2025 年：Go 1.22 与 1.23 受支持，1.21 及更早版本已 EOL。

/// Go 当前仍受支持的 `(major, minor)` 列表。
///
/// 按“最新 → 最旧”排列，第一项视为最新版本。
const GO_SUPPORTED_MAJORS: &[(u64, u64)] = &[
    // (major, minor) —— 当前受支持
    (1, 24),
    (1, 23),
    (1, 22),
];

/// Go 已停止支持版本的 EOL 日期表。
///
/// 元组含义为 `(major, minor, EOL 日期)`。
const GO_EOL_DATES: &[(u64, u64, &str)] = &[
    // (major, minor, EOL 日期)
    (1, 21, "2024-08-13"),
    (1, 20, "2024-02-06"),
    (1, 19, "2023-09-06"),
    (1, 18, "2023-02-01"),
];

/// 根据版本号判断 Go 生命周期阶段。
pub fn go_lifecycle(version: &str) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    if parts.len() < 2 {
        return VersionLifecycle::Active;
    }
    let major = parts[0];
    let minor = parts[1];

    // 最新 Go 版本
    let latest = GO_SUPPORTED_MAJORS.first().copied();
    if let Some((lmaj, lmin)) = latest {
        if major == lmaj && minor == lmin {
            return VersionLifecycle::Latest;
        }
    }

    // 是否在受支持列表中
    let is_supported = GO_SUPPORTED_MAJORS
        .iter()
        .any(|(maj, min)| *maj == major && *min == minor);

    // 检查是否 EOL
    for &(eol_maj, eol_min, eol_date) in GO_EOL_DATES {
        if eol_maj == major && eol_min == minor {
            return VersionLifecycle::Eol {
                eol_date: eol_date.to_string(),
            };
        }
    }

    if is_supported {
        // 在支持列表中但不是最新 = 维护 / 活跃
        VersionLifecycle::Active
    } else {
        // 不在支持列表且未显式标记 EOL = 假定已 EOL
        VersionLifecycle::Eol {
            eol_date: "未知".to_string(),
        }
    }
}

// ── Rust 生命周期 ──

/// 根据版本号判断 Rust 生命周期阶段。
pub fn rust_lifecycle(version: &str) -> VersionLifecycle {
    match version {
        "stable" | "nightly" => VersionLifecycle::Active,
        v if v.starts_with("1.85") || v.starts_with("1.84") => VersionLifecycle::Active,
        v if v.starts_with("1.83") || v.starts_with("1.82") => {
            VersionLifecycle::Maintenance { eol_date: None }
        }
        _ => VersionLifecycle::Active,
    }
}

// ── Java 生命周期 ──

/// 根据版本号判断 Java 生命周期阶段。
pub fn java_lifecycle(version: &str) -> VersionLifecycle {
    let major = version.split('.').next().unwrap_or("");
    match major {
        "21" | "17" | "11" | "8" => VersionLifecycle::Lts {
            codename: format!("JDK {}", major),
        },
        "23" => VersionLifecycle::Latest,
        _ => VersionLifecycle::Active,
    }
}

// ── Deno 生命周期 ──

/// Deno 发布节奏很快 —— 所有版本都视为 Active。
pub fn deno_lifecycle(_version: &str) -> VersionLifecycle {
    VersionLifecycle::Active
}

// ── Bun 生命周期 ──

/// Bun 发布节奏很快 —— 所有版本都视为 Active。
pub fn bun_lifecycle(_version: &str) -> VersionLifecycle {
    VersionLifecycle::Active
}

// ── Ruby 生命周期 ──
//
// Ruby 发布策略：
// - 每约 12 个月发布新的 major.minor
// - 每个版本约 2 年 bugfix 支持，之后约 1 年安全维护
// - 总生命周期约 3 年

/// 根据版本号判断 Ruby 生命周期阶段。
pub fn ruby_lifecycle(version: &str) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    let minor = parts.get(1).copied().unwrap_or(0);
    match minor {
        4 | 3 => VersionLifecycle::Active,
        2 => VersionLifecycle::Maintenance {
            eol_date: Some("2026-03-31".into()),
        },
        1 => VersionLifecycle::Maintenance {
            eol_date: Some("2025-03-31".into()),
        },
        _ if minor < 1 => VersionLifecycle::Eol {
            eol_date: "已结束".into(),
        },
        _ => VersionLifecycle::Active,
    }
}

/// 按运行时类型分发到对应的生命周期函数。
pub fn for_runtime(rt: &RuntimeType, version: &str, lts: Option<&str>) -> VersionLifecycle {
    match rt {
        RuntimeType::Node => node_lifecycle(version, lts),
        RuntimeType::Python => python_lifecycle(version),
        RuntimeType::Go => go_lifecycle(version),
        RuntimeType::Rust => rust_lifecycle(version),
        RuntimeType::Java => java_lifecycle(version),
        RuntimeType::Deno => deno_lifecycle(version),
        RuntimeType::Bun => bun_lifecycle(version),
        RuntimeType::Ruby => ruby_lifecycle(version),
        // Docker 与 uv 没有正式的生命周期阶段
        RuntimeType::Docker | RuntimeType::Uv => VersionLifecycle::Active,
    }
}
