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
    /// 意图说明；后续消息注入会话提示词时使用。
    pub description: String,
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

/// 一次路由请求：消息、最近上下文、会话绑定的领域，以及只读探针可检查的项目根。
#[derive(Debug, Clone, Copy)]
pub struct RouteContext<'a> {
    pub session_id: &'a str,
    pub content: &'a str,
    /// 最近消息，不含当前这条。
    pub recent_messages: &'a [Message],
    /// 会话绑定的领域；`None` 表示没有绑定，消息走普通聊天兜底。
    pub prior_domain: Option<&'a str>,
    /// 历史字段：领域已是硬绑定，本字段不再参与决策。
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
    /// 分解器不可用：超时、调用失败或没有可用候选。与 [`Self::Fallback`] 的
    /// 「合法零意图」不同，调用方不得把这种结果缓存成零意图会话。
    Unavailable,
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

/// 一次拆解出的单个意图。
#[derive(Debug, Clone)]
pub struct DecomposedIntent {
    /// 意图的投影（含执行配方与 path）。
    pub intent: IntentResult,
    /// 模型为该意图生成的具体指令。
    pub instruction: String,
    /// 该意图的置信度。
    pub confidence: f32,
    /// 低于该意图 `confidence_threshold` 时为 true；执行配方不放宽。
    pub uncertain: bool,
    /// 只读探针取到的项目证据（可选）。
    pub probe_evidence: Option<String>,
}

impl DecomposedIntent {
    /// 用投影、指令与置信度构造一项；`uncertain` 由调用方按阈值判定。
    pub fn new(
        intent: IntentResult,
        instruction: impl Into<String>,
        confidence: f32,
        uncertain: bool,
    ) -> Self {
        Self {
            intent,
            instruction: instruction.into(),
            confidence,
            uncertain,
            probe_evidence: None,
        }
    }
}

/// 一次拆解的结果。
///
/// 空 `intents` 表示零意图：调用方应走普通聊天兜底，不执行任何领域意图。
#[derive(Debug, Clone)]
pub struct Decomposition {
    /// 有序的意图项。空 = 零意图。
    pub intents: Vec<DecomposedIntent>,
    /// 整体置信度（取各意图的最小值；零意图为 0.0）。
    pub confidence: f32,
    /// 是否存在不确定项。
    pub uncertain: bool,
    /// 决策来源（供轨迹与复盘）。
    pub source: DecisionSource,
    /// 候选意图与分数。
    pub candidates: Vec<IntentCandidate>,
    /// 证据（LLM 调用次数、探针是否使用、耗时）。
    pub evidence: DecisionEvidence,
}

impl Decomposition {
    /// 零意图结果：空列表，置信度 0，保留证据与候选供观测。
    pub fn zero_intent(
        source: DecisionSource,
        candidates: Vec<IntentCandidate>,
        evidence: DecisionEvidence,
    ) -> Self {
        Self {
            intents: Vec::new(),
            confidence: 0.0,
            uncertain: false,
            source,
            candidates,
            evidence,
        }
    }

    /// 用意图列表构造结果：整体置信度取最小值，存在任一不确定项即标 uncertain。
    pub fn from_intents(
        intents: Vec<DecomposedIntent>,
        source: DecisionSource,
        candidates: Vec<IntentCandidate>,
        evidence: DecisionEvidence,
    ) -> Self {
        let confidence = intents
            .iter()
            .map(|item| item.confidence)
            .fold(f32::INFINITY, f32::min);
        let confidence = if intents.is_empty() { 0.0 } else { confidence };
        let uncertain = intents.iter().any(|item| item.uncertain);
        Self {
            intents,
            confidence,
            uncertain,
            source,
            candidates,
            evidence,
        }
    }

    /// 是否为零意图（应走普通聊天兜底）。
    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }
}


/// 一个意图的执行配方：拆解项 + 它投影出的意图行。
#[derive(Debug, Clone)]
pub struct IntentPolicy {
    pub intent: IntentResult,
    /// 模型为该意图生成的具体指令（探针证据已前置）。
    pub instruction: String,
    pub confidence: f32,
    /// 置信度低于该意图阈值；只影响展示，不放宽任何配方。
    pub uncertain: bool,
}

impl From<&DecomposedIntent> for IntentPolicy {
    fn from(item: &DecomposedIntent) -> Self {
        Self {
            intent: item.intent.clone(),
            instruction: item.instruction.clone(),
            confidence: item.confidence,
            uncertain: item.uncertain,
        }
    }
}

/// 一次回合的意图执行策略，可承载零个或多个意图。
///
/// 空列表表示零意图：普通聊天兜底，不应用任何意图配方。
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub intents: Vec<IntentPolicy>,
    /// 聚合路径：存在任一 `deep` 意图时为 [`PathSelection::Deep`]。
    pub path: PathSelection,
}

impl ExecutionPolicy {
    /// 由拆解结果投影出策略。
    ///
    /// `uncertain` 刻意不影响配方：低置信项仍按意图库给出的白名单与模型执行。
    pub fn from_decomposition(decomposition: &Decomposition) -> Self {
        let intents: Vec<IntentPolicy> = decomposition
            .intents
            .iter()
            .map(IntentPolicy::from)
            .collect();
        let path = if intents
            .iter()
            .any(|policy| policy.intent.path == PathSelection::Deep)
        {
            PathSelection::Deep
        } else {
            PathSelection::Fast
        };
        Self { intents, path }
    }

    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }

    pub fn first(&self) -> Option<&IntentPolicy> {
        self.intents.first()
    }
}

/// 启动时从 `IntentAppConfig` 快照下来的路由配置。
///
/// 括号内为 [`Default`] 的取值，也是旧配置缺字段时的兜底值。
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// 总开关（默认 `true`）。关闭时永远返回零意图（普通聊天）。
    pub enabled: bool,
    /// 是否启用意图拆解（沿用旧字段名，默认 `true`）。
    pub llm_rerank_enabled: bool,
    /// 单次拆解调用的超时毫秒数（默认 `20000`），仍受总预算约束。
    pub llm_timeout_ms: u64,
    /// 是否启用只读探针（默认 `true`）。
    pub probe_enabled: bool,
    /// 探针自身的超时毫秒数（默认 `1500`）。
    pub probe_timeout_ms: u64,
    /// 整个拆解过程的硬性墙钟预算毫秒数（默认 `25000`）。
    pub routing_budget_ms: u64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            llm_rerank_enabled: true,
            llm_timeout_ms: 20000,
            probe_enabled: true,
            probe_timeout_ms: 1500,
            routing_budget_ms: 25000,
        }
    }
}

/// 轨迹里记录的单个意图项。
#[derive(Debug, Clone, Serialize)]
pub struct TraceIntent {
    pub intent_id: String,
    pub intent_name: String,
    pub instruction: String,
    pub confidence: f32,
    pub uncertain: bool,
}

/// 留在进程内环形缓冲里的一条拆解决策。
#[derive(Debug, Clone, Serialize)]
pub struct RouteTrace {
    pub at: String,
    pub session_id: String,
    pub message_excerpt: String,
    /// 本次拆解得到的全部意图；空 = 零意图。
    pub intents: Vec<TraceIntent>,
    pub candidates: Vec<IntentCandidate>,
    pub confidence: f32,
    pub uncertain: bool,
    pub source: DecisionSource,
    pub elapsed_ms: u64,
    pub llm_calls: usize,
    pub probe_used: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent_result(id: &str) -> IntentResult {
        IntentResult {
            id: id.to_string(),
            name: id.to_string(),
            description: format!("{id} 的说明"),
            domain_id: "domain".to_string(),
            domain_name: "领域".to_string(),
            config: IntentConfig {
                system_prompt_appendix: None,
                enabled_tools: None,
                model_id: None,
                max_iterations: None,
                auto_escalate: false,
            },
            path: PathSelection::Fast,
            confidence: 0.0,
        }
    }

    #[test]
    fn zero_decomposition_has_no_intents() {
        let decomposition =
            Decomposition::zero_intent(DecisionSource::Fallback, Vec::new(), DecisionEvidence::default());

        assert!(decomposition.is_empty());
        assert_eq!(decomposition.confidence, 0.0);
        assert!(!decomposition.uncertain);
    }

    #[test]
    fn from_intents_takes_min_confidence_and_flags_uncertain() {
        let intents = vec![
            DecomposedIntent::new(intent_result("a"), "do a", 0.9, false),
            DecomposedIntent::new(intent_result("b"), "do b", 0.4, true),
        ];

        let decomposition = Decomposition::from_intents(
            intents,
            DecisionSource::Llm,
            Vec::new(),
            DecisionEvidence::default(),
        );

        assert_eq!(decomposition.intents.len(), 2);
        assert_eq!(decomposition.confidence, 0.4);
        assert!(decomposition.uncertain);
    }

    #[test]
    fn execution_policy_carries_every_intent() {
        let mut deep = intent_result("a");
        deep.path = PathSelection::Deep;
        let intents = vec![
            DecomposedIntent::new(deep, "do a", 0.9, false),
            DecomposedIntent::new(intent_result("b"), "do b", 0.5, true),
        ];
        let decomposition = Decomposition::from_intents(
            intents,
            DecisionSource::Llm,
            Vec::new(),
            DecisionEvidence::default(),
        );

        let policy = ExecutionPolicy::from_decomposition(&decomposition);

        assert_eq!(policy.intents.len(), 2);
        assert_eq!(policy.intents[0].instruction, "do a");
        assert_eq!(policy.intents[1].intent.id, "b");
        assert_eq!(policy.path, PathSelection::Deep);
    }
}
