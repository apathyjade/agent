//! 平台标识与压缩包扩展名映射。
//!
//! 各运行时下载源使用**不同格式**的平台标识，因此这里按“格式家族”提供
//! 具名函数，而不是把字符串统一成一个值：
//!
//! - Node.js 官方 dist 使用 `win-x64` / `linux-x64` / `darwin-arm64`；
//! - Rust、Python、uv 使用目标三元组，如 `x86_64-pc-windows-msvc`；
//! - Go 使用 `windows-amd64` 这类“系统-架构”组合；
//! - Java（Adoptium）只区分操作系统名 `windows` / `linux` / `mac`；
//! - Bun、Deno 使用完整的资产文件名或后缀。
//!
//! 所有函数都只在编译期由 `cfg` 决定返回值，不做运行时探测。部分函数保留
//! 了安装器与版本源在 macOS x86_64 上历史行为不一致的差异，调用方应按
//! 注释选择，不要跨函数替换。

use super::RuntimeType;

/// Node.js 官方 dist 的平台标识（`win-x64` / `linux-x64` / `darwin-arm64`）。
///
/// macOS 无论 CPU 架构都返回 `darwin-arm64`，与既有实现保持一致。
pub fn node_dist_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "win-x64"
    }
    #[cfg(target_os = "linux")]
    {
        "linux-x64"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin-arm64"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "linux-x64"
    }
}

/// Rust 风格的目标三元组（`x86_64-pc-windows-msvc` 等）。
///
/// Python（python-build-standalone）与 uv 的发布资产使用该格式。macOS 上
/// 固定返回 `aarch64-apple-darwin`，与既有实现一致。
pub fn rust_triple() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(target_os = "linux")]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(target_os = "macos")]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "x86_64-unknown-linux-gnu"
    }
}

/// Go 官方下载使用的操作系统名（`windows` / `linux` / `darwin`）。
pub fn go_platform_os() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "linux"
    }
}

/// Go 官方下载使用的架构名（`amd64` / `arm64`）。
///
/// macOS 固定为 `arm64`，与既有实现一致。
pub fn go_platform_arch() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "amd64"
    }
    #[cfg(target_os = "linux")]
    {
        "amd64"
    }
    #[cfg(target_os = "macos")]
    {
        "arm64"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "amd64"
    }
}

/// Java（Adoptium）使用的操作系统名（`windows` / `linux` / `mac`）。
pub fn java_platform_os() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        "mac"
    }
}

/// Bun 发布资产的完整文件名（版本源使用）。
///
/// 区分 macOS 的两种架构：`bun-darwin-arm64.zip` 与 `bun-darwin-x64.zip`。
pub fn bun_asset_name() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "bun-windows-x64.zip"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "bun-linux-x64.zip"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "bun-darwin-arm64.zip"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "bun-darwin-x64.zip"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(
            target_os = "macos",
            any(target_arch = "aarch64", target_arch = "x86_64")
        ),
    )))]
    {
        "bun-windows-x64.zip"
    }
}

/// Bun 发布资产的完整文件名（安装器历史行为）。
///
/// 与 [`bun_asset_name`] 的唯一差别：macOS x86_64 会落入回退分支，得到
/// `bun-windows-x64.zip`。保留该行为是为了不改变安装器当前产出的 URL。
pub fn bun_asset_name_for_installer() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "bun-windows-x64.zip"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "bun-linux-x64.zip"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "bun-darwin-arm64.zip"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
    )))]
    {
        "bun-windows-x64.zip"
    }
}

/// Deno 发布资产的文件名后缀，包含扩展名（版本源使用）。
///
/// 区分 macOS 的两种架构：`aarch64-apple-darwin.tar.gz` 与
/// `x86_64-apple-darwin.tar.gz`。
pub fn deno_asset_suffix() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc.zip"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin.tar.gz"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(
            target_os = "macos",
            any(target_arch = "aarch64", target_arch = "x86_64")
        ),
    )))]
    {
        "x86_64-pc-windows-msvc.zip"
    }
}

/// Deno 发布资产的文件名后缀（安装器历史行为）。
///
/// 与 [`deno_asset_suffix`] 的唯一差别：macOS x86_64 落入回退分支，得到
/// `aarch64-apple-darwin.tar.gz`。保留该行为是为了不改变安装器当前产出的 URL。
pub fn deno_asset_suffix_for_installer() -> &'static str {
    if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc.zip"
    } else if cfg!(target_os = "linux") {
        "x86_64-unknown-linux-gnu.tar.gz"
    } else {
        "aarch64-apple-darwin.tar.gz"
    }
}

/// 运行时的压缩包扩展名（`zip` 或 `tar.gz`）。
///
/// Windows 默认使用 `zip`；Python 的 python-build-standalone 资产在所有平台
/// 都是 `tar.gz`，Bun 的发布资产在所有平台都是 `zip`，因此单独处理。
pub fn archive_ext(rt: &RuntimeType) -> &'static str {
    match rt {
        RuntimeType::Python => "tar.gz",
        RuntimeType::Bun => "zip",
        _ => {
            if cfg!(target_os = "windows") {
                "zip"
            } else {
                "tar.gz"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_platform_matches_platform() {
        #[cfg(target_os = "windows")]
        assert_eq!(node_dist_platform(), "win-x64");
        #[cfg(target_os = "linux")]
        assert_eq!(node_dist_platform(), "linux-x64");
        #[cfg(target_os = "macos")]
        assert_eq!(node_dist_platform(), "darwin-arm64");
    }

    #[test]
    fn rust_triple_matches_platform() {
        #[cfg(target_os = "windows")]
        assert_eq!(rust_triple(), "x86_64-pc-windows-msvc");
        #[cfg(target_os = "linux")]
        assert_eq!(rust_triple(), "x86_64-unknown-linux-gnu");
        #[cfg(target_os = "macos")]
        assert_eq!(rust_triple(), "aarch64-apple-darwin");
    }

    #[test]
    fn go_platform_matches_platform() {
        #[cfg(target_os = "windows")]
        assert_eq!((go_platform_os(), go_platform_arch()), ("windows", "amd64"));
        #[cfg(target_os = "linux")]
        assert_eq!((go_platform_os(), go_platform_arch()), ("linux", "amd64"));
        #[cfg(target_os = "macos")]
        assert_eq!((go_platform_os(), go_platform_arch()), ("darwin", "arm64"));
    }

    #[test]
    fn java_platform_matches_platform() {
        #[cfg(target_os = "windows")]
        assert_eq!(java_platform_os(), "windows");
        #[cfg(target_os = "linux")]
        assert_eq!(java_platform_os(), "linux");
        #[cfg(target_os = "macos")]
        assert_eq!(java_platform_os(), "mac");
    }

    #[test]
    fn bun_asset_names_match_platform() {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(bun_asset_name(), "bun-windows-x64.zip");
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(bun_asset_name(), "bun-linux-x64.zip");
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(bun_asset_name(), "bun-darwin-arm64.zip");
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            assert_eq!(bun_asset_name(), "bun-darwin-x64.zip");
            assert_eq!(bun_asset_name_for_installer(), "bun-windows-x64.zip");
        }
    }

    #[test]
    fn deno_asset_suffix_matches_platform() {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(deno_asset_suffix(), "x86_64-pc-windows-msvc.zip");
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(deno_asset_suffix(), "x86_64-unknown-linux-gnu.tar.gz");
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(deno_asset_suffix(), "aarch64-apple-darwin.tar.gz");
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            assert_eq!(deno_asset_suffix(), "x86_64-apple-darwin.tar.gz");
            assert_eq!(
                deno_asset_suffix_for_installer(),
                "aarch64-apple-darwin.tar.gz"
            );
        }
    }

    #[test]
    fn archive_ext_matches_runtime() {
        #[cfg(target_os = "windows")]
        assert_eq!(archive_ext(&RuntimeType::Node), "zip");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(archive_ext(&RuntimeType::Node), "tar.gz");
        assert_eq!(archive_ext(&RuntimeType::Python), "tar.gz");
    }
}
