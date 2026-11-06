pub mod classifier;
pub mod index;
pub mod probe;
pub mod router;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::db::models::Message;

/// 路径选择：由哪套 agent 系统处理这条消息。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSelection {
    /// 走 `agent/fast_loop.rs`（快，适合单轮或轻量任务）。
    Fast,
    /// 走 `OrchestratorAgent`（深度推理，可做任务分解）。
    Deep,
}

/// 单个意图行为的可序列化配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentConfig {
    #[serde(default)]
    pub system_prompt_appendix: Option<String>,
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// 为 true 时该意图会触发自主会话模式。
    #[serde(default)]
    pub auto_escalate: bool,
}

/// 完整路由决策的结果（面向调用方投影出的意图行）。
#[derive(Debug, Clone)]
pub struct IntentResult {
    /// 稳定意图 id（`builtin.software_dev.<slug>` 或 uuid）。
    pub id: String,
    /// 意图行上的人类可读名称。
    pub name: String,
    /// 拥有该意图的领域。
    pub domain_id: String,
    /// [`Self::domain_id`] 的显示名。
    ///
    /// 合成 fallback 时为空：没有查询任何领域行，因此调用方可以区分「没有选中
    /// 领域」与真正命中 `builtin.software_dev`（后者名称非空）。
    pub domain_name: String,
    pub config: IntentConfig,
    pub path: PathSelection,
    /// 最终决策置信度。纯闲聊 fallback 与「什么都没匹配上」时为 `0.0`。
    pub confidence: f32,
}

/// 一次路由请求：消息、最近上下文、会话先验，以及只读探针可检查的项目根。
#[derive(Debug, Clone, Copy)]
pub struct RouteContext<'a> {
    pub session_id: &'a str,
    pub content: &'a str,
    /// 最近消息，不含当前这条。
    pub recent_messages: &'a [Message],
    /// 会话上一次的领域，`None` 表示没有先验。
    ///
    /// 在 `auto` 模式下它只是排序提示：给该领域的候选加 `prior_boost`，既不
    /// 限制候选集，也不参与阈值判定。
    pub prior_domain: Option<&'a str>,
    /// 为 true 时表示硬过滤：候选只允许来自 `prior_domain` 领域。
    ///
    /// 与 [`Self::prior_domain`] 的区别在于它**不是**提示而是约束；领域已不存在
    /// 时按 `auto` 处理。
    pub pinned: bool,
    /// 可选只读探针使用的项目根。
    pub project_root: Option<&'a Path>,
}

/// 最终决策由哪个阶段产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    /// embedding 召回直接命名了意图。
    Embedding,
    /// LLM rerank 命名了意图。
    Llm,
    /// 数据驱动的 example/bigram 打分器命名了意图。
    Rule,
    /// 叠加探针证据后的二次 rerank 命名了意图。
    Probe,
    /// 合成的纯闲聊结果。
    Fallback,
}

/// 路由过程中被考虑过的一个候选。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentCandidate {
    pub intent_id: String,
    pub name: String,
    pub domain_id: String,
    pub score: f32,
    pub signal: DecisionSource,
}

/// 路由过程中收集的证据，用于可观测性与测试。
///
/// 这些字段只用于诊断与回放，不参与决策计算。
#[derive(Debug, Clone, Default)]
pub struct DecisionEvidence {
    /// embedding 召回的 top1 原始 cosine 分数。
    pub embedding_top1: Option<f32>,
    /// top1 与 top2 的原始分数差。
    pub embedding_margin: Option<f32>,
    /// 最后一次 LLM rerank 返回的置信度（含探针后的二次 rerank）。
    pub llm_confidence: Option<f32>,
    /// 规则打分器的得分。
    pub rule_score: Option<f32>,
    /// 本次路由实际发起的 LLM 调用次数（0、1 或 2）。
    pub llm_calls: usize,
    /// 是否真正运行过探针（预算不足或失败时为 false）。
    pub probe_used: bool,
    /// 从进入路由到产出决策的总耗时。
    pub elapsed_ms: u64,
}

/// 一次已完成的路由决策，包含「为什么这么选」。
#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub chosen: IntentResult,
    pub confidence: f32,
    /// 选中意图未达到自身 `confidence_threshold`，或 embedding 未达到
    /// floor/margin 时为 true。执行仍按 `chosen` 继续，不放宽工具白名单。
    pub uncertain: bool,
    pub source: DecisionSource,
    pub candidates: Vec<IntentCandidate>,
    pub evidence: DecisionEvidence,
}

/// 由选中意图推导出的执行配方。
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub path: PathSelection,
    pub allowed_tools: Option<Vec<String>>,
    pub model_id: String,
    pub system_prompt_appendix: Option<String>,
    pub max_iterations: Option<usize>,
}

/// 启动时从 `IntentAppConfig` 快照下来的路由配置。
///
/// 括号内为 [`Default`] 的取值，也是旧配置缺字段时的兜底值。
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// 总开关（默认 `true`）。关闭时永远返回合成闲聊。
    pub enabled: bool,
    /// embedding top1 原始分数下限（默认 `0.55`）：低于它不直接命中。
    pub embedding_floor: f32,
    /// top1 与 top2 的最小分数差（默认 `0.05`）：差值不足说明边界模糊。
    pub embedding_margin: f32,
    /// 会话上次领域候选的排序加成（默认 `0.04`）。只影响排序，不参与阈值判定。
    pub prior_boost: f32,
    /// 是否启用 LLM rerank（默认 `true`）。
    pub llm_rerank_enabled: bool,
    /// 单次 LLM rerank 的超时毫秒数（默认 `8000`），仍受总预算约束。
    ///
    /// 必须覆盖分类模型的真实往返：实测默认模型峰值 3.7s，2.5s 会让信号
    /// 在超时后丢失并退化到规则打分。
    pub llm_timeout_ms: u64,
    /// 是否启用只读探针（默认 `true`）。
    pub probe_enabled: bool,
    /// 探针自身的超时毫秒数（默认 `1500`）。
    pub probe_timeout_ms: u64,
    /// 二次 rerank 所需的最小剩余预算毫秒数（默认 `1200`）：不够就跳过探针。
    pub second_rerank_min_budget_ms: u64,
    /// 整个路由的硬性墙钟预算毫秒数（默认 `12000`），涵盖 embedding、rerank、
    /// 探针与二次 rerank。
    pub routing_budget_ms: u64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            embedding_floor: 0.55,
            embedding_margin: 0.05,
            prior_boost: 0.04,
            llm_rerank_enabled: true,
            llm_timeout_ms: 8000,
            probe_enabled: true,
            probe_timeout_ms: 1500,
            second_rerank_min_budget_ms: 1200,
            routing_budget_ms: 12000,
        }
    }
}

/// 留在进程内环形缓冲里的一条路由决策。
#[derive(Debug, Clone, Serialize)]
pub struct RouteTrace {
    pub at: String,
    pub session_id: String,
    pub message_excerpt: String,
    pub candidates: Vec<IntentCandidate>,
    pub chosen_intent_id: String,
    pub chosen_intent_name: String,
    pub confidence: f32,
    pub uncertain: bool,
    pub source: DecisionSource,
    pub elapsed_ms: u64,
    pub llm_calls: usize,
    pub probe_used: bool,
}
