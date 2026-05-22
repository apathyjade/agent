// ── Version Lifecycle: Determine support stage for each version ──
//
// Each runtime has a known support lifecycle. This module maps versions
// to their lifecycle stage (Latest, LTS, Active, Maintenance, EOL).

// Public API functions are used by the frontend via IPC.
#![allow(dead_code)]

use crate::environment::RuntimeType;

/// Lifecycle stage of a runtime version.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum VersionLifecycle {
    /// Latest stable release.
    Latest,
    /// Long-term support version.
    Lts { codename: String },
    /// Actively supported (pre-LTS). Used for Node.js odd-numbered majors.
    Active,
    /// In maintenance mode, approaching EOL.
    Maintenance { eol_date: Option<String> },
    /// End-of-life, no longer supported.
    Eol { eol_date: String },
}

impl VersionLifecycle {
    /// Human-readable label in Chinese.
    pub fn label(&self) -> &'static str {
        match self {
            VersionLifecycle::Latest => "最新",
            VersionLifecycle::Lts { .. } => "LTS",
            VersionLifecycle::Active => "活跃",
            VersionLifecycle::Maintenance { .. } => "维护期",
            VersionLifecycle::Eol { .. } => "已停止支持",
        }
    }

    /// Emoji representation for the lifecycle stage.
    pub fn emoji(&self) -> &'static str {
        match self {
            VersionLifecycle::Latest => "🆕",
            VersionLifecycle::Lts { .. } => "✅",
            VersionLifecycle::Active => "🟢",
            VersionLifecycle::Maintenance { .. } => "🟡",
            VersionLifecycle::Eol { .. } => "🔴",
        }
    }

    /// CSS color class for UI display.
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

// ── Node.js Lifecycle ──
//
// Node.js follows a predictable LTS schedule:
// - Even-numbered majors (18, 20, 22) get LTS
// - Odd-numbered majors (19, 21, 23) are Current/Active only
// - LTS releases have codenames

/// Known Node.js LTS codenames mapped to their major versions.
const NODE_LTS_CODENAMES: &[(u64, &str, &str)] = &[
    // (major, codename, eol_date)
    (22, "Jod", "2027-04-30"),
    (20, "Iron", "2026-04-30"),
    (18, "Hydrogen", "2025-04-30"),
    (16, "Gallium", "2024-09-11"),
    (14, "Fermium", "2023-04-30"),
    (12, "Erbium", "2022-04-30"),
    (10, "Dubnium", "2021-04-30"),
];

const NODE_EOL_MAINTENANCE: &[(u64, &str)] = &[
    // (major, maintenance_eol_date)
    (18, "2025-04-30"),
    (20, "2026-04-30"),
    (22, "2027-04-30"),
];

/// Determine Node.js lifecycle stage based on version and LTS codename.
pub fn node_lifecycle(version: &str, lts_codename: Option<&str>) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    let major = parts.first().copied().unwrap_or(0);
    let latest_lts_major = NODE_LTS_CODENAMES.first().map(|(m, _, _)| *m).unwrap_or(0);

    // LTS version with codename
    if let Some(codename) = lts_codename {
        // Check if it's the latest LTS
        if major == latest_lts_major {
            return VersionLifecycle::Lts {
                codename: codename.to_string(),
            };
        }

        // Check maintenance/EOL status
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

        // Old LTS version not in maintenance table (e.g. Node 16 Gallium)
        // — treat as EOL if it's more than 2 versions behind latest
        if major < latest_lts_major - 2 {
            return VersionLifecycle::Eol {
                eol_date: "已结束".to_string(),
            };
        }

        return VersionLifecycle::Lts {
            codename: codename.to_string(),
        };
    }

    // No LTS codename: even major = Active, odd major = EOL
    if major % 2 == 0 {
        VersionLifecycle::Active
    } else {
        VersionLifecycle::Eol {
            eol_date: "已结束".to_string(),
        }
    }
}

// ── Python Lifecycle ──
//
// Python release cycle:
// - Each major.minor gets ~18 months of bugfix releases, then ~18 months of security fixes
// - Total support ~3 years per minor release

/// Known Python EOL dates by minor version.
const PYTHON_EOL_DATES: &[(u64, &str)] = &[
    // (minor, eol_date)
    (7, "2023-06-27"),
    (8, "2024-10-07"),
    (9, "2025-10-05"),
    (10, "2026-10-04"),
    (11, "2027-10-24"),
    (12, "2028-10-02"),
    (13, "2029-10-01"),
];

const PYTHON_MAINTENANCE_START: &[(u64, &str)] = &[
    // (minor, maintenance_start_date)
    (10, "2025-04-07"),
    (11, "2025-04-24"),
    (12, "2026-04-02"),
    (13, "2027-04-01"),
];

/// Determine Python lifecycle stage based on version.
pub fn python_lifecycle(version: &str) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    let minor = parts.get(1).copied().unwrap_or(0);

    // Latest Python version is 3.13
    let latest_minor = 13;
    if minor == latest_minor {
        return VersionLifecycle::Latest;
    }

    // Check EOL status
    for &(eol_minor, eol_date) in PYTHON_EOL_DATES {
        if minor == eol_minor {
            return VersionLifecycle::Eol {
                eol_date: eol_date.to_string(),
            };
        }
    }

    // Check maintenance status
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

    // Active support
    VersionLifecycle::Active
}

// ── Go Lifecycle ──
//
// Go release policy: last two major releases receive security updates.
// As of 2025: Go 1.22 and 1.23 are supported, 1.21 and older are EOL.

const GO_SUPPORTED_MAJORS: &[(u64, u64)] = &[
    // (major, minor) — currently supported
    (1, 24),
    (1, 23),
    (1, 22),
];

const GO_EOL_DATES: &[(u64, u64, &str)] = &[
    // (major, minor, eol_date)
    (1, 21, "2024-08-13"),
    (1, 20, "2024-02-06"),
    (1, 19, "2023-09-06"),
    (1, 18, "2023-02-01"),
];

/// Determine Go lifecycle stage based on version.
pub fn go_lifecycle(version: &str) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    if parts.len() < 2 {
        return VersionLifecycle::Active;
    }
    let major = parts[0];
    let minor = parts[1];

    // Latest Go version
    let latest = GO_SUPPORTED_MAJORS.first().copied();
    if let Some((lmaj, lmin)) = latest {
        if major == lmaj && minor == lmin {
            return VersionLifecycle::Latest;
        }
    }

    // Check if supported (in the supported list)
    let is_supported = GO_SUPPORTED_MAJORS
        .iter()
        .any(|(maj, min)| *maj == major && *min == minor);

    // Check if EOL
    for &(eol_maj, eol_min, eol_date) in GO_EOL_DATES {
        if eol_maj == major && eol_min == minor {
            return VersionLifecycle::Eol {
                eol_date: eol_date.to_string(),
            };
        }
    }

    if is_supported {
        // In the supported list but not the latest = maintenance/active
        VersionLifecycle::Active
    } else {
        // Not in supported list and not explicitly EOL = assume EOL
        VersionLifecycle::Eol {
            eol_date: "未知".to_string(),
        }
    }
}

// ── Rust Lifecycle ──

/// Determine Rust lifecycle stage based on version.
pub fn rust_lifecycle(version: &str) -> VersionLifecycle {
    match version {
        "stable" | "nightly" => VersionLifecycle::Active,
        v if v.starts_with("1.85") || v.starts_with("1.84") => VersionLifecycle::Active,
        v if v.starts_with("1.83") || v.starts_with("1.82") => VersionLifecycle::Maintenance { eol_date: None },
        _ => VersionLifecycle::Active,
    }
}

// ── Java Lifecycle ──

/// Determine Java lifecycle stage based on version.
pub fn java_lifecycle(version: &str) -> VersionLifecycle {
    let major = version.split('.').next().unwrap_or("");
    match major {
        "21" | "17" | "11" | "8" => VersionLifecycle::Lts { codename: format!("JDK {}", major) },
        "23" => VersionLifecycle::Latest,
        _ => VersionLifecycle::Active,
    }
}

// ── Deno Lifecycle ──

/// Deno has a fast-moving release cycle — all versions are considered Active.
pub fn deno_lifecycle(_version: &str) -> VersionLifecycle {
    VersionLifecycle::Active
}

// ── Bun Lifecycle ──

/// Bun is fast-moving — all versions are considered Active.
pub fn bun_lifecycle(_version: &str) -> VersionLifecycle {
    VersionLifecycle::Active
}

// ── Ruby Lifecycle ──
//
// Ruby release policy:
// - New major.minor every ~12 months
// - Each version gets ~2 years of bugfix support, then ~1 year of security maintenance
// - Total lifecycle ~3 years

/// Determine Ruby lifecycle stage based on version.
pub fn ruby_lifecycle(version: &str) -> VersionLifecycle {
    let parts: Vec<u64> = version.split('.').filter_map(|s| s.parse().ok()).collect();
    let minor = parts.get(1).copied().unwrap_or(0);
    match minor {
        4 | 3 => VersionLifecycle::Active,
        2 => VersionLifecycle::Maintenance { eol_date: Some("2026-03-31".into()) },
        1 => VersionLifecycle::Maintenance { eol_date: Some("2025-03-31".into()) },
        _ if minor < 1 => VersionLifecycle::Eol { eol_date: "已结束".into() },
        _ => VersionLifecycle::Active,
    }
}

/// Dispatch to the correct lifecycle function based on runtime type.
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
        // Docker and uv don't have formal lifecycle stages
        RuntimeType::Docker | RuntimeType::Uv => VersionLifecycle::Active,
    }
}
