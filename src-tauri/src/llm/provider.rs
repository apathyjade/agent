use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use rig::agent::StreamingResult;
use rig::completion::{Message, ToolAnnotations};
use rig::tool::DynamicTool;
use rig::__core::tool::Touches;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::core::error::{AppError, Result};
use crate::llm::model::ModelConfig;

use super::rig::{create_rig_provider_with_approval};

// ---------------------------------------------------------------------------
// 工具策略：审批判定与运行时输入
// ---------------------------------------------------------------------------

/// 由 JSON 参数算出本次调用碰到的东西；参数解析失败时返回 `None`。
pub type FootprintFn = Arc<dyn Fn(&Value) -> Option<Touches> + Send + Sync>;

/// 策略层需要的工具元数据。
#[derive(Clone)]
pub struct ToolPolicyMeta {
    /// 工具级风险注解。
    pub annotations: ToolAnnotations,
    /// 按参数计算的调用级范围。
    pub footprint: Option<FootprintFn>,
}

/// 注册名 → 策略元数据。
pub type ToolMetaMap = HashMap<String, ToolPolicyMeta>;

/// 审批名单。
#[derive(Debug, Clone, Default)]
pub struct ToolPolicy {
    /// 无条件拒绝。
    pub deny_list: Vec<String>,
    /// 无条件放行。
    pub allow_list: Vec<String>,
    /// 必须人工确认。
    pub confirm_list: Vec<String>,
}

/// 一次工具调用的审批结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// 直接执行。
    Allow,
    /// 需要人工确认。
    Confirm,
    /// 拒绝执行，附带模型可见的原因。
    Deny(String),
}

/// 判定一次工具调用是否需要确认。
///
/// 优先级：拒绝名单 → 放行名单 → 确认名单 → 调用级 footprint →
/// 工具级 `destructive` 注解 → 放行。
pub fn decide(
    policy: &ToolPolicy,
    tool_name: &str,
    annotations: &ToolAnnotations,
    footprint: Option<&Touches>,
) -> ApprovalDecision {
    if policy.deny_list.iter().any(|name| name == tool_name) {
        return ApprovalDecision::Deny(format!(
            "Tool '{}' is denied by policy",
            tool_name
        ));
    }
    if policy.allow_list.iter().any(|name| name == tool_name) {
        return ApprovalDecision::Allow;
    }
    if policy.confirm_list.iter().any(|name| name == tool_name) {
        return ApprovalDecision::Confirm;
    }

    match footprint {
        // 有调用级信息时以它为准：只读调用不该被工具级 destructive 拖累。
        Some(Touches::Writes(_)) => ApprovalDecision::Confirm,
        Some(Touches::Reads(_)) | Some(Touches::Independent) => ApprovalDecision::Allow,
        None => {
            if annotations.destructive() {
                ApprovalDecision::Confirm
            } else {
                ApprovalDecision::Allow
            }
        }
    }
}

/// 人工确认通道（由 agent 层实现，llm 层只依赖这个接口）。
#[async_trait]
pub trait ApprovalGate: Send + Sync {
    /// 当前审批名单。
    fn policy(&self) -> &ToolPolicy;

    /// 请求确认；返回 `false` 表示拒绝或等待超时。
    async fn request(&self, call_id: &str, tool_name: &str, arguments: &Value, reason: &str)
        -> bool;
}

/// 交给 Provider 的工具集合与策略信息。
pub struct ToolRuntime {
    /// 交给 Rig 执行的工具。
    pub tools: Vec<DynamicTool>,
    /// 策略层元数据。
    pub meta: Arc<ToolMetaMap>,
    /// 审批通道；`None` 表示不启用人工确认。
    pub approval: Option<Arc<dyn ApprovalGate>>,
}

impl ToolRuntime {
    /// 没有工具的空集合。
    pub fn empty() -> Self {
        Self {
            tools: Vec::new(),
            meta: Arc::new(HashMap::new()),
            approval: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 一轮工具执行的结果
// ---------------------------------------------------------------------------

/// 一次工具调用的终态。
///
/// 只区分「跑完了 / 失败了 / 被拒了 / 被策略拦了」——具体原因在 transcript
/// 的工具结果文本里，这里只做状态归类，供落库与前端展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// 工具执行成功。
    Completed,
    /// 工具执行失败（工具自身报错）。
    Failed,
    /// 用户拒绝，或等待确认超时。
    Rejected,
    /// 策略 / 白名单 / 并行冲突阻止，未执行。
    PolicyDenied,
}

/// 一轮工具执行的结果：最终文本 + 本轮 transcript + 策略判定记录。
pub struct TurnOutcome {
    /// 模型的最终回答。
    pub output: String,
    /// 本轮产生的消息（assistant 轮次 + 工具结果），供落库用。
    ///
    /// 不含本轮开始前已有的历史——这是「要追加到会话历史里的部分」。
    pub messages: Vec<Message>,
    /// 本轮每个工具调用的终态（按 call id）。
    pub decisions: HashMap<String, ToolCallStatus>,
}

impl TurnOutcome {
    /// 没有工具调用的普通回答。
    pub fn text(output: String) -> Self {
        Self {
            output,
            messages: Vec::new(),
            decisions: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// 对象安全 trait —— 把 Rig 的 completion model 包在统一接口后面，
// 让异构供应商能共存在同一个注册表里。
// ---------------------------------------------------------------------------

/// 对象安全的供应商接口，用于对 Rig 模型做类型擦除访问。
///
/// 唯一的实现 [`RigProvider`](super::rig::RigProvider) 覆盖所有后端，因此
/// [`ProviderRegistry`] 每个已配置模型只存一个装箱的 trait object。
#[async_trait]
pub trait ProviderBox: Send + Sync {
    /// 模型标识（例如 `"gpt-4o"`、`"claude-3-opus"`）。
    fn model_id(&self) -> &str;

    /// 单轮 prompt —— 系统前言 + 用户文本 → 助手文本。
    async fn prompt(&self, preamble: &str, prompt: &str) -> Result<String>;

    /// 带对话历史的多轮 chat。
    async fn chat(
        &self,
        preamble: &str,
        prompt: &str,
        history: &mut Vec<Message>,
    ) -> Result<String>;

    /// 支持工具的单轮 prompt。
    /// 多轮工具执行由 Rig 的 Agent 内部负责。
    async fn prompt_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tools: ToolRuntime,
        max_turns: usize,
    ) -> Result<TurnOutcome>;

    /// 支持工具的多轮 chat。
    /// Rig 在内部执行请求的工具调用，并把工具结果回灌进历史，直到模型停止。
    ///
    /// `history` 只读：Rig 自己维护本轮 transcript，调用方拿到
    /// [`TurnOutcome::messages`] 后自行决定如何合并。
    async fn chat_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        history: &[Message],
        tools: ToolRuntime,
        max_turns: usize,
    ) -> Result<TurnOutcome>;

    /// 流式单轮 prompt（不带工具）。
    async fn stream_prompt(
        &self,
        preamble: &str,
        prompt: &str,
    ) -> Result<BoxStream<'static, Result<String>>>;

    /// 支持工具的流式 prompt。产出原始 agent 流元素（文本增量、工具调用、工具
    /// 结果），并以最终响应或错误结束。
    async fn stream_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        history: &[Message],
        tools: ToolRuntime,
        max_turns: usize,
    ) -> Result<StreamingResult>;
}

// ---------------------------------------------------------------------------
// ProviderRegistry
// ---------------------------------------------------------------------------

/// 已配置模型的注册表：`model.id` → 装箱的 [`ProviderBox`]。
///
/// 查找是**精确匹配且区分大小写**的：键就是 [`ModelConfig::id`] 原样，不做别名
/// 归一化，也不做大小写折叠。因此 `"GPT-4o"` 与 `"gpt-4o"` 是两个不同的键。
/// 未注册的 id 会得到 [`AppError::Provider`]，而不是 `None`。
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ProviderBox>>,
    default_model_id: Option<String>,
    /// 人工确认通道；所有 provider 共享同一份。
    approval: Option<Arc<dyn ApprovalGate>>,
}

impl ProviderRegistry {
    /// 按配置批量创建供应商（不启用人工确认）。
    pub fn new(models: &[ModelConfig]) -> Self {
        Self::new_with_approval(models, None)
    }

    /// 按配置批量创建供应商，并注入共享的人工确认通道。
    pub fn new_with_approval(
        models: &[ModelConfig],
        approval: Option<Arc<dyn ApprovalGate>>,
    ) -> Self {
        let mut providers = HashMap::new();

        for model in models {
            if !model.enabled {
                continue;
            }

            match create_rig_provider_with_approval(model, approval.clone()) {
                Ok(provider) => {
                    providers.insert(model.id.clone(), Arc::from(provider));
                }
                Err(e) => {
                    log::warn!(
                        "Failed to create Rig provider for '{}' ({}): {}",
                        model.id, model.provider, e
                    );
                    continue;
                }
            }
        }

        let default_id = crate::llm::model::default_model(models).map(|m| m.id.clone());

        Self {
            providers,
            default_model_id: default_id,
            approval,
        }
    }

    /// 共享的人工确认通道。
    pub fn approval(&self) -> Option<Arc<dyn ApprovalGate>> {
        self.approval.clone()
    }

    /// 默认模型的 id；没有默认模型时返回空串。
    pub fn default_model_id(&self) -> &str {
        self.default_model_id.as_deref().unwrap_or("")
    }

    /// 按 `model_id` 精确查找供应商；未注册时返回 [`AppError::Provider`]。
    pub fn get(&self, model_id: &str) -> Result<Arc<dyn ProviderBox>> {
        self.providers
            .get(model_id)
            .cloned()
            .ok_or_else(|| {
                AppError::Provider(format!(
                    "Model '{}' not found or not configured",
                    model_id
                ))
            })
    }

    /// 按 `model_id` 解析供应商；`None` 时回退到默认模型。
    ///
    /// 默认模型也为空（即没有任何可用配置）时报错。
    pub fn resolve(&self, model_id: Option<&str>) -> Result<Arc<dyn ProviderBox>> {
        let mid = model_id.unwrap_or_else(|| self.default_model_id());
        if mid.is_empty() {
            return Err(AppError::Provider("No model configured".into()));
        }
        self.get(mid)
    }

    /// 列出已注册的模型 id（顺序不保证）。
    pub fn list_models(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// 注册一个新模型。`enabled == false` 时忽略；创建失败只记 warn。
    pub fn add_model(&mut self, model: ModelConfig) {
        if !model.enabled {
            return;
        }

        match create_rig_provider_with_approval(&model, self.approval.clone()) {
            Ok(provider) => {
                self.providers.insert(model.id.clone(), Arc::from(provider));
            }
            Err(e) => {
                log::warn!("Failed to create Rig provider for '{}': {}", model.id, e);
            }
        }
    }

    /// 按 `model_id` 注销模型；不存在时是空操作。
    pub fn remove_model(&mut self, model_id: &str) {
        self.providers.remove(model_id);
    }

    /// `model_id` 是否已注册（精确匹配）。
    pub fn is_registered(&self, model_id: &str) -> bool {
        self.providers.contains_key(model_id)
    }

    /// 与 [`Self::list_models`] 相同，保留旧名字供调用方使用。
    pub fn get_registered_model_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::__core::tool::{Scope, Touches};
    use std::path::PathBuf;

    fn annotations(destructive: bool, read_only: bool) -> ToolAnnotations {
        ToolAnnotations::default()
            .with_read_only(read_only)
            .with_destructive(destructive)
            .with_open_world(false)
    }

    fn policy() -> ToolPolicy {
        ToolPolicy {
            deny_list: vec!["danger".to_string()],
            allow_list: vec!["trusted".to_string()],
            confirm_list: vec!["ask".to_string()],
        }
    }

    #[test]
    fn deny_list_wins_over_allow_list() {
        let policy = ToolPolicy {
            deny_list: vec!["tool".to_string()],
            allow_list: vec!["tool".to_string()],
            confirm_list: Vec::new(),
        };

        assert!(matches!(
            decide(&policy, "tool", &annotations(false, true), None),
            ApprovalDecision::Deny(_)
        ));
    }

    #[test]
    fn allow_list_bypasses_confirm_and_destructive() {
        assert_eq!(
            decide(
                &policy(),
                "trusted",
                &annotations(true, false),
                Some(&Touches::Writes(Scope::Anything))
            ),
            ApprovalDecision::Allow
        );
    }

    #[test]
    fn confirm_list_forces_confirmation_even_for_reads() {
        assert_eq!(
            decide(
                &policy(),
                "ask",
                &annotations(false, true),
                Some(&Touches::Reads(Scope::Anything))
            ),
            ApprovalDecision::Confirm
        );
    }

    #[test]
    fn footprint_decides_before_destructive_annotation() {
        let destructive = annotations(true, false);

        assert_eq!(
            decide(
                &policy(),
                "file_system",
                &destructive,
                Some(&Touches::Reads(Scope::Paths(vec![PathBuf::from("/a")])))
            ),
            ApprovalDecision::Allow,
            "只读调用不应因为工具整体可写而弹窗"
        );
        assert_eq!(
            decide(
                &policy(),
                "file_system",
                &destructive,
                Some(&Touches::Writes(Scope::Paths(vec![PathBuf::from("/a")])))
            ),
            ApprovalDecision::Confirm
        );
    }

    #[test]
    fn destructive_annotation_is_the_fallback_without_footprint() {
        assert_eq!(
            decide(&policy(), "unknown", &annotations(true, false), None),
            ApprovalDecision::Confirm
        );
        assert_eq!(
            decide(&policy(), "unknown", &annotations(false, true), None),
            ApprovalDecision::Allow
        );
    }
}
