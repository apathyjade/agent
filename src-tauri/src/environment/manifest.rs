// ── Runtime Version Manifest ──
//
// Each runtime directory contains a .manifest.json file tracking installed versions.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub path: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeManifest {
    /// Currently active version. None = no version active.
    pub active_version: Option<String>,
    /// All installed versions, keyed by version string.
    pub versions: HashMap<String, VersionInfo>,
}

/// Read a manifest file. Returns None if file doesn't exist or is invalid.
pub fn read_manifest(path: &Path) -> Option<RuntimeManifest> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Write a manifest file.
pub fn write_manifest(path: &Path, manifest: &RuntimeManifest) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path, content)
}
