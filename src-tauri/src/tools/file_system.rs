use async_trait::async_trait;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, Result};

use super::r#trait::Tool;

pub struct FileSystemTool {
    base_path: PathBuf,
}

impl FileSystemTool {
    pub fn new() -> Self {
        let base_path = dirs::document_dir().unwrap_or_else(|| PathBuf::from("."));
        Self { base_path }
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let resolved = if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(&path[2..])
            } else {
                self.base_path.join(path)
            }
        } else {
            self.base_path.join(path)
        };

        let canonical = resolved.canonicalize().unwrap_or(resolved);
        if !canonical.starts_with(&self.base_path) && !canonical.starts_with(dirs::home_dir().unwrap_or_default()) {
            return Err(AppError::Tool("Access denied: path outside allowed directories".to_string()));
        }

        Ok(canonical)
    }
}

#[async_trait]
impl Tool for FileSystemTool {
    fn name(&self) -> &str {
        "file_system"
    }

    fn description(&self) -> &str {
        "Read, write, and manage files and directories. Supports reading files, writing files, listing directory contents, creating directories, and deleting files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write", "list", "mkdir", "delete"],
                    "description": "The action to perform"
                },
                "path": {
                    "type": "string",
                    "description": "The file or directory path"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write (for write action)"
                }
            },
            "required": ["action", "path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let action = input["action"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'action' parameter".to_string()))?;

        let path = input["path"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'path' parameter".to_string()))?;

        let resolved = self.resolve_path(path)?;

        match action {
            "read" => {
                let content = fs::read_to_string(&resolved)
                    .map_err(|e| AppError::Tool(format!("Failed to read file: {}", e)))?;
                Ok(json!({
                    "action": "read",
                    "path": path,
                    "content": content,
                    "size": content.len()
                }))
            }
            "write" => {
                let content = input["content"]
                    .as_str()
                    .ok_or_else(|| AppError::InvalidInput("Missing 'content' parameter for write action".to_string()))?;

                if let Some(parent) = resolved.parent() {
                    fs::create_dir_all(parent)?;
                }

                fs::write(&resolved, content)
                    .map_err(|e| AppError::Tool(format!("Failed to write file: {}", e)))?;

                Ok(json!({
                    "action": "write",
                    "path": path,
                    "bytes_written": content.len()
                }))
            }
            "list" => {
                let entries = fs::read_dir(&resolved)
                    .map_err(|e| AppError::Tool(format!("Failed to list directory: {}", e)))?;

                let mut files = Vec::new();
                let mut dirs = Vec::new();

                for entry in entries {
                    if let Ok(entry) = entry {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                            dirs.push(name);
                        } else {
                            files.push(name);
                        }
                    }
                }

                Ok(json!({
                    "action": "list",
                    "path": path,
                    "directories": dirs,
                    "files": files
                }))
            }
            "mkdir" => {
                fs::create_dir_all(&resolved)
                    .map_err(|e| AppError::Tool(format!("Failed to create directory: {}", e)))?;
                Ok(json!({
                    "action": "mkdir",
                    "path": path,
                    "created": true
                }))
            }
            "delete" => {
                if resolved.is_dir() {
                    fs::remove_dir_all(&resolved)
                        .map_err(|e| AppError::Tool(format!("Failed to delete directory: {}", e)))?;
                } else {
                    fs::remove_file(&resolved)
                        .map_err(|e| AppError::Tool(format!("Failed to delete file: {}", e)))?;
                }
                Ok(json!({
                    "action": "delete",
                    "path": path,
                    "deleted": true
                }))
            }
            _ => Err(AppError::InvalidInput(format!("Unknown action: {}", action))),
        }
    }
}
