//! 外部版本管理器探测：列出可用的管理器及其安装状态。

use crate::runtime::toolchain::RuntimeType;
use serde::{Deserialize, Serialize};

/// 一个外部版本管理器的描述信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManager {
    /// 管理器标识（如 "fnm"、"built-in"）。
    pub id: String,
    /// 展示名称。
    pub display_name: String,
    /// 所属运行时。
    pub runtime_type: RuntimeType,
    /// 是否已安装。
    pub installed: bool,
    /// 安装路径（已安装时）。
    pub install_path: Option<String>,
    /// 管理器版本号。
    pub version: Option<String>,
    /// 是否支持通过该管理器安装版本。
    pub can_install: bool,
    /// 安装指引（URL 或说明）。
    pub install_guide: Option<String>,
    /// 是否为推荐管理器。
    pub recommended: bool,
    #[serde(default)]
    pub install_url: Option<String>,
}

/// 检查命令是否存在，并返回 `(路径, 版本号)`。
fn check_command(cmd: &str, args: &[&str]) -> Option<(String, String)> {
    let which = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let which_output = std::process::Command::new(which)
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !which_output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&which_output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();

    let ver_output = std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let version = String::from_utf8_lossy(&ver_output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let version = if version.is_empty() {
        None
    } else {
        Some(version)
    };

    Some((path, version.unwrap_or_else(|| "未知".to_string())))
}

fn detect_node_managers() -> Vec<VersionManager> {
    let mut managers = Vec::new();

    // fnm
    if let Some((path, version)) = check_command("fnm", &["--version"]) {
        managers.push(VersionManager {
            id: "fnm".to_string(),
            display_name: "fnm (Fast Node Manager)".to_string(),
            runtime_type: RuntimeType::Node,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: true,
            install_guide: None,
            recommended: true,
            install_url: Some("https://fnm.vercel.app/".to_string()),
        });
    } else {
        managers.push(VersionManager {
            id: "fnm".to_string(),
            display_name: "fnm (Fast Node Manager)".to_string(),
            runtime_type: RuntimeType::Node,
            installed: false,
            install_path: None,
            version: None,
            can_install: true,
            install_guide: None,
            recommended: true,
            install_url: Some("https://fnm.vercel.app/".to_string()),
        });
    }

    // nvm（macOS/Linux）或 nvm-windows
    let nvm_cmd = "nvm";
    if let Some((path, version)) = check_command(nvm_cmd, &["--version"]) {
        managers.push(VersionManager {
            id: "nvm".to_string(),
            display_name: if cfg!(target_os = "windows") {
                "nvm-windows".to_string()
            } else {
                "nvm".to_string()
            },
            runtime_type: RuntimeType::Node,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: false,
            install_guide: Some(if cfg!(target_os = "windows") {
                "https://github.com/coreybutler/nvm-windows".to_string()
            } else {
                "https://github.com/nvm-sh/nvm".to_string()
            }),
            recommended: false,
            install_url: None,
        });
    } else {
        managers.push(VersionManager {
            id: "nvm".to_string(),
            display_name: if cfg!(target_os = "windows") {
                "nvm-windows".to_string()
            } else {
                "nvm".to_string()
            },
            runtime_type: RuntimeType::Node,
            installed: false,
            install_path: None,
            version: None,
            can_install: false,
            install_guide: Some(if cfg!(target_os = "windows") {
                "https://github.com/coreybutler/nvm-windows".to_string()
            } else {
                "https://github.com/nvm-sh/nvm".to_string()
            }),
            recommended: false,
            install_url: None,
        });
    }

    // volta
    if let Some((path, version)) = check_command("volta", &["--version"]) {
        managers.push(VersionManager {
            id: "volta".to_string(),
            display_name: "Volta".to_string(),
            runtime_type: RuntimeType::Node,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: false,
            install_guide: Some("https://volta.sh/".to_string()),
            recommended: false,
            install_url: None,
        });
    } else {
        managers.push(VersionManager {
            id: "volta".to_string(),
            display_name: "Volta".to_string(),
            runtime_type: RuntimeType::Node,
            installed: false,
            install_path: None,
            version: None,
            can_install: false,
            install_guide: Some("https://volta.sh/".to_string()),
            recommended: false,
            install_url: None,
        });
    }

    managers
}

fn detect_python_managers() -> Vec<VersionManager> {
    let mut managers = Vec::new();

    // uv
    if let Some((path, version)) = check_command("uv", &["--version"]) {
        managers.push(VersionManager {
            id: "uv".to_string(),
            display_name: "uv (Python Package Manager)".to_string(),
            runtime_type: RuntimeType::Python,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: true,
            install_guide: None,
            recommended: true,
            install_url: Some("https://docs.astral.sh/uv/".to_string()),
        });
    } else {
        managers.push(VersionManager {
            id: "uv".to_string(),
            display_name: "uv (Python Package Manager)".to_string(),
            runtime_type: RuntimeType::Python,
            installed: false,
            install_path: None,
            version: None,
            can_install: true,
            install_guide: None,
            recommended: true,
            install_url: Some("https://docs.astral.sh/uv/".to_string()),
        });
    }

    // pyenv
    if let Some((path, version)) = check_command("pyenv", &["--version"]) {
        managers.push(VersionManager {
            id: "pyenv".to_string(),
            display_name: "pyenv".to_string(),
            runtime_type: RuntimeType::Python,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: false,
            install_guide: Some("https://github.com/pyenv/pyenv".to_string()),
            recommended: false,
            install_url: None,
        });
    } else {
        managers.push(VersionManager {
            id: "pyenv".to_string(),
            display_name: "pyenv".to_string(),
            runtime_type: RuntimeType::Python,
            installed: false,
            install_path: None,
            version: None,
            can_install: false,
            install_guide: Some("https://github.com/pyenv/pyenv".to_string()),
            recommended: false,
            install_url: None,
        });
    }

    managers
}

fn detect_rust_managers() -> Vec<VersionManager> {
    let mut managers = Vec::new();

    // rustup
    if let Some((path, version)) = check_command("rustup", &["--version"]) {
        managers.push(VersionManager {
            id: "rustup".to_string(),
            display_name: "rustup".to_string(),
            runtime_type: RuntimeType::Rust,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: true,
            install_guide: None,
            recommended: true,
            install_url: Some("https://rustup.rs/".to_string()),
        });
    } else {
        managers.push(VersionManager {
            id: "rustup".to_string(),
            display_name: "rustup".to_string(),
            runtime_type: RuntimeType::Rust,
            installed: false,
            install_path: None,
            version: None,
            can_install: true,
            install_guide: None,
            recommended: true,
            install_url: Some("https://rustup.rs/".to_string()),
        });
    }

    managers
}

fn detect_go_managers() -> Vec<VersionManager> {
    let mut managers = Vec::new();

    // gvm
    if let Some((path, version)) = check_command("gvm", &["version"]) {
        managers.push(VersionManager {
            id: "gvm".to_string(),
            display_name: "gvm (Go Version Manager)".to_string(),
            runtime_type: RuntimeType::Go,
            installed: true,
            install_path: Some(path),
            version: Some(version),
            can_install: false,
            install_guide: Some("https://github.com/moovweb/gvm".to_string()),
            recommended: true,
            install_url: None,
        });
    } else {
        managers.push(VersionManager {
            id: "gvm".to_string(),
            display_name: "gvm (Go Version Manager)".to_string(),
            runtime_type: RuntimeType::Go,
            installed: false,
            install_path: None,
            version: None,
            can_install: false,
            install_guide: Some("https://github.com/moovweb/gvm".to_string()),
            recommended: true,
            install_url: None,
        });
    }

    managers
}

/// 构造内置管理器的描述项。
fn builtin_manager(rt: RuntimeType) -> VersionManager {
    VersionManager {
        id: "built-in".to_string(),
        display_name: format!("内置管理 ({})", rt.display_name()),
        runtime_type: rt,
        installed: true,
        install_path: None,
        version: None,
        can_install: false,
        install_guide: None,
        recommended: false,
        install_url: None,
    }
}

/// 检测某个运行时可用的外部管理器，并附带内置管理器。
///
/// 只做本地命令探测，不访问网络；未安装的管理器也会返回（`installed = false`），
/// 供 UI 展示安装指引。
pub fn detect_managers(rt: &RuntimeType) -> Vec<VersionManager> {
    let mut managers = match rt {
        RuntimeType::Node => detect_node_managers(),
        RuntimeType::Python => detect_python_managers(),
        RuntimeType::Rust => detect_rust_managers(),
        RuntimeType::Go => detect_go_managers(),
        _ => vec![],
    };
    managers.push(builtin_manager(rt.clone()));
    managers
}

/// 检测所有运行时的外部管理器。
pub fn detect_all_managers() -> Vec<VersionManager> {
    let mut all = Vec::new();
    for rt in RuntimeType::all() {
        all.extend(detect_managers(rt));
    }
    all
}
