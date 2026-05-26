use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{AppError, Result};
use crate::tools::calculator::CalculatorTool;
use crate::tools::code_executor::CodeExecutorTool;
use crate::tools::file_system::FileSystemTool;
use crate::tools::run_workflow::RunWorkflowTool;
use crate::tools::web_search::WebSearchTool;
use crate::tools::r#trait::{Tool, ToolInfo};

/// Aliases for tool names, mapping common alternative names to registered tool names
fn tool_aliases() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("read_file", "file_system");
    m.insert("write_file", "file_system");
    m.insert("read_directory", "file_system");
    m.insert("list_files", "file_system");
    m.insert("delete_file", "file_system");
    m.insert("search", "web_search");
    m.insert("websearch", "web_search");
    m.insert("web-search", "web_search");
    m.insert("google", "web_search");
    m.insert("calculate", "calculator");
    m.insert("calc", "calculator");
    m.insert("math", "calculator");
    m.insert("execute", "code_executor");
    m.insert("run", "code_executor");
    m.insert("shell", "code_executor");
    m.insert("bash", "code_executor");
    m.insert("python", "code_executor");
    m.insert("workflow", "run_workflow");
    m
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    enabled: HashMap<String, bool>,
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut aliases = HashMap::new();
        for (alias, target) in tool_aliases() {
            aliases.insert(alias.to_string(), target.to_string());
        }

        let mut registry = Self {
            tools: HashMap::new(),
            enabled: HashMap::new(),
            aliases,
        };

        registry.register("calculator", Arc::new(CalculatorTool::new()), true);
        registry.register("file_system", Arc::new(FileSystemTool::new()), true);
        registry.register("web_search", Arc::new(WebSearchTool::new()), true);
        registry.register("code_executor", Arc::new(CodeExecutorTool::new()), true);
        registry.register("run_workflow", Arc::new(RunWorkflowTool), true);

        registry
    }

    pub fn register(&mut self, name: &str, tool: Arc<dyn Tool>, enabled: bool) {
        self.tools.insert(name.to_string(), tool);
        self.enabled.insert(name.to_string(), enabled);
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn Tool>> {
        // Direct lookup first
        if let Some(tool) = self.tools.get(name) {
            return Ok(tool.clone());
        }
        // Alias lookup
        if let Some(alias) = self.aliases.get(name) {
            if let Some(tool) = self.tools.get(alias) {
                return Ok(tool.clone());
            }
        }
        // Case-insensitive fallback
        let lower = name.to_lowercase();
        for (key, tool) in &self.tools {
            if key.to_lowercase() == lower {
                return Ok(tool.clone());
            }
        }
        Err(AppError::Tool(format!("Tool '{}' not found", name)))
    }

    pub fn get_enabled(&self) -> Vec<Arc<dyn Tool>> {
        self.tools
            .iter()
            .filter(|(name, _)| self.enabled.get(name.as_str()).copied().unwrap_or(false))
            .map(|(_, tool)| tool.clone())
            .collect()
    }

    pub fn list(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|(name, tool)| ToolInfo {
                name: name.clone(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
                enabled: self.enabled.get(name.as_str()).copied().unwrap_or(false),
            })
            .collect()
    }

    pub fn register_dynamic(&mut self, name: &str, tool: Arc<dyn Tool>, enabled: bool) {
        self.tools.insert(name.to_string(), tool);
        self.enabled.insert(name.to_string(), enabled);
    }

    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
        self.enabled.remove(name);
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn toggle(&mut self, name: &str, enabled: bool) -> Result<()> {
        if self.tools.contains_key(name) {
            self.enabled.insert(name.to_string(), enabled);
            Ok(())
        } else {
            Err(AppError::Tool(format!("Tool '{}' not found", name)))
        }
    }

    pub async fn execute(&self, name: &str, input: serde_json::Value) -> Result<serde_json::Value> {
        let tool = self.get(name)?;
        tool.execute(input).await
    }
}
