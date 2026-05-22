use std::process::Command;
use crate::environment::RuntimeType;

pub type ManagerResult<T> = std::result::Result<T, String>;

fn run_cmd(cmd: &str, args: &[&str]) -> ManagerResult<String> {
    let output = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("执行命令失败 '{}': {}", cmd, e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("命令 '{}' 失败: {}", cmd, if stderr.is_empty() { "未知错误" } else { &stderr }))
    }
}

fn install_node(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "fnm" => {
            run_cmd("fnm", &["install", version])?;
            Ok(())
        }
        "nvm" => {
            if cfg!(target_os = "windows") {
                run_cmd("nvm", &["install", version])?;
            } else {
                run_cmd("bash", &["-c", &format!(". \"$NVM_DIR/nvm.sh\" && nvm install {}", version)])?;
            }
            Ok(())
        }
        "volta" => {
            run_cmd("volta", &["install", &format!("node@{}", version)])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Node.js 管理器: {}", manager)),
    }
}

fn install_python(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "uv" => {
            run_cmd("uv", &["python", "install", version])?;
            Ok(())
        }
        "pyenv" => {
            run_cmd("pyenv", &["install", version])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Python 管理器: {}", manager)),
    }
}

fn install_rust(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "rustup" => {
            run_cmd("rustup", &["toolchain", "install", version])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Rust 管理器: {}", manager)),
    }
}

fn switch_node(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "fnm" => {
            run_cmd("fnm", &["use", version])?;
            Ok(())
        }
        "nvm" => {
            if cfg!(target_os = "windows") {
                run_cmd("nvm", &["use", version])?;
            } else {
                run_cmd("bash", &["-c", &format!(". \"$NVM_DIR/nvm.sh\" && nvm use {}", version)])?;
            }
            Ok(())
        }
        "volta" => {
            run_cmd("volta", &["use", &format!("node@{}", version)])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Node.js 管理器: {}", manager)),
    }
}

fn switch_python(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "uv" => {
            run_cmd("uv", &["python", "pin", version])?;
            Ok(())
        }
        "pyenv" => {
            run_cmd("pyenv", &["global", version])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Python 管理器: {}", manager)),
    }
}

fn switch_rust(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "rustup" => {
            run_cmd("rustup", &["default", version])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Rust 管理器: {}", manager)),
    }
}

fn uninstall_node(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "fnm" => {
            run_cmd("fnm", &["uninstall", version])?;
            Ok(())
        }
        "nvm" => {
            if cfg!(target_os = "windows") {
                run_cmd("nvm", &["uninstall", version])?;
            } else {
                run_cmd("bash", &["-c", &format!(". \"$NVM_DIR/nvm.sh\" && nvm uninstall {}", version)])?;
            }
            Ok(())
        }
        "volta" => {
            run_cmd("volta", &["uninstall", &format!("node@{}", version)])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Node.js 管理器: {}", manager)),
    }
}

fn uninstall_python(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "uv" => {
            run_cmd("uv", &["python", "uninstall", version])?;
            Ok(())
        }
        "pyenv" => {
            run_cmd("pyenv", &["uninstall", version])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Python 管理器: {}", manager)),
    }
}

fn uninstall_rust(manager: &str, version: &str) -> ManagerResult<()> {
    match manager {
        "rustup" => {
            run_cmd("rustup", &["toolchain", "uninstall", version])?;
            Ok(())
        }
        _ => Err(format!("不支持的 Rust 管理器: {}", manager)),
    }
}

pub fn install_version(manager: &str, rt: &RuntimeType, version: &str) -> ManagerResult<()> {
    match rt {
        RuntimeType::Node => install_node(manager, version),
        RuntimeType::Python => install_python(manager, version),
        RuntimeType::Rust => install_rust(manager, version),
        _ => Err(format!("不支持通过外部管理器安装 {}", rt.display_name())),
    }
}

pub fn switch_version(manager: &str, rt: &RuntimeType, version: &str) -> ManagerResult<()> {
    match rt {
        RuntimeType::Node => switch_node(manager, version),
        RuntimeType::Python => switch_python(manager, version),
        RuntimeType::Rust => switch_rust(manager, version),
        _ => Err(format!("不支持通过外部管理器切换 {}", rt.display_name())),
    }
}

pub fn uninstall_version(manager: &str, rt: &RuntimeType, version: &str) -> ManagerResult<()> {
    match rt {
        RuntimeType::Node => uninstall_node(manager, version),
        RuntimeType::Python => uninstall_python(manager, version),
        RuntimeType::Rust => uninstall_rust(manager, version),
        _ => Err(format!("不支持通过外部管理器卸载 {}", rt.display_name())),
    }
}
