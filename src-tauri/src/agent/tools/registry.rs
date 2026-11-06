//! 工具注册表：注册、启用开关、别名解析与调用。
//!
//! 注册表只做**策略**：保存已构造好的 [`ToolEntry`]（动态工具 + footprint
//! 解析器）、维护启用状态与别名。工具的定义、参数校验与错误语义全部来自
//! Rig，注册表不重复实现。
//!
//! 约束：别名只做「名字 → 已注册工具名」的映射，不会隐式注册新工具。

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use serde_json::Value;

use crate::core::error::{AppError, Result};
use crate::agent::tools::base::ToolInfo;
use crate::agent::tools::convert::ToolEntry;
use crate::agent::tools::rig_tools;
use crate::llm::provider::{
    decide, ApprovalDecision, ApprovalGate, ToolPolicyMeta, ToolRuntime,
};

/// 工具别名表：把常见替代写法映射到已注册的工具名。
fn tool_aliases() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    // 文件类：旧名字拆到 rig-tools 的具体工具上。
    m.insert("read_file", "read");
    m.insert("read_text", "read");
    m.insert("write_file", "write");
    m.insert("edit_file", "edit");
    m.insert("read_directory", "list");
    m.insert("list_files", "list");
    m.insert("list_dir", "list");
    m.insert("delete_file", "delete");
    m.insert("move_file", "move");
    m.insert("rename", "move");
    m.insert("grep", "search");
    m.insert("rg", "search");
    // 执行类：旧 code_executor 统一落到 shell。
    m.insert("execute", "shell");
    m.insert("run", "shell");
    m.insert("bash", "shell");
    m.insert("sh", "shell");
    m.insert("python", "shell");
    m.insert("code_executor", "shell");
    m
}

/// 工具注册表，维护工具条目、启用状态、来源与别名。
pub struct ToolRegistry {
    tools: HashMap<String, ToolEntry>,
    enabled: HashMap<String, bool>,
    /// 来源标注：`builtin` 或 `mcp:<server>`。
    sources: HashMap<String, String>,
    aliases: HashMap<String, String>,
    /// 人工确认通道；`None` 表示不启用。
    gate: Option<Arc<dyn ApprovalGate>>,
}

impl ToolRegistry {
    /// 创建注册表并注册全部内置工具（默认启用）。
    pub fn new() -> Self {
        let mut aliases = HashMap::new();
        for (alias, target) in tool_aliases() {
            aliases.insert(alias.to_string(), target.to_string());
        }

        let mut registry = Self {
            tools: HashMap::new(),
            enabled: HashMap::new(),
            sources: HashMap::new(),
            aliases,
            gate: None,
        };

        for entry in rig_tools::tool_entries() {
            let name = entry.name();
            registry.register(&name, entry, true);
        }

        registry
    }

    /// 注册（或覆盖）一个内置工具，并设置其启用状态。
    pub fn register(&mut self, name: &str, entry: ToolEntry, enabled: bool) {
        self.insert_with_source(name, entry, enabled, "builtin");
    }

    /// 注册一个 MCP 工具，来源标为 `mcp:<server>`。
    pub fn register_mcp(&mut self, server: &str, name: &str, entry: ToolEntry, enabled: bool) {
        let source = format!("mcp:{}", server);
        self.insert_with_source(name, entry, enabled, &source);
    }

    /// 内部注册入口。
    fn insert_with_source(&mut self, name: &str, entry: ToolEntry, enabled: bool, source: &str) {
        self.tools.insert(name.to_string(), entry);
        self.enabled.insert(name.to_string(), enabled);
        self.sources.insert(name.to_string(), source.to_string());
    }

    /// 按配置刷新**内置**工具的启用状态。
    ///
    /// MCP 工具的启用状态由各自服务器配置决定，不能被这份列表覆盖。
    pub fn apply_builtin_enabled(&mut self, enabled_names: &[String]) {
        for (name, source) in &self.sources {
            if source == "builtin" {
                let enabled = enabled_names.iter().any(|item| item == name);
                self.enabled.insert(name.clone(), enabled);
            }
        }
    }

    /// 某个已注册工具当前是否启用（不解析别名）。
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.get(name).copied().unwrap_or(false)
    }

    /// 注入人工确认通道。
    pub fn set_gate(&mut self, gate: Arc<dyn ApprovalGate>) {
        self.gate = Some(gate);
    }

    /// 组装交给 Rig 的工具集合与策略元数据。
    pub fn tool_runtime(&self, allowed: Option<&[String]>) -> ToolRuntime {
        let mut tools = Vec::new();
        let mut meta = HashMap::new();

        for entry in self.active(allowed) {
            let definition = entry.tool.definition();
            let name = definition.name.as_str().to_string();
            meta.insert(
                name,
                ToolPolicyMeta {
                    annotations: definition.annotations,
                    footprint: entry.footprint.clone(),
                },
            );
            tools.push(entry.tool);
        }

        ToolRuntime {
            tools,
            meta: Arc::new(meta),
            approval: self.gate.clone(),
        }
    }

    /// 按名称查找工具。
    ///
    /// 依次尝试：精确匹配 → 别名 → 忽略大小写；都失败时返回错误。
    pub fn get(&self, name: &str) -> Result<ToolEntry> {
        // 先做精确匹配。
        if let Some(tool) = self.tools.get(name) {
            return Ok(tool.clone());
        }
        // 再查别名。
        if let Some(alias) = self.aliases.get(name) {
            if let Some(tool) = self.tools.get(alias) {
                return Ok(tool.clone());
            }
        }
        // 最后忽略大小写兜底。
        let lower = name.to_lowercase();
        for (key, tool) in &self.tools {
            if key.to_lowercase() == lower {
                return Ok(tool.clone());
            }
        }
        Err(AppError::Tool(format!("Tool '{}' not found", name)))
    }

    /// 解析别名后的规范注册名。
    pub fn canonical_name(&self, name: &str) -> Option<String> {
        if self.tools.contains_key(name) {
            return Some(name.to_string());
        }
        if let Some(target) = self.aliases.get(name) {
            if self.tools.contains_key(target) {
                return Some(target.clone());
            }
        }
        let lower = name.to_lowercase();
        self.tools
            .keys()
            .find(|key| key.to_lowercase() == lower)
            .cloned()
    }

    /// 当前对模型生效的工具集合。
    ///
    /// 两层过滤：启用开关 + 调用方白名单。白名单里的名字先做别名归一
    /// （`read_file` → `file_system`），因此外部配置不必知道真实注册名。
    /// `Some(&[])` 是明确的「一个都不给」，`None` 才是「不限制」。
    pub fn active(&self, allowed: Option<&[String]>) -> Vec<ToolEntry> {
        let allowed_names: Option<Vec<String>> = allowed.map(|list| {
            list.iter()
                .filter_map(|item| self.canonical_name(item))
                .collect()
        });

        self.tools
            .iter()
            .filter(|(name, _)| {
                let is_enabled = self.enabled.get(name.as_str()).copied().unwrap_or(false);
                let is_allowed = allowed_names
                    .as_ref()
                    .map_or(true, |list| list.iter().any(|item| item == *name));
                is_enabled && is_allowed
            })
            .map(|(_, entry)| entry.clone())
            .collect()
    }

    /// 把生效的工具转成 Rig 的 [`DynamicTool`](rig::tool::DynamicTool)。
    ///
    /// 注册表存的是 [`ToolEntry`]，但交给 Rig 执行的只有其中的 `tool`——
    /// 执行形态保持唯一。
    pub fn to_rig_tools(&self, allowed: Option<&[String]>) -> Vec<rig::tool::DynamicTool> {
        self.active(allowed)
            .into_iter()
            .map(|entry| entry.tool)
            .collect()
    }

    /// 以 JSON 对象调用工具。
    ///
    /// 返回 JSON 的工具直接解析；返回纯文本的工具则原样包成字符串。
    /// 不做「解析失败即报错」——工具输出形态本来就允许是文本。
    pub fn execute(&self, name: &str, input: Value) -> impl Future<Output = Result<Value>> + Send + '_ {
        let name_owned = name.to_string();
        async move {
            let entry = self.get(&name_owned)?;

            // 审批闸门：deep 路径没有 Rig 的 dispatch hook，必须在注册表入口
            // 走同一套判定，否则同一条策略在两条路径上不一致。
            if let Some(gate) = &self.gate {
                let footprint = entry.footprint_for(&input);
                let definition = entry.tool.definition();
                match decide(
                    gate.policy(),
                    definition.name.as_str(),
                    &definition.annotations,
                    footprint.as_ref(),
                ) {
                    ApprovalDecision::Allow => {}
                    ApprovalDecision::Deny(reason) => {
                        return Err(AppError::ApprovalDenied(reason));
                    }
                    ApprovalDecision::Confirm => {
                        let call_id = format!("deep-{}", uuid::Uuid::new_v4());
                        let approved = gate
                            .request(
                                &call_id,
                                definition.name.as_str(),
                                &input,
                                "This tool call may modify data and needs confirmation",
                            )
                            .await;
                        if !approved {
                            return Err(AppError::ApprovalDenied(
                                "User rejected this tool call".to_string(),
                            ));
                        }
                    }
                }
            }

            let output = entry
                .tool
                .run(input)
                .await
                .map_err(|e| AppError::Tool(format!("Tool '{}' failed: {}", name_owned, e)))?;

            let rendered = output.render();
            Ok(serde_json::from_str(&rendered).unwrap_or(Value::String(rendered)))
        }
    }

    /// 工具目录：含真实描述、参数 schema 与风险注解，供前端与规划器使用。
    pub fn catalog(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|(name, entry)| {
                let definition = entry.tool.definition();
                let annotations = definition.annotations;
                ToolInfo {
                    name: definition.name.as_str().to_string(),
                    description: definition.description,
                    parameters: definition.parameters,
                    enabled: self.enabled.get(name.as_str()).copied().unwrap_or(false),
                    read_only: annotations.read_only(),
                    destructive: annotations.destructive(),
                    open_world: annotations.open_world(),
                    source: self
                        .sources
                        .get(name.as_str())
                        .cloned()
                        .unwrap_or_else(|| "builtin".to_string()),
                }
            })
            .collect()
    }

    /// 列出所有工具及其启用状态（供前端展示）。
    pub fn list(&self) -> Vec<ToolInfo> {
        self.catalog()
    }

    /// 移除工具及其启用状态与来源记录。
    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
        self.enabled.remove(name);
        self.sources.remove(name);
    }

    /// 判断某个名字是否已注册（不解析别名）。
    pub fn is_registered(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// 开关某个已注册的工具；未注册时返回错误。
    pub fn toggle(&mut self, name: &str, enabled: bool) -> Result<()> {
        if self.tools.contains_key(name) {
            self.enabled.insert(name.to_string(), enabled);
            Ok(())
        } else {
            Err(AppError::Tool(format!("Tool '{}' not found", name)))
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
