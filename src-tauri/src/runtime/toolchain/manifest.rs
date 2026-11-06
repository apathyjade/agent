//! 运行时版本清单。
//!
//! 每个运行时目录下都有一个 `.manifest.json`，记录已安装版本与当前生效版本。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 单个已安装版本的记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub path: String,
    pub installed_at: String,
}

/// 运行时目录的版本清单。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeManifest {
    /// 当前生效版本。`None` 表示没有生效版本。
    pub active_version: Option<String>,
    /// 所有已安装版本，以版本字符串为键。
    pub versions: HashMap<String, VersionInfo>,
}

/// 读取清单文件。文件不存在或内容非法时返回 `None`。
pub fn read_manifest(path: &Path) -> Option<RuntimeManifest> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 写入清单文件，必要时自动创建父目录。
pub fn write_manifest(path: &Path, manifest: &RuntimeManifest) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path, content)
}
