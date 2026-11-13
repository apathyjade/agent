//! 请求级作用域：把会话 / 领域 / 意图的裁剪声明解析为 [`ResolvedScope`]。
//!
//! 本模块是纯逻辑：只依赖 serde，不做 IO、不碰数据库、不发起路由调用。
//! 声明的持久化在 `core/db/scope_repo.rs`（只存 JSON 文本），资源目录由上层
//! 从工具注册表快照后传入。
//!
//! 约定：scope 是「可见性」而非权限。安全策略始终最后执行，且不可被任何层
//! 的 deny 覆盖。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// 当前声明格式版本。新增轴时递增，并保留旧版本的读取路径。
pub const SCOPE_SCHEMA_VERSION: u32 = 1;

/// 某一轴的裁剪模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMode {
    /// 不施加限制。
    #[default]
    Unrestricted,
    /// 只保留命中选择器的资源；选择器为空表示该轴无资源。
    Allow,
    /// 移除命中选择器的资源。
    Deny,
}

/// 工具轴的声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolScope {
    #[serde(default)]
    pub mode: ScopeMode,
    #[serde(default)]
    pub selectors: Vec<String>,
    /// 只允许这些 MCP server 的工具；空表示不按 server 限制。
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    /// 未命中的 MCP server 是否允许按需连接（由执行层消费）。
    #[serde(default)]
    pub on_demand_mcp: bool,
}

/// 提示词轴的声明：有序片段 id + 变量。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PromptScope {
    #[serde(default)]
    pub fragments: Vec<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

/// 记忆召回轴。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoryScope {
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// 预算轴：超出即稳定截断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScopeBudget {
    #[serde(default)]
    pub max_tools: Option<usize>,
    #[serde(default)]
    pub max_context_tokens: Option<usize>,
}

fn schema_version() -> u32 {
    SCOPE_SCHEMA_VERSION
}

/// 一份版本化作用域声明，可挂在会话、领域或意图上。
///
/// 序列化时保留 `schema_version`，便于未来迁移；未知字段被忽略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentScopeV1 {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub tools: ToolScope,
    #[serde(default)]
    pub prompts: PromptScope,
    #[serde(default)]
    pub memory: Option<MemoryScope>,
    #[serde(default)]
    pub budget: Option<ScopeBudget>,
    /// 预留轴：skills 收窄尚未实现，本轮恒为 `None`。
    #[serde(default)]
    pub skills: Option<serde_json::Value>,
}

impl Default for IntentScopeV1 {
    fn default() -> Self {
        Self {
            schema_version: SCOPE_SCHEMA_VERSION,
            tools: ToolScope::default(),
            prompts: PromptScope::default(),
            memory: None,
            budget: None,
            skills: None,
        }
    }
}

impl IntentScopeV1 {
    /// 把旧的 `intent.allowed_tools` 映射成 scope。
    ///
    /// 空列表返回 `None`（沿用「不限制」语义）；非空返回 `Allow` + 同名精确
    /// 选择器，保证缺少 scope 数据的旧库行为不变。
    /// 把一组工具名表达成「精确允许」的 scope（空列表 = 没有工具）。
    pub fn allow_exact(tools: &[String]) -> Self {
        Self {
            tools: ToolScope {
                mode: ScopeMode::Allow,
                selectors: tools.to_vec(),
                ..ToolScope::default()
            },
            ..Self::default()
        }
    }

    pub fn from_allowed_tools(tools: &[String]) -> Option<Self> {
        if tools.is_empty() {
            return None;
        }
        Some(Self {
            tools: ToolScope {
                mode: ScopeMode::Allow,
                selectors: tools.to_vec(),
                ..ToolScope::default()
            },
            ..Self::default()
        })
    }
}

/// 资源目录里的一个工具。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogTool {
    pub id: String,
    /// `builtin` 或 `mcp:<server>`。
    pub source: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 粗略 token 成本，仅用于预算排序。
    #[serde(default)]
    pub est_tokens: usize,
}

impl CatalogTool {
    pub fn new(id: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            enabled: true,
            tags: Vec::new(),
            est_tokens: 0,
        }
    }

    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|tag| tag.to_string()).collect();
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_cost(mut self, est_tokens: usize) -> Self {
        self.est_tokens = est_tokens;
        self
    }
}

/// 资源目录里的一个提示词片段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPrompt {
    pub id: String,
    #[serde(default)]
    pub description: String,
}

/// 可裁剪资源的只读快照。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceCatalog {
    tools: Vec<CatalogTool>,
    prompts: Vec<CatalogPrompt>,
}

impl ResourceCatalog {
    /// 由工具列表构造：按 id 去重（先到先得），并按 `(source, id)` 稳定排序。
    pub fn from_tools(tools: impl IntoIterator<Item = CatalogTool>) -> Self {
        let mut seen = BTreeSet::new();
        let mut tools: Vec<CatalogTool> = tools
            .into_iter()
            .filter(|tool| seen.insert(tool.id.clone()))
            .collect();
        tools.sort_by(|a, b| a.source.cmp(&b.source).then(a.id.cmp(&b.id)));
        Self {
            tools,
            prompts: Vec::new(),
        }
    }

    pub fn with_prompts(mut self, prompts: impl IntoIterator<Item = CatalogPrompt>) -> Self {
        self.prompts = prompts.into_iter().collect();
        self.prompts.sort_by(|a, b| a.id.cmp(&b.id));
        self
    }

    pub fn tools(&self) -> &[CatalogTool] {
        &self.tools
    }

    pub fn prompts(&self) -> &[CatalogPrompt] {
        &self.prompts
    }

    pub fn tools_by_source(&self, source: &str) -> Vec<&CatalogTool> {
        self.tools
            .iter()
            .filter(|tool| tool.source == source)
            .collect()
    }

    /// 按 id 取工具；不存在返回 `None`。
    pub fn find(&self, id: &str) -> Option<&CatalogTool> {
        self.tools.iter().find(|tool| tool.id == id)
    }
}

/// 判断选择器是否命中一个工具。
///
/// 支持：精确名、`<source>/*`、`builtin:*`、尾部 `*` 前缀、`tag:<tag>`。
/// 其余写法按精确名处理，匹配不到即不生效。
pub fn selector_matches(selector: &str, tool: &CatalogTool) -> bool {
    let selector = selector.trim();
    if selector.is_empty() {
        return false;
    }
    if let Some(tag) = selector.strip_prefix("tag:") {
        return !tag.is_empty() && tool.tags.iter().any(|item| item == tag);
    }
    if let Some(prefix) = selector.strip_suffix('*') {
        if prefix.is_empty() {
            return true;
        }
        let source_prefix = prefix.trim_end_matches(':').trim_end_matches('/');
        let qualified = format!("{}/{}", tool.source, tool.id);
        return tool.id.starts_with(prefix)
            || qualified.starts_with(prefix)
            || (!source_prefix.is_empty() && tool.source.starts_with(source_prefix));
    }
    tool.id == selector || format!("{}/{}", tool.source, tool.id) == selector
}

/// 裁剪发生在哪一层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeStage {
    Session,
    Domain,
    Intent,
    Draft,
    Safety,
    Budget,
}

/// 一条裁剪记录，用于解释「为什么这个资源不见了」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeRemoval {
    pub id: String,
    pub stage: ScopeStage,
    pub reason: String,
}

/// 安全策略：不受任何层收窄影响。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SafetyPolicy {
    pub force_include: Vec<String>,
    pub force_exclude: Vec<String>,
}

/// 路由模型给出的收窄建议：只能缩小，不能新增。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeDraft {
    pub tools: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
}

/// 一次解析的全部输入。
#[derive(Debug, Clone, Copy, Default)]
pub struct ScopeLayers<'a> {
    pub session: Option<&'a IntentScopeV1>,
    pub domain: Option<&'a IntentScopeV1>,
    pub intents: &'a [&'a IntentScopeV1],
    pub draft: Option<&'a ScopeDraft>,
    pub safety: Option<&'a SafetyPolicy>,
}

/// 解析结果：不可变、可序列化、可回放。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedScope {
    pub tools: Vec<String>,
    pub prompts: Vec<String>,
    pub memory_limit: Option<usize>,
    pub truncated: bool,
    pub removals: Vec<ScopeRemoval>,
}

impl ResolvedScope {
    pub fn allows_tool(&self, id: &str) -> bool {
        self.tools.iter().any(|tool| tool == id)
    }
}

/// 按 session → domain → intent（并集）→ draft → safety → budget 的顺序解析。
pub fn resolve(catalog: &ResourceCatalog, layers: ScopeLayers<'_>) -> ResolvedScope {
    let by_id: BTreeMap<&str, &CatalogTool> = catalog
        .tools
        .iter()
        .map(|tool| (tool.id.as_str(), tool))
        .collect();

    let mut allowed: BTreeSet<String> = catalog.tools.iter().map(|tool| tool.id.clone()).collect();
    let mut removals: Vec<ScopeRemoval> = Vec::new();

    // 外层两层逐级收窄。
    for (stage, scope) in [
        (ScopeStage::Session, layers.session),
        (ScopeStage::Domain, layers.domain),
    ] {
        if let Some(scope) = scope {
            apply_tool_scope(&mut allowed, catalog, &scope.tools, stage, &mut removals);
        }
    }

    // 多意图：allow 先并集再整体收窄，deny 取并集移除。
    let mut intent_allow: Option<BTreeSet<String>> = None;
    let mut intent_deny: BTreeSet<String> = BTreeSet::new();
    for scope in layers.intents {
        match scope.tools.mode {
            ScopeMode::Allow => {
                let matched = matching_ids(catalog, &scope.tools.selectors);
                intent_allow
                    .get_or_insert_with(BTreeSet::new)
                    .extend(matched);
            }
            ScopeMode::Deny => {
                intent_deny.extend(matching_ids(catalog, &scope.tools.selectors));
            }
            ScopeMode::Unrestricted => {}
        }
    }
    if let Some(union) = intent_allow {
        remove_where(&mut allowed, &union, false, ScopeStage::Intent, "not allowed by intent", &mut removals);
    }
    remove_where(&mut allowed, &intent_deny, true, ScopeStage::Intent, "denied by intent", &mut removals);

    // 意图层的 MCP server 白名单与工具选择器独立生效。
    let intent_servers: Vec<&String> = layers
        .intents
        .iter()
        .flat_map(|scope| scope.tools.mcp_servers.iter())
        .collect();
    if !intent_servers.is_empty() {
        let keep: BTreeSet<String> = catalog
            .tools
            .iter()
            .filter(|tool| {
                !tool.source.starts_with("mcp:")
                    || intent_servers
                        .iter()
                        .any(|server| tool.source == format!("mcp:{server}"))
            })
            .map(|tool| tool.id.clone())
            .collect();
        remove_where(
            &mut allowed,
            &keep,
            false,
            ScopeStage::Intent,
            "mcp server not allowed by intent",
            &mut removals,
        );
    }

    // draft 只能收窄。
    if let Some(draft) = layers.draft {
        if let Some(draft_tools) = &draft.tools {
            let keep: BTreeSet<String> = draft_tools.iter().cloned().collect();
            remove_where(&mut allowed, &keep, false, ScopeStage::Draft, "not selected by route draft", &mut removals);
        }
    }

    // 安全策略最后执行。
    if let Some(safety) = layers.safety {
        for id in &safety.force_exclude {
            if allowed.remove(id) {
                removals.push(ScopeRemoval {
                    id: id.clone(),
                    stage: ScopeStage::Safety,
                    reason: "excluded by safety policy".to_string(),
                });
            }
        }
        for id in &safety.force_include {
            if by_id.contains_key(id.as_str()) && allowed.insert(id.clone()) {
                removals.push(ScopeRemoval {
                    id: id.clone(),
                    stage: ScopeStage::Safety,
                    reason: "forced by safety policy".to_string(),
                });
            }
        }
    }

    // 预算：先按 max_tools 截断，再按 token 成本截断；顺序稳定。
    let budget = merged_budget(layers);
    let mut truncated = false;
    let mut ordered = ordered_tools(&allowed, &by_id);
    if let Some(limit) = budget.as_ref().and_then(|budget| budget.max_tools) {
        if ordered.len() > limit {
            for tool in ordered.split_off(limit) {
                allowed.remove(&tool.id);
                removals.push(ScopeRemoval {
                    id: tool.id.clone(),
                    stage: ScopeStage::Budget,
                    reason: "truncated by max_tools".to_string(),
                });
            }
            truncated = true;
        }
    }
    if let Some(max_tokens) = budget.as_ref().and_then(|budget| budget.max_context_tokens) {
        let mut used = 0usize;
        let mut dropped: Vec<CatalogTool> = Vec::new();
        for tool in ordered_tools(&allowed, &by_id) {
            if used + tool.est_tokens > max_tokens {
                dropped.push(tool);
            } else {
                used += tool.est_tokens;
            }
        }
        for tool in dropped {
            allowed.remove(&tool.id);
            removals.push(ScopeRemoval {
                id: tool.id.clone(),
                stage: ScopeStage::Budget,
                reason: "truncated by max_context_tokens".to_string(),
            });
            truncated = true;
        }
    }

    // 输出按 (source, id) 排序，保证同一输入字节级相等。
    let mut tools: Vec<String> = allowed.into_iter().collect();
    tools.sort_by(|a, b| {
        let left = by_id.get(a.as_str());
        let right = by_id.get(b.as_str());
        match (left, right) {
            (Some(left), Some(right)) => left
                .source
                .cmp(&right.source)
                .then(left.id.cmp(&right.id)),
            _ => a.cmp(b),
        }
    });

    removals.sort_by(|a, b| a.stage.cmp(&b.stage).then(a.id.cmp(&b.id)));

    ResolvedScope {
        tools,
        prompts: resolve_prompts(layers),
        memory_limit: merged_memory_limit(layers),
        truncated,
        removals,
    }
}

/// 从当前集合里移除 `subject` 中的元素（`invert = true` 时移除 `subject` 之外的元素）。
fn remove_where(
    allowed: &mut BTreeSet<String>,
    subject: &BTreeSet<String>,
    invert: bool,
    stage: ScopeStage,
    reason: &str,
    removals: &mut Vec<ScopeRemoval>,
) {
    let victims: Vec<String> = allowed
        .iter()
        .filter(|id| subject.contains(*id) == invert)
        .cloned()
        .collect();
    for id in victims {
        allowed.remove(&id);
        removals.push(ScopeRemoval {
            id,
            stage,
            reason: reason.to_string(),
        });
    }
}

fn matching_ids(catalog: &ResourceCatalog, selectors: &[String]) -> BTreeSet<String> {
    catalog
        .tools
        .iter()
        .filter(|tool| selectors.iter().any(|selector| selector_matches(selector, tool)))
        .map(|tool| tool.id.clone())
        .collect()
}

fn apply_tool_scope(
    allowed: &mut BTreeSet<String>,
    catalog: &ResourceCatalog,
    scope: &ToolScope,
    stage: ScopeStage,
    removals: &mut Vec<ScopeRemoval>,
) {
    match scope.mode {
        ScopeMode::Unrestricted => {}
        ScopeMode::Allow => {
            let keep = matching_ids(catalog, &scope.selectors);
            remove_where(allowed, &keep, false, stage, "not allowed by scope", removals);
        }
        ScopeMode::Deny => {
            let deny = matching_ids(catalog, &scope.selectors);
            remove_where(allowed, &deny, true, stage, "denied by scope", removals);
        }
    }

    // server 白名单是独立约束，即使 mode 为 unrestricted 也生效。
    if !scope.mcp_servers.is_empty() {
        let keep: BTreeSet<String> = catalog
            .tools
            .iter()
            .filter(|tool| {
                !tool.source.starts_with("mcp:")
                    || scope
                        .mcp_servers
                        .iter()
                        .any(|server| tool.source == format!("mcp:{server}"))
            })
            .map(|tool| tool.id.clone())
            .collect();
        remove_where(allowed, &keep, false, stage, "mcp server not allowed", removals);
    }
}

fn ordered_tools<'a>(
    allowed: &BTreeSet<String>,
    by_id: &BTreeMap<&'a str, &'a CatalogTool>,
) -> Vec<CatalogTool> {
    let mut tools: Vec<CatalogTool> = allowed
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).map(|tool| (**tool).clone()))
        .collect();
    tools.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then(a.est_tokens.cmp(&b.est_tokens))
            .then(a.id.cmp(&b.id))
    });
    tools
}

/// 取所有层里最严格的预算（同轴取最小值）。
fn merged_budget(layers: ScopeLayers<'_>) -> Option<ScopeBudget> {
    let scopes = layers
        .session
        .into_iter()
        .chain(layers.domain)
        .chain(layers.intents.iter().copied());
    let mut merged = ScopeBudget::default();
    let mut seen = false;
    for budget in scopes.filter_map(|scope| scope.budget.as_ref()) {
        seen = true;
        merged.max_tools = min_option(merged.max_tools, budget.max_tools);
        merged.max_context_tokens =
            min_option(merged.max_context_tokens, budget.max_context_tokens);
    }
    seen.then_some(merged)
}

fn merged_memory_limit(layers: ScopeLayers<'_>) -> Option<usize> {
    let scopes = layers
        .session
        .into_iter()
        .chain(layers.domain)
        .chain(layers.intents.iter().copied());
    let mut limit: Option<usize> = None;
    for candidate in scopes.filter_map(|scope| scope.memory.as_ref().and_then(|m| m.limit)) {
        limit = min_option(limit, Some(candidate));
    }
    limit
}

fn min_option(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

/// 提示词片段是「追加文本」而非权限，因此按声明顺序并集；draft 只做过滤。
fn resolve_prompts(layers: ScopeLayers<'_>) -> Vec<String> {
    let mut fragments: Vec<String> = Vec::new();
    let scopes = layers
        .session
        .into_iter()
        .chain(layers.domain)
        .chain(layers.intents.iter().copied());
    for fragment in scopes.flat_map(|scope| scope.prompts.fragments.iter()) {
        if !fragments.contains(fragment) {
            fragments.push(fragment.clone());
        }
    }
    if let Some(draft_prompts) = layers.draft.and_then(|draft| draft.prompts.as_ref()) {
        fragments.retain(|fragment| draft_prompts.contains(fragment));
    }
    fragments
}

/// 资源目录指纹：目录内容变化时缓存必须失效。
///
/// 使用 FNV-1a 64 位对 `(source, id)` 序列求值，稳定且无额外依赖。
impl ResourceCatalog {
    pub fn signature(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for tool in &self.tools {
            for byte in tool.source.as_bytes().iter().chain(b"/").chain(tool.id.as_bytes()) {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x1000_0000_01b3);
            }
        }
        hash
    }
}

/// 第二级路由（资源收窄）的系统提示词：列出候选资源，只要求返回 id 数组。
pub fn build_scope_prompt(resources: &[CatalogTool]) -> String {
    let mut prompt = String::from(
        "You are narrowing the tool set for ONE request of an AI coding assistant.\n\
         Below is every tool available in the current domain. Pick only the tools this\n\
         request actually needs; calling tools outside your pick is impossible.\n\
         Prefer fewer tools. If the request needs no tool, answer with an empty array.\n\
         Reply with a JSON array of tool ids and nothing else.\n\nAvailable tools:\n",
    );
    for tool in resources {
        prompt.push_str(&format!("- {} ({})\n", tool.id, tool.source));
    }
    prompt
}

/// 解析第二级路由的回复。
///
/// 只接受字符串数组（可被对象 `{"tools": [...]}` 包裹）；未知 id 被丢弃，
/// 空数组与不可解析的回复都返回 `None`——收窄失败时不能把工具全裁掉。
pub fn parse_scope_reply(reply: &str, resources: &[CatalogTool]) -> Option<ScopeDraft> {
    let value = extract_json(reply)?;
    let raw = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(map) => map.get("tools")?.as_array()?.clone(),
        _ => return None,
    };

    let valid: BTreeSet<&str> = resources.iter().map(|tool| tool.id.as_str()).collect();
    let mut selected: Vec<String> = Vec::new();
    for item in raw {
        let Some(id) = item.as_str() else { continue };
        if valid.contains(id) && !selected.iter().any(|known| known == id) {
            selected.push(id.to_string());
        }
    }
    if selected.is_empty() {
        return None;
    }
    Some(ScopeDraft {
        tools: Some(selected),
        prompts: None,
    })
}

/// 从可能带 markdown 围栏或前后说明的回复里取出第一个 JSON 值。
fn extract_json(reply: &str) -> Option<serde_json::Value> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(reply.trim()) {
        return Some(value);
    }
    let start = reply.find(['[', '{'])?;
    let end = reply.rfind([']', '}'])?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&reply[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_catalog() -> ResourceCatalog {
        ResourceCatalog::from_tools([
            CatalogTool::new("read", "builtin").with_tags(&["filesystem"]).with_cost(40),
            CatalogTool::new("write", "builtin").with_tags(&["filesystem"]).with_cost(80),
            CatalogTool::new("shell", "builtin").with_cost(120),
            CatalogTool::new("create_issue", "mcp:linear").with_cost(60),
            CatalogTool::new("delete_issue", "mcp:linear").with_cost(60),
            CatalogTool::new("search_repo", "mcp:github").with_cost(60),
        ])
    }

    fn allow(selectors: &[&str]) -> IntentScopeV1 {
        IntentScopeV1 {
            tools: ToolScope {
                mode: ScopeMode::Allow,
                selectors: selectors.iter().map(|s| s.to_string()).collect(),
                ..ToolScope::default()
            },
            ..IntentScopeV1::default()
        }
    }

    fn deny(selectors: &[&str]) -> IntentScopeV1 {
        IntentScopeV1 {
            tools: ToolScope {
                mode: ScopeMode::Deny,
                selectors: selectors.iter().map(|s| s.to_string()).collect(),
                ..ToolScope::default()
            },
            ..IntentScopeV1::default()
        }
    }

    fn resolve_with(layers: ScopeLayers<'_>) -> ResolvedScope {
        let catalog = sample_catalog();
        resolve(&catalog, layers)
    }

    #[test]
    fn minimal_json_defaults_to_unrestricted() {
        let scope: IntentScopeV1 = serde_json::from_str(r#"{"schema_version":1}"#).unwrap();
        assert_eq!(scope.tools.mode, ScopeMode::Unrestricted);
        assert!(scope.prompts.fragments.is_empty());
        assert!(scope.memory.is_none());
        assert!(scope.budget.is_none());
        assert!(scope.skills.is_none(), "skills 轴本轮必须为空");
    }

    #[test]
    fn empty_json_uses_schema_version_default() {
        let scope: IntentScopeV1 = serde_json::from_str("{}").unwrap();
        assert_eq!(scope.schema_version, SCOPE_SCHEMA_VERSION);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let scope: IntentScopeV1 =
            serde_json::from_str(r#"{"schema_version":1,"future_axis":{"x":1}}"#).unwrap();
        assert_eq!(scope.schema_version, 1);
    }

    #[test]
    fn scope_round_trips_through_json() {
        let scope = IntentScopeV1 {
            tools: ToolScope {
                mode: ScopeMode::Allow,
                selectors: vec!["mcp:linear/*".to_string()],
                mcp_servers: vec!["linear".to_string()],
                on_demand_mcp: true,
            },
            prompts: PromptScope {
                fragments: vec!["base".to_string(), "style".to_string()],
                variables: BTreeMap::from([("lang".to_string(), "zh".to_string())]),
            },
            memory: Some(MemoryScope {
                domain: Some("work".to_string()),
                limit: Some(3),
            }),
            budget: Some(ScopeBudget {
                max_tools: Some(8),
                max_context_tokens: Some(2048),
            }),
            ..IntentScopeV1::default()
        };
        let json = serde_json::to_string(&scope).unwrap();
        let back: IntentScopeV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(scope, back);
    }

    #[test]
    fn selector_matches_exact_and_prefix() {
        let catalog = sample_catalog();
        let read = catalog.find("read").unwrap();
        assert!(selector_matches("read", read));
        assert!(selector_matches("rea*", read));
        assert!(selector_matches("builtin:*", read));
        assert!(selector_matches("builtin/read", read));
        assert!(!selector_matches("write", read));
    }

    #[test]
    fn selector_matches_mcp_server_wildcard() {
        let catalog = sample_catalog();
        let linear = catalog.find("create_issue").unwrap();
        let github = catalog.find("search_repo").unwrap();
        assert!(selector_matches("mcp:linear/*", linear));
        assert!(!selector_matches("mcp:linear/*", github));
    }

    #[test]
    fn selector_matches_tag() {
        let catalog = sample_catalog();
        let read = catalog.find("read").unwrap();
        assert!(selector_matches("tag:filesystem", read));
        assert!(!selector_matches("tag:network", read));
    }

    #[test]
    fn unrecognized_selector_matches_nothing() {
        let catalog = sample_catalog();
        let read = catalog.find("read").unwrap();
        assert!(!selector_matches("fs::{read,write}", read));
        assert!(!selector_matches("", read));
    }

    #[test]
    fn empty_allow_list_yields_no_tools() {
        let scope = IntentScopeV1 {
            tools: ToolScope {
                mode: ScopeMode::Allow,
                selectors: vec![],
                ..ToolScope::default()
            },
            ..IntentScopeV1::default()
        };
        let resolved = resolve_with(ScopeLayers {
            intents: &[&scope],
            ..ScopeLayers::default()
        });
        assert!(resolved.tools.is_empty());
    }

    #[test]
    fn deny_overrides_allow_in_same_layer() {
        let allowed = allow(&["mcp:linear/*"]);
        let denied = deny(&["mcp:linear/delete_issue"]);
        let resolved = resolve_with(ScopeLayers {
            intents: &[&allowed, &denied],
            ..ScopeLayers::default()
        });
        assert!(resolved.allows_tool("create_issue"));
        assert!(!resolved.allows_tool("delete_issue"));
    }

    #[test]
    fn intent_cannot_exceed_session() {
        let session = allow(&["read", "write"]);
        let intent = allow(&["read", "write", "shell"]);
        let resolved = resolve_with(ScopeLayers {
            session: Some(&session),
            intents: &[&intent],
            ..ScopeLayers::default()
        });
        assert_eq!(resolved.tools, vec!["read".to_string(), "write".to_string()]);
    }

    #[test]
    fn multiple_intents_union_tools() {
        let first = allow(&["read"]);
        let second = allow(&["write"]);
        let resolved = resolve_with(ScopeLayers {
            intents: &[&first, &second],
            ..ScopeLayers::default()
        });
        assert_eq!(resolved.tools, vec!["read".to_string(), "write".to_string()]);
    }

    #[test]
    fn draft_cannot_add_tools() {
        let intent = allow(&["read"]);
        let draft = ScopeDraft {
            tools: Some(vec!["read".to_string(), "shell".to_string()]),
            prompts: None,
        };
        let resolved = resolve_with(ScopeLayers {
            intents: &[&intent],
            draft: Some(&draft),
            ..ScopeLayers::default()
        });
        assert_eq!(resolved.tools, vec!["read".to_string()]);
    }

    #[test]
    fn draft_narrows_within_unrestricted_catalog() {
        let draft = ScopeDraft {
            tools: Some(vec!["shell".to_string()]),
            prompts: None,
        };
        let resolved = resolve_with(ScopeLayers {
            draft: Some(&draft),
            ..ScopeLayers::default()
        });
        assert_eq!(resolved.tools, vec!["shell".to_string()]);
    }

    #[test]
    fn safety_force_include_beats_intent_deny() {
        let denied = deny(&["read"]);
        let safety = SafetyPolicy {
            force_include: vec!["read".to_string()],
            force_exclude: vec![],
        };
        let resolved = resolve_with(ScopeLayers {
            intents: &[&denied],
            safety: Some(&safety),
            ..ScopeLayers::default()
        });
        assert!(resolved.allows_tool("read"));
        assert!(resolved
            .removals
            .iter()
            .any(|removal| removal.stage == ScopeStage::Safety && removal.id == "read"));
    }

    #[test]
    fn safety_force_exclude_wins() {
        let safety = SafetyPolicy {
            force_include: vec![],
            force_exclude: vec!["shell".to_string()],
        };
        let resolved = resolve_with(ScopeLayers {
            safety: Some(&safety),
            ..ScopeLayers::default()
        });
        assert!(!resolved.allows_tool("shell"));
    }

    #[test]
    fn budget_truncates_stably() {
        let scope = IntentScopeV1 {
            budget: Some(ScopeBudget {
                max_tools: Some(3),
                max_context_tokens: None,
            }),
            ..IntentScopeV1::default()
        };
        let layers = ScopeLayers {
            intents: &[&scope],
            ..ScopeLayers::default()
        };
        let first = resolve_with(layers);
        let second = resolve_with(layers);
        assert_eq!(first, second, "相同输入必须字节级相等");
        assert_eq!(first.tools.len(), 3);
        assert!(first.truncated);
        // 成本最低的 read(40) / create_issue(60) / delete_issue(60) 优先保留。
        assert!(first.allows_tool("read"));
        assert!(!first.allows_tool("shell"));
    }

    #[test]
    fn token_budget_truncates_expensive_tools() {
        let scope = IntentScopeV1 {
            budget: Some(ScopeBudget {
                max_tools: None,
                max_context_tokens: Some(150),
            }),
            ..IntentScopeV1::default()
        };
        let resolved = resolve_with(ScopeLayers {
            intents: &[&scope],
            ..ScopeLayers::default()
        });
        assert!(resolved.truncated);
        assert!(resolved.allows_tool("read"));
        assert!(!resolved.allows_tool("shell"));
    }

    #[test]
    fn prompts_are_merged_in_declaration_order() {
        let domain = IntentScopeV1 {
            prompts: PromptScope {
                fragments: vec!["base".to_string(), "style".to_string()],
                ..PromptScope::default()
            },
            ..IntentScopeV1::default()
        };
        let intent = IntentScopeV1 {
            prompts: PromptScope {
                fragments: vec!["style".to_string(), "task".to_string()],
                ..PromptScope::default()
            },
            ..IntentScopeV1::default()
        };
        let resolved = resolve_with(ScopeLayers {
            domain: Some(&domain),
            intents: &[&intent],
            ..ScopeLayers::default()
        });
        assert_eq!(resolved.prompts, vec!["base", "style", "task"]);
    }

    #[test]
    fn memory_limit_takes_minimum() {
        let first = IntentScopeV1 {
            memory: Some(MemoryScope {
                domain: None,
                limit: Some(5),
            }),
            ..IntentScopeV1::default()
        };
        let second = IntentScopeV1 {
            memory: Some(MemoryScope {
                domain: None,
                limit: Some(2),
            }),
            ..IntentScopeV1::default()
        };
        let resolved = resolve_with(ScopeLayers {
            intents: &[&first, &second],
            ..ScopeLayers::default()
        });
        assert_eq!(resolved.memory_limit, Some(2));
    }

    #[test]
    fn from_allowed_tools_empty_is_none() {
        assert!(IntentScopeV1::from_allowed_tools(&[]).is_none());
    }

    #[test]
    fn from_allowed_tools_non_empty_is_allow() {
        let scope = IntentScopeV1::from_allowed_tools(&["read_file".to_string()]).unwrap();
        assert_eq!(scope.tools.mode, ScopeMode::Allow);
        assert_eq!(scope.tools.selectors, vec!["read_file".to_string()]);
    }

    #[test]
    fn allow_exact_keeps_empty_list_as_allow() {
        let scope = IntentScopeV1::allow_exact(&[]);
        assert_eq!(scope.tools.mode, ScopeMode::Allow);
        assert!(scope.tools.selectors.is_empty());
    }

    #[test]
    fn catalog_deduplicates_and_sorts() {
        let catalog = ResourceCatalog::from_tools([
            CatalogTool::new("write", "builtin"),
            CatalogTool::new("read", "builtin"),
            CatalogTool::new("read", "mcp:other"),
        ]);
        let ids: Vec<&str> = catalog.tools().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["read", "write"]);
        assert_eq!(catalog.tools_by_source("mcp:linear").len(), 0);
    }

    #[test]
    fn resolve_matches_intersect_tools_semantics() {
        use crate::llm::intent::router::intersect_tools;

        let session = vec!["read".to_string(), "write".to_string()];
        let intent = vec!["read".to_string()];
        let catalog = ResourceCatalog::from_tools(
            session
                .iter()
                .map(|name| CatalogTool::new(name.clone(), "builtin")),
        );

        let session_scope = IntentScopeV1::from_allowed_tools(&session).unwrap();
        let intent_scope = IntentScopeV1::from_allowed_tools(&intent).unwrap();

        let both = resolve(
            &catalog,
            ScopeLayers {
                session: Some(&session_scope),
                intents: &[&intent_scope],
                ..ScopeLayers::default()
            },
        );
        assert_eq!(
            both.tools,
            intersect_tools(Some(session.clone()), Some(&intent)).unwrap()
        );

        let session_only = resolve(
            &catalog,
            ScopeLayers {
                session: Some(&session_scope),
                ..ScopeLayers::default()
            },
        );
        assert_eq!(
            session_only.tools,
            intersect_tools(Some(session), None).unwrap()
        );

        let intent_only = resolve(
            &catalog,
            ScopeLayers {
                intents: &[&intent_scope],
                ..ScopeLayers::default()
            },
        );
        assert_eq!(
            intent_only.tools,
            intersect_tools(None, Some(&intent)).unwrap()
        );
    }

    #[test]
    fn mcp_server_whitelist_applies_even_when_mode_unrestricted() {
        let scope = IntentScopeV1 {
            tools: ToolScope {
                mode: ScopeMode::Unrestricted,
                selectors: vec![],
                mcp_servers: vec!["linear".to_string()],
                on_demand_mcp: true,
            },
            ..IntentScopeV1::default()
        };
        let resolved = resolve_with(ScopeLayers {
            intents: &[&scope],
            ..ScopeLayers::default()
        });
        assert!(resolved.allows_tool("create_issue"));
        assert!(resolved.allows_tool("read"), "内置工具不受 MCP 白名单影响");
        assert!(!resolved.allows_tool("search_repo"));
    }
}

#[cfg(test)]
mod scope_prompt_tests {
    use super::*;

    fn resources() -> Vec<CatalogTool> {
        vec![
            CatalogTool::new("read", "builtin"),
            CatalogTool::new("shell", "builtin"),
            CatalogTool::new("create_issue", "mcp:linear"),
        ]
    }

    #[test]
    fn prompt_lists_resource_ids_and_sources() {
        let prompt = build_scope_prompt(&resources());
        assert!(prompt.contains("- read (builtin)"));
        assert!(prompt.contains("- create_issue (mcp:linear)"));
    }

    #[test]
    fn parses_plain_array() {
        let draft = parse_scope_reply(r#"["read","create_issue"]"#, &resources()).unwrap();
        assert_eq!(
            draft.tools,
            Some(vec!["read".to_string(), "create_issue".to_string()])
        );
    }

    #[test]
    fn parses_fenced_reply_with_prose() {
        let reply = "Sure:\n```json\n{\"tools\":[\"shell\"]}\n```";
        let draft = parse_scope_reply(reply, &resources()).unwrap();
        assert_eq!(draft.tools, Some(vec!["shell".to_string()]));
    }

    #[test]
    fn drops_unknown_and_duplicate_ids() {
        let draft =
            parse_scope_reply(r#"["read","read","not_a_tool"]"#, &resources()).unwrap();
        assert_eq!(draft.tools, Some(vec!["read".to_string()]));
    }

    #[test]
    fn empty_or_invalid_reply_yields_none() {
        assert!(parse_scope_reply("[]", &resources()).is_none());
        assert!(parse_scope_reply("no idea", &resources()).is_none());
        assert!(parse_scope_reply(r#"{"tools":[]}"#, &resources()).is_none());
    }

    #[test]
    fn signature_changes_when_catalog_changes() {
        let first = ResourceCatalog::from_tools(resources());
        let second = ResourceCatalog::from_tools([CatalogTool::new("read", "builtin")]);
        assert_ne!(first.signature(), second.signature());
        assert_eq!(first.signature(), first.clone().signature());
    }
}
