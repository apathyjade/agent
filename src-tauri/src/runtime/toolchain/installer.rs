//! 运行时安装器。
//!
//! 按版本安装运行时，版本存放于 `{install_dir}/{rt}/{version}/`，并通过
//! 清单文件 `.manifest.json` 记录当前生效版本。

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use futures::StreamExt;

use crate::core::error::Result;
use crate::runtime::toolchain::platform;
use crate::runtime::toolchain::RuntimeType::*;
use crate::runtime::toolchain::{
    read_manifest, write_manifest, AvailableVersion, InstallProgress, RuntimeInfo, RuntimeManifest,
    RuntimeSource, RuntimeType,
};

/// 负责下载、解压并登记运行时版本的安装器。
pub struct RuntimeInstaller {
    install_dir: PathBuf,
}

impl RuntimeInstaller {
    /// 用指定的运行时根目录创建安装器。
    pub fn new(install_dir: PathBuf) -> Self {
        Self { install_dir }
    }

    /// 更新运行时根目录（不会移动已安装的文件）。
    pub fn set_runtimes_dir(&mut self, new_dir: PathBuf) {
        self.install_dir = new_dir;
    }

    /// 安装运行时的指定版本。
    ///
    /// `version` 为 `None` 时按各运行时约定安装 "latest"。下载与解压是异步
    /// 网络操作，进度通过 `on_progress` 回调同步推送；安装失败时会自动清理
    /// 未完成的版本目录。
    pub async fn install(
        &self,
        rt: &RuntimeType,
        version: Option<String>,
        _install_dir: PathBuf, // caller passes the current dir
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        match rt {
            Node => {
                self.install_portable(rt.clone(), version, "node", node_install_info, on_progress)
                    .await
            }
            Uv => {
                self.install_portable(rt.clone(), version, "uv", uv_install_info, on_progress)
                    .await
            }
            Python => {
                self.install_portable(
                    rt.clone(),
                    version,
                    "python",
                    python_install_info,
                    on_progress,
                )
                .await
            }
            Go => {
                self.install_portable(rt.clone(), version, "go", go_install_info, on_progress)
                    .await
            }
            Docker => self.install_docker(on_progress).await,
            Rust => self.install_rust(rt.clone(), version, on_progress).await,
            Java => {
                self.install_portable(rt.clone(), version, "java", java_install_info, on_progress)
                    .await
            }
            Deno => {
                self.install_portable(rt.clone(), version, "deno", deno_install_info, on_progress)
                    .await
            }
            Bun => {
                self.install_portable(rt.clone(), version, "bun", bun_install_info, on_progress)
                    .await
            }
            Ruby => self.install_ruby(rt.clone(), version, on_progress).await,
        }
    }

    async fn install_portable<F: Fn(&str) -> Option<RuntimeDownloadInfo>>(
        &self,
        rt: RuntimeType,
        version: Option<String>,
        dir_name: &str,
        info_fn: F,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        let install_dir = &self.install_dir;
        let rt_dir = install_dir.join(dir_name);

        // 决定要安装的版本
        let version_str = version.clone().unwrap_or_else(|| "latest".to_string());

        // 获取下载信息（支持 "latest" 或具体版本）
        let dl_info = info_fn(&version_str).ok_or_else(|| {
            crate::core::error::AppError::InvalidInput(format!(
                "不支持的版本: {}. 请选择可用版本后重试",
                version_str
            ))
        })?;

        let display = rt.display_name();
        let ver_dir = rt_dir.join(&dl_info.version);

        // 守卫：失败时清理未完成的安装目录
        let mut _cleanup = CleanupGuard::new(ver_dir.clone());

        emit_progress(
            &on_progress,
            rt.clone(),
            "downloading",
            0.0,
            &format!("正在下载 {} {}...", display, dl_info.version),
        );

        let bytes = download_file(&dl_info.url, &on_progress, rt.clone()).await?;

        emit_progress(&on_progress, rt.clone(), "extracting", 0.9, "正在解压...");

        // 清空版本目录并重新创建
        if ver_dir.exists() {
            std::fs::remove_dir_all(&ver_dir)?;
        }
        std::fs::create_dir_all(&rt_dir)?;

        // 解压
        if let Some(subdir) = dl_info.extract_subdir {
            extract_archive(&bytes, &rt_dir, dl_info.ext)?;
            let extracted = rt_dir.join(subdir);
            if extracted.exists() && extracted != ver_dir {
                rename_cross_device(&extracted, &ver_dir)?;
            }
        } else {
            extract_archive(&bytes, &ver_dir, dl_info.ext)?;
        }

        // 设置可执行权限
        #[cfg(not(target_os = "windows"))]
        for exe_name in &dl_info.executables {
            let candidates = [ver_dir.join(exe_name), ver_dir.join("bin").join(exe_name)];
            for p in &candidates {
                if p.exists() {
                    let _ = make_executable(p.to_str().unwrap_or(""));
                }
            }
        }

        // 更新清单：把该版本设为生效版本并加入版本列表
        let manifest_path = rt_dir.join(".manifest.json");
        let mut manifest = read_manifest(&manifest_path).unwrap_or_else(|| RuntimeManifest {
            active_version: None,
            versions: HashMap::new(),
        });
        manifest.active_version = Some(dl_info.version.clone());
        manifest.versions.insert(
            dl_info.version.clone(),
            crate::runtime::toolchain::manifest::VersionInfo {
                path: dl_info.version.clone(),
                installed_at: Utc::now().to_rfc3339(),
            },
        );
        write_manifest(&manifest_path, &manifest).map_err(crate::core::error::AppError::Io)?;

        emit_progress(
            &on_progress,
            rt.clone(),
            "verifying",
            1.0,
            &format!("{} {} 安装完成", display, dl_info.version),
        );

        // 解除清理守卫：安装成功
        _cleanup.disarm();

        // 通过简单检测返回更新后的 RuntimeInfo
        Ok(RuntimeInfo {
            runtime_type: rt.clone(),
            display_name: format!("{} (内置)", display),
            source: RuntimeSource::BuiltIn,
            version: Some(dl_info.version.clone()),
            installed_versions: vec![],
            executable_path: None,
            error: None,
            available: true,
        })
    }

    async fn install_rust(
        &self,
        rt: RuntimeType,
        version: Option<String>,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        let version_str = version.unwrap_or_else(|| "stable".to_string());

        emit_progress(
            &on_progress,
            rt.clone(),
            "checking",
            0.0,
            "正在检查 rustup...",
        );

        // 检查 rustup 是否可用
        let rustup_check = tokio::process::Command::new("rustup")
            .arg("--version")
            .output()
            .await;

        match rustup_check {
            Ok(output) if output.status.success() => {
                emit_progress(
                    &on_progress,
                    rt.clone(),
                    "installing",
                    0.3,
                    &format!("正在通过 rustup 安装 Rust {}...", version_str),
                );

                // 安装请求的工具链
                let install_result = tokio::process::Command::new("rustup")
                    .args(["toolchain", "install", &version_str])
                    .output()
                    .await;

                match install_result {
                    Ok(inst) if inst.status.success() => {
                        emit_progress(
                            &on_progress,
                            rt.clone(),
                            "setting-default",
                            0.7,
                            "正在设置默认工具链...",
                        );

                        // 设为默认工具链
                        let _ = tokio::process::Command::new("rustup")
                            .args(["default", &version_str])
                            .output()
                            .await;

                        emit_progress(
                            &on_progress,
                            rt.clone(),
                            "done",
                            1.0,
                            &format!("Rust {} 安装完成", version_str),
                        );

                        Ok(RuntimeInfo {
                            runtime_type: rt,
                            display_name: "Rust (rustup)".to_string(),
                            source: RuntimeSource::BuiltIn,
                            version: Some(version_str),
                            installed_versions: vec![],
                            executable_path: None,
                            error: None,
                            available: true,
                        })
                    }
                    Ok(inst) => {
                        let stderr = String::from_utf8_lossy(&inst.stderr);
                        Err(crate::core::error::AppError::InvalidInput(format!(
                            "rustup 安装失败: {}",
                            stderr
                        )))
                    }
                    Err(e) => Err(crate::core::error::AppError::InvalidInput(format!(
                        "执行 rustup 失败: {}",
                        e
                    ))),
                }
            }
            _ => {
                // rustup 不可用
                Err(crate::core::error::AppError::InvalidInput(
                    "请通过 https://rustup.rs 安装 rustup".to_string(),
                ))
            }
        }
    }

    async fn install_docker(
        &self,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        let is_windows = cfg!(target_os = "windows");
        let is_macos = cfg!(target_os = "macos");

        let (url, filename) = if is_windows {
            (
                "https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe"
                    .to_string(),
                "Docker Desktop Installer.exe".to_string(),
            )
        } else if is_macos {
            (
                "https://desktop.docker.com/mac/main/arm64/Docker.dmg".to_string(),
                "Docker.dmg".to_string(),
            )
        } else {
            return Err(crate::core::error::AppError::InvalidInput(
                "Linux 请通过包管理器安装 Docker：curl -fsSL https://get.docker.com | sh"
                    .to_string(),
            ));
        };

        let docker_dir = self.install_dir.join("docker");
        let installer_path = docker_dir.join(&filename);

        emit_progress(
            &on_progress,
            Docker,
            "downloading",
            0.0,
            "正在下载 Docker Desktop 安装程序...",
        );

        let bytes = download_file(&url, &on_progress, Docker).await?;

        emit_progress(&on_progress, Docker, "saving", 0.9, "正在保存安装程序...");

        if docker_dir.exists() {
            std::fs::remove_dir_all(&docker_dir)?;
        }
        std::fs::create_dir_all(&docker_dir)?;
        std::fs::write(&installer_path, &bytes)?;

        emit_progress(
            &on_progress,
            Docker,
            "done",
            1.0,
            &format!(
                "Docker Desktop 安装程序已下载到:\n{}\n请手动运行安装",
                installer_path.display()
            ),
        );

        Ok(RuntimeInfo {
            runtime_type: Docker,
            display_name: "Docker (内置)".to_string(),
            source: RuntimeSource::BuiltIn,
            version: None,
            installed_versions: vec![],
            executable_path: Some(installer_path.to_string_lossy().to_string()),
            error: Some("请手动运行安装程序".to_string()),
            available: false,
        })
    }

    async fn install_ruby(
        &self,
        rt: RuntimeType,
        version: Option<String>,
        on_progress: impl Fn(InstallProgress) + Send + 'static,
    ) -> Result<RuntimeInfo> {
        let _version_str = version.unwrap_or_else(|| "3.4.2".to_string());

        emit_progress(
            &on_progress,
            rt.clone(),
            "checking",
            0.0,
            "正在检查系统 Ruby...",
        );

        // Ruby 难以作为便携二进制安装，
        // 建议使用 rbenv、RVM 或系统包管理器。
        Err(crate::core::error::AppError::InvalidInput(
            "请通过 rbenv 或 RVM 安装 Ruby，或访问 https://www.ruby-lang.org/".to_string(),
        ))
    }
}

// ── 可下载版本 ──

/// 返回某个运行时可直接下载的版本列表。
///
/// 注意：这里返回的是**离线静态清单**，不访问网络；实时版本请使用
/// `sources` 中的版本源。Docker 通过独立安装程序安装，因此返回空列表。
pub fn available_versions(rt: &RuntimeType) -> Vec<AvailableVersion> {
    match rt {
        Node => node_versions(),
        Uv => uv_versions(),
        Python => python_versions(),
        Go => go_versions(),
        Rust => rust_versions(),
        Java => java_versions(),
        Deno => deno_versions(),
        Docker => vec![], // Docker 通过安装程序下载
        Bun => bun_versions(),
        Ruby => ruby_versions(),
    }
}

// 下面 9 个函数结构相似，但每个运行时的下载地址模板、默认版本与资产命名
// 规则都不同（例如 Java 每个大版本对应不同仓库，Ruby 使用源码包，Rust 由
// rustup 管理没有下载地址）。因此保留为独立函数，而不是强行合并成一张表：
// 一旦某个运行时的 URL 规则变化，只需修改对应函数，避免表格驱动带来的隐式
// 假设。平台相关部分统一委托给 `platform` 模块。

fn node_versions() -> Vec<AvailableVersion> {
    let arch = platform::node_dist_platform();
    let ext = platform::archive_ext(&Node);
    // 已知的 LTS 版本
    ["22.14.0", "20.18.3", "18.20.7"]
        .iter()
        .map(|v| {
            let url = format!(
                "https://nodejs.org/dist/v{}/node-v{}-{}.{}",
                v, v, arch, ext
            );
            AvailableVersion {
                version: v.to_string(),
                display_name: format!("Node.js v{}", v),
                url,
            }
        })
        .collect()
}

fn uv_versions() -> Vec<AvailableVersion> {
    let arch = platform::rust_triple();
    let ext = platform::archive_ext(&Uv);
    ["0.6.2", "0.5.29", "0.5.0"]
        .iter()
        .map(|v| {
            let url = format!(
                "https://github.com/astral-sh/uv/releases/download/{}/uv-{}.{}",
                v, arch, ext
            );
            AvailableVersion {
                version: v.to_string(),
                display_name: format!("uv v{}", v),
                url,
            }
        })
        .collect()
}

fn python_versions() -> Vec<AvailableVersion> {
    let tag = "20260510";
    let arch_label = platform::rust_triple();
    let ext = platform::archive_ext(&Python);
    ["3.10.20", "3.11.12", "3.12.10"]
        .iter()
        .map(|v| {
            let archive_name = format!("cpython-{}+{}-{}-install_only", v, tag, arch_label);
            let url = format!(
                "https://github.com/astral-sh/python-build-standalone/releases/download/{}/{}.{}",
                tag, archive_name, ext
            );
            AvailableVersion {
                version: v.to_string(),
                display_name: format!("Python {}", v),
                url,
            }
        })
        .collect()
}

fn go_versions() -> Vec<AvailableVersion> {
    let arch = format!(
        "{}-{}",
        platform::go_platform_os(),
        platform::go_platform_arch()
    );
    let ext = platform::archive_ext(&Go);
    ["1.22.4", "1.21.13", "1.20.14"]
        .iter()
        .map(|v| {
            let url = format!("https://dl.google.com/go/go{}.{}.{}", v, arch, ext);
            AvailableVersion {
                version: v.to_string(),
                display_name: format!("Go {}", v),
                url,
            }
        })
        .collect()
}

fn rust_versions() -> Vec<AvailableVersion> {
    // Rust 由 rustup 管理 —— 没有直接下载地址
    // 返回 rustup 可安装的工具链名称
    vec![
        AvailableVersion {
            version: "stable".to_string(),
            display_name: "Rust stable".to_string(),
            url: String::new(),
        },
        AvailableVersion {
            version: "nightly".to_string(),
            display_name: "Rust nightly".to_string(),
            url: String::new(),
        },
        AvailableVersion {
            version: "1.85.0".to_string(),
            display_name: "Rust 1.85.0".to_string(),
            url: String::new(),
        },
        AvailableVersion {
            version: "1.84.1".to_string(),
            display_name: "Rust 1.84.1".to_string(),
            url: String::new(),
        },
        AvailableVersion {
            version: "1.83.0".to_string(),
            display_name: "Rust 1.83.0".to_string(),
            url: String::new(),
        },
    ]
}

fn java_versions() -> Vec<AvailableVersion> {
    // 来自 Adoptium 的 JDK 版本
    let os = platform::java_platform_os();
    let arch = "x64";
    let ext = platform::archive_ext(&Java);
    // 已知 JDK 版本及其下载地址
    vec![
        AvailableVersion {
            version: "21.0.7".to_string(),
            display_name: "JDK 21.0.7 (LTS)".to_string(),
            url: format!("https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.7%2B9/OpenJDK21U-jdk_{}_{}_hotspot_21.0.7_9.{}", arch, os, ext),
        },
        AvailableVersion {
            version: "17.0.14".to_string(),
            display_name: "JDK 17.0.14 (LTS)".to_string(),
            url: format!("https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.14%2B7/OpenJDK17U-jdk_{}_{}_hotspot_17.0.14_7.{}", arch, os, ext),
        },
        AvailableVersion {
            version: "11.0.26".to_string(),
            display_name: "JDK 11.0.26 (LTS)".to_string(),
            url: format!("https://github.com/adoptium/temurin11-binaries/releases/download/jdk-11.0.26%2B4/OpenJDK11U-jdk_{}_{}_hotspot_11.0.26_4.{}", arch, os, ext),
        },
        AvailableVersion {
            version: "8.0.442".to_string(),
            display_name: "JDK 8.0.442 (LTS)".to_string(),
            url: format!("https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u442-b06/OpenJDK8U-jdk_{}_{}_hotspot_8u442b06.{}", arch, os, ext),
        },
    ]
}

fn bun_versions() -> Vec<AvailableVersion> {
    let asset = platform::bun_asset_name_for_installer();
    ["1.2.5", "1.1.42", "1.0.36", "0.8.1"]
        .iter()
        .map(|v| {
            let url = format!(
                "https://github.com/oven-sh/bun/releases/download/bun-v{}/{}",
                v, asset
            );
            AvailableVersion {
                version: v.to_string(),
                display_name: format!("Bun v{}", v),
                url,
            }
        })
        .collect()
}

fn ruby_versions() -> Vec<AvailableVersion> {
    vec![
        AvailableVersion {
            version: "3.4.2".to_string(),
            display_name: "Ruby 3.4.2".to_string(),
            url: "https://cache.ruby-lang.org/pub/ruby/3.4/ruby-3.4.2.tar.gz".to_string(),
        },
        AvailableVersion {
            version: "3.3.7".to_string(),
            display_name: "Ruby 3.3.7".to_string(),
            url: "https://cache.ruby-lang.org/pub/ruby/3.3/ruby-3.3.7.tar.gz".to_string(),
        },
    ]
}

fn deno_versions() -> Vec<AvailableVersion> {
    let suffix = platform::deno_asset_suffix_for_installer();
    ["2.2.0", "2.1.10", "2.0.6", "1.46.3"]
        .iter()
        .map(|v| {
            let url = format!(
                "https://github.com/denoland/deno/releases/download/v{}/deno-{}",
                v, suffix
            );
            AvailableVersion {
                version: v.to_string(),
                display_name: format!("Deno {}", v),
                url,
            }
        })
        .collect()
}

// ── 下载描述符 ──

/// 单个版本的下载描述信息。
struct RuntimeDownloadInfo {
    /// 下载地址（随平台变化）。
    url: String,
    /// 实际安装的版本号（"latest" 已解析为具体版本）。
    version: String,
    /// 压缩包扩展名。
    ext: &'static str,
    /// 解压后需要重命名的子目录（部分归档会多包一层）。
    extract_subdir: Option<String>,
    /// 仅 Unix 下读取，用于给解压出的二进制加可执行权限。
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    executables: Vec<&'static str>,
}

fn node_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let arch = platform::node_dist_platform();
    let ext = platform::archive_ext(&Node);
    let v = if version == "latest" {
        "20.18.3"
    } else {
        version
    };
    let subdir = format!("node-v{}-{}", v, arch);
    Some(RuntimeDownloadInfo {
        url: format!(
            "https://nodejs.org/dist/v{}/node-v{}-{}.{}",
            v, v, arch, ext
        ),
        version: v.to_string(),
        ext,
        extract_subdir: Some(subdir),
        executables: vec!["node"],
    })
}

fn uv_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let arch = platform::rust_triple();
    let ext = platform::archive_ext(&Uv);
    let v = if version == "latest" {
        "0.6.2"
    } else {
        version
    };
    Some(RuntimeDownloadInfo {
        url: format!(
            "https://github.com/astral-sh/uv/releases/download/{}/uv-{}.{}",
            v, arch, ext
        ),
        version: v.to_string(),
        ext,
        extract_subdir: None,
        executables: vec!["uv"],
    })
}

fn python_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let tag = "20260510";
    let arch_label = platform::rust_triple();
    let ext = platform::archive_ext(&Python);
    let v = if version == "latest" {
        "3.12.10"
    } else {
        version
    };
    let archive_name = format!("cpython-{}+{}-{}-install_only", v, tag, arch_label);
    Some(RuntimeDownloadInfo {
        url: format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{}/{}.{}",
            tag, archive_name, ext
        ),
        version: v.to_string(),
        ext,
        extract_subdir: Some(archive_name),
        executables: vec!["python3"],
    })
}

fn go_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let arch = format!(
        "{}-{}",
        platform::go_platform_os(),
        platform::go_platform_arch()
    );
    let ext = platform::archive_ext(&Go);
    let v = if version == "latest" {
        "1.22.4"
    } else {
        version
    };
    Some(RuntimeDownloadInfo {
        url: format!("https://dl.google.com/go/go{}.{}.{}", v, arch, ext),
        version: v.to_string(),
        ext,
        extract_subdir: Some("go".to_string()),
        executables: vec!["go"],
    })
}

fn java_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let os = platform::java_platform_os();
    let arch = "x64";
    let ext = platform::archive_ext(&Java);
    let v = if version == "latest" {
        "21.0.7"
    } else {
        version
    };

    // 解析大版本号以选择正确的 Adoptium 仓库
    let major = v.split('.').next().unwrap_or("21");
    let download_url = match major {
        "21" => format!("https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.7%2B9/OpenJDK21U-jdk_{}_{}_hotspot_21.0.7_9.{}", arch, os, ext),
        "17" => format!("https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.14%2B7/OpenJDK17U-jdk_{}_{}_hotspot_17.0.14_7.{}", arch, os, ext),
        "11" => format!("https://github.com/adoptium/temurin11-binaries/releases/download/jdk-11.0.26%2B4/OpenJDK11U-jdk_{}_{}_hotspot_11.0.26_4.{}", arch, os, ext),
        "8" => format!("https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u442-b06/OpenJDK8U-jdk_{}_{}_hotspot_8u442b06.{}", arch, os, ext),
        _ => format!("https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.7%2B9/OpenJDK21U-jdk_{}_{}_hotspot_21.0.7_9.{}", arch, os, ext),
    };

    // JDK 归档解压后会得到形如 "jdk-21.0.7+9" 的目录
    let subdir = format!("jdk-{}", v.replace('+', "%2B"));

    Some(RuntimeDownloadInfo {
        url: download_url,
        version: v.to_string(),
        ext,
        extract_subdir: Some(subdir),
        executables: vec!["java", "javac"],
    })
}

fn deno_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let suffix = platform::deno_asset_suffix_for_installer();
    let ext = platform::archive_ext(&Deno);
    let v = if version == "latest" {
        "2.2.0"
    } else {
        version
    };
    Some(RuntimeDownloadInfo {
        url: format!(
            "https://github.com/denoland/deno/releases/download/v{}/deno-{}",
            v, suffix
        ),
        version: v.to_string(),
        ext,
        extract_subdir: None,
        executables: vec!["deno"],
    })
}

fn bun_install_info(version: &str) -> Option<RuntimeDownloadInfo> {
    let asset = platform::bun_asset_name_for_installer();
    let ext = platform::archive_ext(&Bun);
    let v = if version == "latest" {
        "1.2.5"
    } else {
        version
    };
    Some(RuntimeDownloadInfo {
        url: format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/{}",
            v, asset
        ),
        version: v.to_string(),
        ext,
        extract_subdir: None,
        executables: vec!["bun"],
    })
}

// ── 辅助函数 ──

fn emit_progress<F: Fn(InstallProgress) + Send + 'static>(
    cb: &F,
    rt: RuntimeType,
    stage: &str,
    progress: f64,
    message: &str,
) {
    cb(InstallProgress {
        runtime_type: rt,
        stage: stage.to_string(),
        progress,
        message: message.to_string(),
    });
}

async fn download_file<F: Fn(InstallProgress) + Send + 'static>(
    url: &str,
    on_progress: &F,
    rt: RuntimeType,
) -> Result<Vec<u8>> {
    let response = crate::runtime::toolchain::http_client::get_http_client()
        .get(url)
        .send()
        .await
        .map_err(crate::core::error::AppError::Http)?;
    if !response.status().is_success() {
        return Err(crate::core::error::AppError::Http(
            response.error_for_status().unwrap_err(),
        ));
    }
    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(crate::core::error::AppError::Http)?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        if total_size > 0 {
            let pct = downloaded as f64 / total_size as f64;
            emit_progress(
                on_progress,
                rt.clone(),
                "downloading",
                pct.min(0.9),
                &format!("下载中... {:.1}%", pct * 100.0),
            );
        }
    }
    Ok(bytes)
}

fn extract_archive(bytes: &[u8], dest: &std::path::Path, ext: &str) -> Result<()> {
    match ext {
        "zip" => {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
            archive.extract(dest).map_err(|e| {
                crate::core::error::AppError::Io(std::io::Error::other(e))
            })?;
        }
        "tar.gz" | "tgz" => {
            let decoder = flate2::read::GzDecoder::new(bytes);
            let mut archive = tar::Archive::new(decoder);
            archive
                .unpack(dest)
                .map_err(crate::core::error::AppError::Io)?;
        }
        _ => {
            return Err(crate::core::error::AppError::InvalidInput(format!(
                "不支持的压缩格式: {}",
                ext
            )))
        }
    }
    Ok(())
}

fn rename_cross_device(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_err() {
        copy_dir_all(src, dst)?;
        std::fs::remove_dir_all(src)?;
    }
    Ok(())
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn make_executable(path: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

/// 清理守卫：若未解除，在 drop 时删除跟踪的目录。
/// 用于 `install_portable` 失败时清理未完成的安装。
struct CleanupGuard {
    path: Option<std::path::PathBuf>,
}

impl CleanupGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path.take();
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            if path.exists() {
                log::warn!("清理未完成的安装: {:?}", path);
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}
