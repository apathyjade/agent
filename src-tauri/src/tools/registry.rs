use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolSet};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::tools::calculator::CalculatorTool;
use crate::tools::code_executor::CodeExecutorTool;
use crate::tools::file_system::FileSystemTool;
use crate::tools::run_workflow::RunWorkflowTool;
use crate::tools::web_search::WebSearchTool;
use crate::tools::r#trait::ToolInfo;

/// Wrapper to allow Arc<dyn ToolDyn> to be used as a sized ToolDyn impl
/// for building ToolSet.
struct ArcToolDyn(Arc<dyn ToolDyn>);

impl ToolDyn for ArcToolDyn {
    fn name(&self) -> String {
        self.0.name()
    }

    fn definition<'a>(
        &'a self,
        prompt: String,
    ) -> Pin<Box<dyn Future<Output = ToolDefinition> + Send + 'a>> {
        self.0.definition(prompt)
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, rig::tool::ToolError>> + Send + 'a>>
    {
        self.0.call(args)
    }
}

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
    tools: HashMap<String, Arc<dyn ToolDyn>>,
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

    pub fn register(&mut self, name: &str, tool: Arc<dyn ToolDyn>, enabled: bool) {
        self.tools.insert(name.to_string(), tool);
        self.enabled.insert(name.to_string(), enabled);
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn ToolDyn>> {
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

    pub fn get_enabled(&self) -> Vec<Arc<dyn ToolDyn>> {
        self.tools
            .iter()
            .filter(|(name, _)| self.enabled.get(name.as_str()).copied().unwrap_or(false))
            .map(|(_, tool)| tool.clone())
            .collect()
    }

    /// Convert enabled tools to a Rig ToolSet for use with rig::agent::Agent.
    pub fn to_rig_tool_set(&self, allowed: Option<&[String]>) -> ToolSet {
        let mut builder = ToolSet::builder();
        for (name, tool) in &self.tools {
            let is_enabled = self.enabled.get(name).copied().unwrap_or(false);
            let is_allowed = allowed.map_or(true, |a| a.contains(name));
            if is_enabled && is_allowed {
                builder = builder.static_tool(ArcToolDyn(tool.clone()));
            }
        }
        builder.build()
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

    pub fn register_dynamic(&mut self, name: &str, tool: Arc<dyn ToolDyn>, enabled: bool) {
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
