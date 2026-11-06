//! 工具链模块：本地运行时检测、版本管理与安装。
//!
//! 这是一个独立的运行时工具链管理器（Node、Python 等），不依赖桌面应用本身，
//! 也不依赖其数据库或 UI 状态。上层（app / CLI）通过 [`services::RuntimeServices`]
//! 组合这些能力，避免两个入口的安装、切换语义出现分歧。

pub mod alias;
mod detector;
pub mod http_client;
mod installer;
pub mod lifecycle;
pub mod manager;
pub mod manager_detector;
pub mod manager_executor;
mod manifest;
pub mod node_integration;
pub mod node_toolchain;
pub mod platform;
pub mod registry;
pub mod resolver;
pub mod services;
pub mod sources;
mod upgrade;
pub mod version;

pub use alias::AliasManager;
pub use detector::RuntimeDetector;
pub use installer::RuntimeInstaller;
pub use lifecycle::VersionLifecycle;
pub use manager::{DiskUsageItem, RuntimeManager};
pub use manifest::{read_manifest, write_manifest, RuntimeManifest};
pub use registry::{CachedVersions, RuntimeRegistry, RuntimeVersion, VersionSource};
pub use resolver::VersionResolver;
pub use services::{external_manager, RuntimeOptions, RuntimeServices};
pub use upgrade::{check_updates, VersionUpdate};

// ── 运行时类型 ──

/// 受支持的运行时环境。
///
/// 序列化时统一使用 `snake_case`，与前端 / 数据库中的运行时标识保持一致。
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeType {
    Node,
    Python,
    Docker,
    Uv,
    Go,
    Rust,
    Java,
    Deno,
    Bun,
    Ruby,
}

impl std::fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeType::Node => write!(f, "node"),
            RuntimeType::Python => write!(f, "python"),
            RuntimeType::Docker => write!(f, "docker"),
            RuntimeType::Uv => write!(f, "uv"),
            RuntimeType::Go => write!(f, "go"),
            RuntimeType::Rust => write!(f, "rust"),
            RuntimeType::Java => write!(f, "java"),
            RuntimeType::Deno => write!(f, "deno"),
            RuntimeType::Bun => write!(f, "bun"),
            RuntimeType::Ruby => write!(f, "ruby"),
        }
    }
}

impl std::str::FromStr for RuntimeType {
    type Err = crate::core::error::AppError;

    fn from_str(s: &str) -> crate::core::error::Result<Self> {
        let normalized = s.trim().to_lowercase();
        let runtime = match normalized.as_str() {
            "node" | "nodejs" | "npx" | "npm" => RuntimeType::Node,
            "python" | "python3" | "py" | "pip" | "pip3" => RuntimeType::Python,
            "docker" => RuntimeType::Docker,
            "uv" | "uvx" => RuntimeType::Uv,
            "go" | "golang" => RuntimeType::Go,
            "rust" | "rustc" | "cargo" | "rustup" => RuntimeType::Rust,
            "java" | "jdk" | "jre" | "javac" => RuntimeType::Java,
            "deno" => RuntimeType::Deno,
            "bun" => RuntimeType::Bun,
            "ruby" | "irb" | "gem" | "bundler" => RuntimeType::Ruby,
            _ => return Err(crate::core::error::AppError::InvalidInput(format!(
                "未知运行时: {}. 支持: node, python, docker, uv, go, rust, java, deno, bun, ruby",
                s
            ))),
        };
        Ok(runtime)
    }
}

impl RuntimeType {
    /// 面向用户展示的运行时名称。
    ///
    /// 仅用于 UI 文案，不参与路径或命令解析；持久化请使用 [`RuntimeType::dir_name`]
    /// 或 `Display` 实现。
    pub fn display_name(&self) -> &'static str {
        match self {
            RuntimeType::Node => "Node.js",
            RuntimeType::Python => "Python",
            RuntimeType::Docker => "Docker",
            RuntimeType::Uv => "uv",
            RuntimeType::Go => "Go",
            RuntimeType::Rust => "Rust",
            RuntimeType::Java => "Java (JDK)",
            RuntimeType::Deno => "Deno",
            RuntimeType::Bun => "Bun",
            RuntimeType::Ruby => "Ruby",
        }
    }

    /// 该运行时对应的 CLI 命令列表，按探测优先级排列（用于 PATH 检测）。
    ///
    /// 例如 Python 优先探测 `python3`，其次 `python`；列表第一项即
    /// [`RuntimeType::primary_command`] 的返回值。
    pub fn commands(&self) -> &[&'static str] {
        match self {
            RuntimeType::Node => &["node", "npx"],
            RuntimeType::Python => &["python3", "python"],
            RuntimeType::Docker => &["docker"],
            RuntimeType::Uv => &["uv", "uvx"],
            RuntimeType::Go => &["go"],
            RuntimeType::Rust => &["rustc", "cargo"],
            RuntimeType::Java => &["java", "javac"],
            RuntimeType::Deno => &["deno"],
            RuntimeType::Bun => &["bun"],
            RuntimeType::Ruby => &["ruby", "irb"],
        }
    }

    /// 主命令名，即 [`RuntimeType::commands`] 的第一项。
    ///
    /// 用于在内置版本目录中查找可执行文件。
    pub fn primary_command(&self) -> &'static str {
        self.commands()[0]
    }

    /// 查询版本号时传给命令的参数。
    ///
    /// 大多数运行时使用 `--version`，但 **Go 与 Java 不同**：
    /// - Go 使用子命令 `version`（`go version`）；
    /// - Java 使用单短横线 `-version`（`java -version`，输出到 stderr）。
    ///
    /// 探测代码必须使用该函数，不能硬编码 `--version`。
    pub fn version_args(&self) -> &'static [&'static str] {
        match self {
            RuntimeType::Go => &["version"],
            RuntimeType::Java => &["-version"],
            _ => &["--version"],
        }
    }

    /// 从可执行命令推断运行时类型，忽略命令后面的参数。
    ///
    /// 例如 `"npx -y foo"` 会按可执行文件名 `npx` 推断出 Node。无法识别时返回
    /// `None`，不会报错。
    pub fn infer_from_command(cmd: &str) -> Option<Self> {
        let executable = cmd.split_whitespace().next()?;
        let base = std::path::Path::new(executable)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(executable);
        base.parse().ok()
    }

    /// 全部运行时变体，顺序固定，供遍历检测与 UI 展示使用。
    pub fn all() -> &'static [RuntimeType] {
        &[
            RuntimeType::Node,
            RuntimeType::Python,
            RuntimeType::Docker,
            RuntimeType::Uv,
            RuntimeType::Go,
            RuntimeType::Rust,
            RuntimeType::Java,
            RuntimeType::Deno,
            RuntimeType::Bun,
            RuntimeType::Ruby,
        ]
    }

    /// 磁盘上存放该运行时各版本的目录名。
    ///
    /// 与 `Display` 输出不同，该值用于拼接安装路径，一经发布不应随意改动，
    /// 否则会找不到已安装的版本。
    pub fn dir_name(&self) -> &'static str {
        match self {
            RuntimeType::Node => "node",
            RuntimeType::Python => "python",
            RuntimeType::Docker => "docker",
            RuntimeType::Uv => "uv",
            RuntimeType::Go => "go",
            RuntimeType::Rust => "rust",
            RuntimeType::Java => "java",
            RuntimeType::Deno => "deno",
            RuntimeType::Bun => "bun",
            RuntimeType::Ruby => "ruby",
        }
    }
}

// ── 安装来源 ──

/// 运行时版本的来源。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSource {
    /// 来自系统 PATH 中已有的安装。
    System,
    /// 由应用内置安装器下载并管理。
    BuiltIn,
    /// 没有找到可用安装；序列化时保持为 `"none"` 供 UI 判断。
    #[serde(rename = "none")]
    Unavailable,
}

// ── 已安装版本 ──

/// 某个运行时的单个已安装版本。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstalledVersion {
    pub version: String,
    pub path: String,
    pub installed_at: String,
    pub is_active: bool,
}

// ── 运行时信息 ──

/// 检测到的运行时详细信息。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeInfo {
    pub runtime_type: RuntimeType,
    pub display_name: String,
    pub source: RuntimeSource,
    /// 当前生效的版本号（未安装时为 `None`）。
    pub version: Option<String>,
    /// 本地已安装的全部版本。
    pub installed_versions: Vec<InstalledVersion>,
    /// 当前生效可执行文件的路径。
    pub executable_path: Option<String>,
    pub error: Option<String>,
    pub available: bool,
}

// ── PATH 冲突检测 ──

/// PATH 中找到的单个可执行文件。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FoundExecutable {
    pub path: String,
    pub version: Option<String>,
    #[serde(default)]
    pub is_active: bool,
}

/// 某个运行时的 PATH 冲突信息。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathConflict {
    pub runtime_type: RuntimeType,
    pub executables: Vec<FoundExecutable>,
    pub conflict: bool,
}

// ── 安装进度 ──

/// 安装过程中的进度事件，通过回调推送给 UI。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallProgress {
    pub runtime_type: RuntimeType,
    pub stage: String,
    pub progress: f64,
    pub message: String,
}

// ── 可下载版本 ──

/// 可从网络下载的版本条目。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AvailableVersion {
    pub version: String,
    pub display_name: String,
    pub url: String,
}

// ── Manager 已拆分到 manager.rs ──
