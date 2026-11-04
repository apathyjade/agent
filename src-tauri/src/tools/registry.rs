use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use serde_json::Value;

use crate::core::error::{AppError, Result};
use crate::tools::calculator::CalculatorTool;
use crate::tools::code_executor::CodeExecutorTool;
use crate::tools::file_system::FileSystemTool;
use crate::tools::run_workflow::RunWorkflowTool;
use crate::tools::web_search::WebSearchTool;
use crate::tools::r#trait::{AgentTool, ToolInfo};

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
    tools: HashMap<String, Arc<dyn AgentTool>>,
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

    pub fn register(&mut self, name: &str, tool: Arc<dyn AgentTool>, enabled: bool) {
        self.tools.insert(name.to_string(), tool);
        self.enabled.insert(name.to_string(), enabled);
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn AgentTool>> {
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

    pub fn get_enabled(&self) -> Vec<Arc<dyn AgentTool>> {
        self.tools
            .iter()
            .filter(|(name, _)| self.enabled.get(name.as_str()).copied().unwrap_or(false))
            .map(|(_, tool)| tool.clone())
            .collect()
    }

    /// Convert enabled tools to Rig [`DynamicTool`]s for use with
    /// [`rig::agent::Agent`].
    ///
    /// Rig 0.45 has no object-safe tool trait, so each registered tool is
    /// erased into a `DynamicTool` whose callback forwards to
    /// [`AgentTool::call`](crate::tools::r#trait::AgentTool::call).
    pub fn to_rig_tools(&self, allowed: Option<&[String]>) -> Vec<rig::tool::DynamicTool> {
        use rig::message::ToolName;
        use rig::tool::{ToolExecutionError, ToolOutput};

        let mut dynamic_tools = Vec::new();

        for (name, tool) in &self.tools {
            let is_enabled = self.enabled.get(name).copied().unwrap_or(false);
            let is_allowed = allowed.map_or(true, |a| a.contains(name));
            if !is_enabled || !is_allowed {
                continue;
            }

            let spec = tool.definition();
            let tool_name = match ToolName::new(spec.name.clone()) {
                Ok(name) => name,
                Err(e) => {
                    log::warn!("Skipping tool '{}': {}", name, e);
                    continue;
                }
            };

            let tool = tool.clone();
            dynamic_tools.push(rig::tool::DynamicTool::new(
                tool_name,
                spec.description,
                spec.parameters,
                move |args: Value| {
                    let tool = tool.clone();
                    Box::pin(async move {
                        tool.call(args.to_string())
                            .await
                            .map(ToolOutput::text)
                            .map_err(ToolExecutionError::from)
                    })
                },
            ));
        }

        dynamic_tools
    }

    pub fn execute(&self, name: &str, input: Value) -> impl Future<Output = Result<Value>> + Send + '_ {
        let name_owned = name.to_string();
        async move {
            let tool = self.get(&name_owned)?;
            let args = input.to_string();
            let result = tool
                .call(args)
                .await
                .map_err(|e| AppError::Tool(format!("Tool '{}' failed: {}", name_owned, e)))?;
            serde_json::from_str(&result)
                .map_err(|e| AppError::Tool(format!("Tool '{}' returned invalid JSON: {}", name_owned, e)))
        }
    }

    pub fn list(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|(name, tool)| ToolInfo {
                name: tool.name(),
                description: String::new(),
                parameters: Value::Null,
                enabled: self.enabled.get(name.as_str()).copied().unwrap_or(false),
            })
            .collect()
    }

    pub fn register_dynamic(&mut self, name: &str, tool: Arc<dyn AgentTool>, enabled: bool) {
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
}
