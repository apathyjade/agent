use serde_json::{json, Value};
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use crate::error::AppError;

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};

pub struct FileSystemTool;

impl FileSystemTool {
    pub fn new() -> Self {
        Self
    }

    fn allowed_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home);
        }
        if let Some(doc) = dirs::document_dir() {
            dirs.push(doc);
        }
        if let Some(desktop) = dirs::desktop_dir() {
            dirs.push(desktop);
        }
        // Always allow the current working directory
        if let Ok(cwd) = std::env::current_dir() {
            if !dirs.iter().any(|d| cwd.starts_with(d)) {
                dirs.push(cwd);
            }
        }
        // Absolute fallback
        if dirs.is_empty() {
            dirs.push(PathBuf::from("."));
        }
        dirs
    }

    fn resolve_path(&self, path: &str) -> std::result::Result<PathBuf, AppError> {
        let resolved = if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(&path[2..])
            } else {
                PathBuf::from(path)
            }
        } else {
            PathBuf::from(path)
        };

        // Try to canonicalize, but if the path doesn't exist yet (e.g., for write/mkdir),
        // resolve using its parent
        let canonical = match resolved.canonicalize() {
            Ok(c) => c,
            Err(_) => {
                // For non-existent paths, canonicalize the parent and append the filename
                if let Some(parent) = resolved.parent() {
                    if let Ok(parent_canonical) = parent.canonicalize() {
                        parent_canonical.join(
                            resolved.file_name().unwrap_or_default(),
                        )
                    } else {
                        resolved
                    }
                } else {
                    resolved
                }
            }
        };

        // Check path is within any allowed directory
        let allowed = Self::allowed_dirs();
        let is_allowed = allowed.iter().any(|dir| canonical.starts_with(dir));
        if !is_allowed {
            return Err(AppError::Tool(format!(
                "Access denied: path '{}' is outside allowed directories",
                path
            )));
        }

        Ok(canonical)
    }
}

impl ToolDyn for FileSystemTool {
    fn name(&self) -> String {
        "file_system".to_string()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> Pin<Box<dyn Future<Output = ToolDefinition> + Send + 'a>> {
        Box::pin(async move {
            ToolDefinition {
                name: "file_system".to_string(),
                description: "Read, write, and manage files and directories. Supports reading files, writing files, listing directory contents, creating directories, and deleting files.".to_string(),
                parameters: json!({
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
                }),
            }
        })
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, ToolError>> + Send + 'a>>
    {
        Box::pin(async move {
            let input: Value = serde_json::from_str(&args)
                .map_err(|e| ToolError::JsonError(e))?;

            let action = input["action"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::ToolCallError("Missing 'action' parameter".to_string().into())
                })?;

            let path = input["path"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::ToolCallError("Missing 'path' parameter".to_string().into())
                })?;

            let resolved = self.resolve_path(path)
                .map_err(|e| ToolError::ToolCallError(e.into()))?;

            match action {
                "read" => {
                    let content = fs::read_to_string(&resolved)
                        .map_err(|e| {
                            ToolError::ToolCallError(format!("Failed to read file: {}", e).into())
                        })?;
                    serde_json::to_string(&json!({
                        "action": "read",
                        "path": path,
                        "content": content,
                        "size": content.len()
                    }))
                    .map_err(|e| ToolError::JsonError(e))
                }
                "write" => {
                    let content = input["content"]
                        .as_str()
                        .ok_or_else(|| {
                            ToolError::ToolCallError(
                                "Missing 'content' parameter for write action".to_string().into(),
                            )
                        })?;

                    if let Some(parent) = resolved.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|e| {
                                ToolError::ToolCallError(
                                    format!("Failed to create directories: {}", e).into(),
                                )
                            })?;
                    }

                    fs::write(&resolved, content)
                        .map_err(|e| {
                            ToolError::ToolCallError(format!("Failed to write file: {}", e).into())
                        })?;

                    serde_json::to_string(&json!({
                        "action": "write",
                        "path": path,
                        "bytes_written": content.len()
                    }))
                    .map_err(|e| ToolError::JsonError(e))
                }
                "list" => {
                    let entries = fs::read_dir(&resolved)
                        .map_err(|e| {
                            ToolError::ToolCallError(
                                format!("Failed to list directory: {}", e).into(),
                            )
                        })?;

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

                    serde_json::to_string(&json!({
                        "action": "list",
                        "path": path,
                        "directories": dirs,
                        "files": files
                    }))
                    .map_err(|e| ToolError::JsonError(e))
                }
                "mkdir" => {
                    fs::create_dir_all(&resolved)
                        .map_err(|e| {
                            ToolError::ToolCallError(
                                format!("Failed to create directory: {}", e).into(),
                            )
                        })?;
                    serde_json::to_string(&json!({
                        "action": "mkdir",
                        "path": path,
                        "created": true
                    }))
                    .map_err(|e| ToolError::JsonError(e))
                }
                "delete" => {
                    if resolved.is_dir() {
                        fs::remove_dir_all(&resolved)
                            .map_err(|e| {
                                ToolError::ToolCallError(
                                    format!("Failed to delete directory: {}", e).into(),
                                )
                            })?;
                    } else {
                        fs::remove_file(&resolved)
                            .map_err(|e| {
                                ToolError::ToolCallError(
                                    format!("Failed to delete file: {}", e).into(),
                                )
                            })?;
                    }
                    serde_json::to_string(&json!({
                        "action": "delete",
                        "path": path,
                        "deleted": true
                    }))
                    .map_err(|e| ToolError::JsonError(e))
                }
                _ => Err(ToolError::ToolCallError(
                    format!("Unknown action: {}", action).into(),
                )),
            }
        })
    }
}
