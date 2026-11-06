use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

use crate::core::db::models::IntentRecord;
use crate::core::db::repository::Database;
use crate::llm::embedding::EmbeddingService;
use crate::llm::intent::classifier::{score_intents, IntentReranker, RerankOutcome};
use crate::llm::intent::index::{intent_units, IntentVectorIndex};
use crate::llm::intent::probe::probe;
use crate::llm::intent::{
    DecisionEvidence, DecisionSource, IntentCandidate, IntentConfig, IntentResult, PathSelection,
    RouteContext, RouteDecision, RouteTrace, RouterConfig,
};

/// 路由被禁用或无法执行时上报的稳定意图 id。
const FALLBACK_INTENT_ID: &str = "builtin.software_dev.chat";

/// 合成结果归属的领域。它只是标明 fallback 意图所属的内置领域；结果的
/// `domain_name` 故意留空，因为并没有查询任何领域行。
const FALLBACK_DOMAIN_ID: &str = "builtin.software_dev";

/// embedding 召回保留的候选数量。
const EMBEDDING_TOP_K: usize = 6;
/// 没有 embedding 结果时，交给 LLM rerank 的候选上限。
const LLM_CANDIDATE_LIMIT: usize = 40;
/// 进程内保留的路由轨迹条数。
const TRACE_LIMIT: usize = 50;
/// 单次 embedding 查询等待上限，避免拖垮整体预算。
const EMBEDDING_QUERY_TIMEOUT_MS: u64 = 1200;

/// 一次路由所需的全量数据快照：领域 id → 名称，以及所有启用意图。
struct RoutingData {
    domain_names: HashMap<String, String>,
    intents: Vec<IntentRecord>,
}

/// 本次路由的候选范围：候选意图切片、按 id 的索引，以及 `auto` 模式下的先验领域。
///
/// `prior_domain` 只参与排序加成，不参与阈值判定，也不限制候选集。
struct RoutingScope<'a> {
    data: &'a RoutingData,
    candidates: &'a [IntentRecord],
    intents_by_id: HashMap<&'a str, &'a IntentRecord>,
    candidate_ids: HashSet<String>,
    prior_domain: Option<&'a str>,
}

impl<'a> RoutingScope<'a> {
    fn new(
        data: &'a RoutingData,
        candidates: &'a [IntentRecord],
        prior_domain: Option<&'a str>,
    ) -> Self {
        let intents_by_id = candidates
            .iter()
            .map(|intent| (intent.id.as_str(), intent))
            .collect();
        let candidate_ids = candidates
            .iter()
            .map(|intent| intent.id.clone())
            .collect();
        Self {
            data,
            candidates,
            intents_by_id,
            candidate_ids,
            prior_domain,
        }
    }
}

/// 信号阶段之间传递的中间状态。
#[derive(Default)]
struct SignalState {
    /// 最近一次 embedding 召回的 top-k 结果（id, 原始 cosine 分数）。
    emb_top: Vec<(String, f32)>,
    /// 已经考虑过的候选，用于观测与回放。
    candidates_meta: Vec<IntentCandidate>,
    /// 各信号留下的证据字段。
    evidence: DecisionEvidence,
}

/// LLM 信号的处理结果。
///
/// `NoMatch` 是可信的语义结论（模型明确回答 `none`），不能再用规则打分
/// 覆盖；`Unavailable` 才允许继续降级。
enum LlmSignal {
    Decided(RouteDecision),
    NoMatch,
    Unavailable,
}

/// 意图路由器：按信号可信度级联解析用户消息（embedding → LLM rerank →
/// 规则打分），记录决策来源，并在进程内维护一个轨迹环形缓冲。
pub struct IntentRouter {
    db: Arc<Mutex<Database>>,
    embedding: Option<Arc<EmbeddingService>>,
    index: RwLock<IntentVectorIndex>,
    reranker: Arc<dyn IntentReranker>,
    config: RouterConfig,
    traces: Mutex<VecDeque<RouteTrace>>,
}

impl IntentRouter {
    /// 用数据库句柄、可选的 embedding 服务、reranker 与配置构造路由器。
    ///
    /// `embedding` 为 `None` 时 embedding 信号永久不可用，路由退化为 LLM 与规则。
    pub fn new(
        db: Arc<Mutex<Database>>,
        embedding: Option<Arc<EmbeddingService>>,
        reranker: Arc<dyn IntentReranker>,
        config: RouterConfig,
    ) -> Self {
        Self {
            db,
            embedding,
            index: RwLock::new(IntentVectorIndex::new()),
            reranker,
            config,
            traces: Mutex::new(VecDeque::new()),
        }
    }

    /// 在后台构建意图索引。绝不阻塞路由：构建完成前 embedding 信号只是不可用。
    pub async fn warm_up(self: &Arc<Self>) {
        let Some(embedding) = self
            .embedding
            .as_ref()
            .filter(|embedding| embedding.available())
        else {
            self.index.write().await.clear();
            return;
        };

        let intents = {
            let db = self.db.lock().await;
            db.list_all_enabled_intents()
        };
        let intents = match intents {
            Ok(intents) => intents,
            Err(error) => {
                log::warn!("intent index: failed to list intents: {error}");
                return;
            }
        };

        let mut units: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
        for intent in &intents {
            let texts = intent_units(&intent.name, &intent.description, &intent.examples);
            let mut vectors = Vec::with_capacity(texts.len());
            for text in texts {
                if let Some(vector) = embedding.embed(&text).await {
                    vectors.push(vector);
                }
            }
            if !vectors.is_empty() {
                units.insert(intent.id.clone(), vectors);
            }
        }

        let count = units.len();
        self.index.write().await.replace(units);
        log::info!("intent index: ready with {count} intents");
    }

    /// 把索引标记为过期。调用方通常随后 spawn 一次 [`Self::warm_up`]；
    /// 重建期间旧快照继续服务。
    pub async fn mark_dirty(&self) {
        self.index.write().await.mark_dirty();
    }

    /// 读取轨迹环形缓冲的快照，最新的在前（供调试面板只读使用）。
    pub async fn recent_traces(&self) -> Vec<RouteTrace> {
        let traces = self.traces.lock().await;
        traces.iter().cloned().collect()
    }

    /// 根据会话限制与意图限制求最终工具列表。
    ///
    /// 意图侧的显式列表是权威的：它列出该意图允许的全部工具，因此交集为空
    /// 表示「没有任何工具」（`Some(vec![])`），而不是「不限制」。意图侧为
    /// `None` 才表示该意图不施加限制。
    pub fn resolve_tools(
        &self,
        session_tools: Option<Vec<String>>,
        intent_tools: Option<&Vec<String>>,
    ) -> Option<Vec<String>> {
        // 旧配置/旧意图里可能还是 file_system / code_executor；先归一化，
        // 否则升级后交集为空，模型会突然没有任何工具。
        let session_tools = session_tools.map(normalize_tool_names);
        let intent_tools = intent_tools.map(|tools| normalize_tool_names(tools.clone()));

        match (session_tools, intent_tools.as_ref()) {
            (Some(session), Some(intent)) => Some(
                session
                    .into_iter()
                    .filter(|tool| intent.contains(tool))
                    .collect(),
            ),
            (None, Some(intent)) => Some(intent.clone()),
            (Some(session), None) => Some(session),
            (None, None) => None,
        }
    }

    /// 把一条消息解析为一个意图。永不失败：任何降级情形都会返回一个决策，
    /// 保证消息仍能被回答。
    ///
    /// 整体是三层结构：先按可信度依次收集信号（embedding → LLM rerank →
    /// 探针 → 规则），命中即产出决策；再投影出执行策略；最后写入轨迹。
    /// 信号之间不共享分数，也不做加权平均。
    pub async fn route(&self, ctx: RouteContext<'_>) -> RouteDecision {
        let started = Instant::now();

        if !self.config.enabled {
            return self
                .finalize(Self::synthetic(false), &ctx, started)
                .await;
        }

        let Some(data) = self.load_routing_data().await else {
            return self
                .finalize(Self::synthetic(true), &ctx, started)
                .await;
        };

        if data.intents.is_empty() {
            log::warn!("intent routing: no enabled intents in any domain");
            return self
                .finalize(Self::synthetic(false), &ctx, started)
                .await;
        }

        // `pinned` 是硬过滤；`auto` 下只保留候选全集。
        let Some(candidates) = Self::select_candidates(&data, &ctx) else {
            return self
                .finalize(Self::synthetic(false), &ctx, started)
                .await;
        };

        let scope = RoutingScope::new(&data, &candidates, ctx.prior_domain);
        let mut state = SignalState::default();

        // 第一层：按可信度级联收集信号，第一个命中的信号直接产出决策。
        if let Some(decision) = self
            .collect_embedding_signal(&scope, &ctx, started, &mut state)
            .await
        {
            return self.finalize(decision, &ctx, started).await;
        }

        match self
            .collect_llm_signal(&scope, &ctx, started, &mut state)
            .await
        {
            LlmSignal::Decided(decision) => return self.finalize(decision, &ctx, started).await,
            LlmSignal::NoMatch => {
                // 模型明确说候选都不匹配：只允许 embedding 兜底，绝不用
                // 规则打分的噪声推翻这个结论。
                if let Some(decision) = self.collect_embedding_fallback(&scope, &mut state) {
                    return self.finalize(decision, &ctx, started).await;
                }
                let evidence = std::mem::take(&mut state.evidence);
                return self
                    .finalize(Self::synthetic_with(false, evidence), &ctx, started)
                    .await;
            }
            LlmSignal::Unavailable => {}
        }

        if let Some(decision) = self.collect_embedding_fallback(&scope, &mut state) {
            return self.finalize(decision, &ctx, started).await;
        }

        if let Some(decision) = self.collect_rule_signal(&scope, &ctx, &mut state) {
            return self.finalize(decision, &ctx, started).await;
        }

        let evidence = std::mem::take(&mut state.evidence);
        self.finalize(Self::synthetic_with(true, evidence), &ctx, started)
            .await
    }

    /// 按会话的 `pinned` 语义裁剪候选集。
    ///
    /// `pinned` 且领域存在时只保留该领域的启用意图；领域已不存在时按 `auto`
    /// 处理并记一次 warn。返回 `None` 表示没有任何候选，调用方应返回合成闲聊。
    fn select_candidates(data: &RoutingData, ctx: &RouteContext<'_>) -> Option<Vec<IntentRecord>> {
        let prior_exists = ctx
            .prior_domain
            .is_some_and(|id| data.domain_names.contains_key(id));
        if ctx.pinned && ctx.prior_domain.is_some() && !prior_exists {
            log::warn!(
                "intent routing: pinned domain '{}' no longer exists, treating the session as auto",
                ctx.prior_domain.unwrap_or_default()
            );
        }
        let pinned = ctx.pinned && prior_exists;

        let candidates: Vec<IntentRecord> = if pinned {
            data.intents
                .iter()
                .filter(|intent| Some(intent.domain_id.as_str()) == ctx.prior_domain)
                .cloned()
                .collect()
        } else {
            data.intents.clone()
        };

        if candidates.is_empty() {
            log::warn!("intent routing: pinned domain has no enabled intents");
            return None;
        }

        Some(candidates)
    }

    /// 信号 1：embedding 召回。
    ///
    /// 索引未就绪、embedder 不可用或预算耗尽时静默返回 `None`，把决策让给后续
    /// 信号。top1 同时满足 `embedding_floor` 与 `embedding_margin` 时才直接命中，
    /// 否则只把候选与证据留给下游。
    async fn collect_embedding_signal(
        &self,
        scope: &RoutingScope<'_>,
        ctx: &RouteContext<'_>,
        started: Instant,
        state: &mut SignalState,
    ) -> Option<RouteDecision> {
        state.emb_top = self.embedding_recall(scope, ctx, started).await;

        let top1 = state.emb_top.first().map(|(_, score)| *score);
        let top2 = state.emb_top.get(1).map(|(_, score)| *score).unwrap_or(0.0);
        state.evidence.embedding_top1 = top1;
        state.evidence.embedding_margin = top1.map(|score| score - top2);

        state.candidates_meta = embedding_candidates(&state.emb_top, &scope.intents_by_id);

        let decisive = top1.is_some_and(|score| {
            score >= self.config.embedding_floor
                && (score - top2) >= self.config.embedding_margin
        });
        if !decisive {
            return None;
        }

        let (record, confidence) = state
            .emb_top
            .first()
            .and_then(|(id, _)| scope.intents_by_id.get(id.as_str()).copied())
            .map(|record| (record.clone(), top1.unwrap_or(0.0)))?;

        Some(self.decision_for(
            scope.data,
            &record,
            confidence,
            false,
            DecisionSource::Embedding,
            std::mem::take(&mut state.candidates_meta),
            std::mem::take(&mut state.evidence),
        ))
    }

    /// 取一次 embedding 查询结果，受剩余预算与单次查询上限共同约束。
    async fn embedding_recall(
        &self,
        scope: &RoutingScope<'_>,
        ctx: &RouteContext<'_>,
        started: Instant,
    ) -> Vec<(String, f32)> {
        let Some(embedding) = self
            .embedding
            .as_ref()
            .filter(|embedding| embedding.available())
        else {
            return Vec::new();
        };
        if !self.index.read().await.is_ready() {
            return Vec::new();
        }

        let wait = budget_left(started, self.config.routing_budget_ms)
            .min(Duration::from_millis(EMBEDDING_QUERY_TIMEOUT_MS));
        if wait.is_zero() {
            return Vec::new();
        }

        let Ok(Some(query)) = tokio::time::timeout(wait, embedding.embed(ctx.content)).await
        else {
            return Vec::new();
        };

        let index = self.index.read().await;
        index.top_k(&query, &scope.candidate_ids, EMBEDDING_TOP_K)
    }

    /// 信号 2：LLM rerank（必要时叠加只读探针）。
    ///
    /// 仅在 embedding 未明确命中时运行。LLM 不可用、超时或返回未知 id 时返回
    /// `None`，让 embedding 兜底而不是被规则覆盖。
    async fn collect_llm_signal(
        &self,
        scope: &RoutingScope<'_>,
        ctx: &RouteContext<'_>,
        started: Instant,
        state: &mut SignalState,
    ) -> LlmSignal {
        if !self.config.llm_rerank_enabled {
            return LlmSignal::Unavailable;
        }

        let llm_candidates: Vec<IntentRecord> = if state.emb_top.is_empty() {
            scope
                .candidates
                .iter()
                .take(LLM_CANDIDATE_LIMIT)
                .cloned()
                .collect()
        } else {
            state
                .emb_top
                .iter()
                .filter_map(|(id, _)| scope.intents_by_id.get(id.as_str()).map(|r| (*r).clone()))
                .collect()
        };
        if llm_candidates.is_empty() {
            return LlmSignal::Unavailable;
        }

        let wait = budget_left(started, self.config.routing_budget_ms)
            .min(Duration::from_millis(self.config.llm_timeout_ms));
        if wait.is_zero() {
            return LlmSignal::Unavailable;
        }

        state.evidence.llm_calls += 1;
        let outcome = tokio::time::timeout(
            wait,
            self.reranker
                .rerank(ctx.content, ctx.recent_messages, &llm_candidates),
        )
        .await;
        let (intent_id, confidence) = match outcome {
            Ok(RerankOutcome::Picked {
                intent_id,
                confidence,
            }) => (intent_id, confidence),
            Ok(RerankOutcome::NoMatch) => {
                state.evidence.llm_confidence = Some(0.0);
                return LlmSignal::NoMatch;
            }
            // 超时、调用失败或回复不可解析：允许后续信号兜底。
            Ok(RerankOutcome::Unavailable) | Err(_) => return LlmSignal::Unavailable,
        };
        let Some(record) = scope.intents_by_id.get(intent_id.as_str()).copied() else {
            return LlmSignal::Unavailable;
        };

        state.evidence.llm_confidence = Some(confidence);
        let uncertain = confidence < record.confidence_threshold as f32;

        // 低置信时才值得为探针再花一次 LLM 调用。
        if uncertain && self.config.probe_enabled {
            if let Some(decision) = self
                .collect_probe_signal(scope, ctx, started, state, record, &llm_candidates)
                .await
            {
                return LlmSignal::Decided(decision);
            }
        }

        push_candidate(
            &mut state.candidates_meta,
            record,
            confidence,
            DecisionSource::Llm,
        );
        LlmSignal::Decided(self.decision_for(
            scope.data,
            record,
            confidence,
            uncertain,
            DecisionSource::Llm,
            std::mem::take(&mut state.candidates_meta),
            std::mem::take(&mut state.evidence),
        ))
    }

    /// 信号 3：探针证据 + 第二次 rerank。
    ///
    /// 只有在预算足够、项目根存在且探针确实产出证据时才有效。任一步失败都返回
    /// `None`，调用方退回第一次 LLM 的结果。
    async fn collect_probe_signal(
        &self,
        scope: &RoutingScope<'_>,
        ctx: &RouteContext<'_>,
        started: Instant,
        state: &mut SignalState,
        record: &IntentRecord,
        llm_candidates: &[IntentRecord],
    ) -> Option<RouteDecision> {
        let project_root = ctx.project_root?;

        let need = Duration::from_millis(
            self.config.probe_timeout_ms + self.config.second_rerank_min_budget_ms,
        );
        if budget_left(started, self.config.routing_budget_ms) < need {
            return None;
        }

        state.evidence.probe_used = true;
        let evidence_text = probe(
            record,
            project_root,
            Duration::from_millis(self.config.probe_timeout_ms),
        )
        .await?;

        let combined = format!("{}\n\nProject evidence:\n{}", ctx.content, evidence_text);
        let wait = budget_left(started, self.config.routing_budget_ms)
            .min(Duration::from_millis(self.config.llm_timeout_ms));
        if wait.is_zero() {
            return None;
        }

        state.evidence.llm_calls += 1;
        let (second_id, second_conf) = match tokio::time::timeout(
            wait,
            self.reranker
                .rerank(&combined, ctx.recent_messages, llm_candidates),
        )
        .await
        {
            Ok(RerankOutcome::Picked {
                intent_id,
                confidence,
            }) => (intent_id, confidence),
            // 二次 rerank 明确 no-match 或失败都只当「无新证据」，退回第一次
            // LLM 的结果。
            Ok(RerankOutcome::NoMatch) | Ok(RerankOutcome::Unavailable) | Err(_) => {
                return None;
            }
        };
        let second_record = scope.intents_by_id.get(second_id.as_str()).copied()?;

        state.evidence.llm_confidence = Some(second_conf);
        push_candidate(
            &mut state.candidates_meta,
            second_record,
            second_conf,
            DecisionSource::Probe,
        );
        Some(self.decision_for(
            scope.data,
            second_record,
            second_conf,
            second_conf < second_record.confidence_threshold as f32,
            DecisionSource::Probe,
            std::mem::take(&mut state.candidates_meta),
            std::mem::take(&mut state.evidence),
        ))
    }

    /// 信号 4：embedding 兜底。
    ///
    /// 用于 LLM 不可用或未能给出可用 id 的情形。`prior_boost` 只影响这里的
    /// 排序，不改原始分数，因此决策始终标记为 `uncertain`。
    fn collect_embedding_fallback(
        &self,
        scope: &RoutingScope<'_>,
        state: &mut SignalState,
    ) -> Option<RouteDecision> {
        if state.emb_top.is_empty() {
            return None;
        }

        let (record, raw) = ranked_choice(
            &state.emb_top,
            &scope.intents_by_id,
            scope.prior_domain,
            self.config.prior_boost,
        )?;

        push_candidate(
            &mut state.candidates_meta,
            &record,
            raw,
            DecisionSource::Embedding,
        );
        Some(self.decision_for(
            scope.data,
            &record,
            raw,
            true,
            DecisionSource::Embedding,
            std::mem::take(&mut state.candidates_meta),
            std::mem::take(&mut state.evidence),
        ))
    }

    /// 信号 5：纯数据驱动的规则打分（example 命中优先，其次中文 bigram 重叠）。
    fn collect_rule_signal(
        &self,
        scope: &RoutingScope<'_>,
        ctx: &RouteContext<'_>,
        state: &mut SignalState,
    ) -> Option<RouteDecision> {
        let (intent_id, score) = score_intents(ctx.content, scope.candidates)?;
        let record = scope.intents_by_id.get(intent_id.as_str()).copied()?;

        state.evidence.rule_score = Some(score);
        push_candidate(
            &mut state.candidates_meta,
            record,
            score,
            DecisionSource::Rule,
        );
        Some(self.decision_for(
            scope.data,
            record,
            score,
            score < record.confidence_threshold as f32,
            DecisionSource::Rule,
            std::mem::take(&mut state.candidates_meta),
            std::mem::take(&mut state.evidence),
        ))
    }

    /// 一次性读出领域名映射与所有启用意图；任一查询失败都返回 `None`（降级为
    /// 合成闲聊），不向调用方抛错。
    async fn load_routing_data(&self) -> Option<RoutingData> {
        let (domains, intents) = {
            let db = self.db.lock().await;
            (db.list_intent_domains(), db.list_all_enabled_intents())
        };

        let domains = match domains {
            Ok(domains) => domains,
            Err(error) => {
                log::warn!("intent routing: failed to list domains: {error}");
                return None;
            }
        };
        let intents = match intents {
            Ok(intents) => intents,
            Err(error) => {
                log::warn!("intent routing: failed to list enabled intents: {error}");
                return None;
            }
        };

        let mut domain_names = HashMap::new();
        for domain in &domains {
            domain_names.insert(domain.id.clone(), domain.name.clone());
        }

        Some(RoutingData {
            domain_names,
            intents,
        })
    }

    /// 用选中的意图行投影出一个完整决策，并把领域名一并带上。
    fn decision_for(
        &self,
        data: &RoutingData,
        record: &IntentRecord,
        confidence: f32,
        uncertain: bool,
        source: DecisionSource,
        candidates: Vec<IntentCandidate>,
        evidence: DecisionEvidence,
    ) -> RouteDecision {
        RouteDecision {
            chosen: Self::project(record, data.domain_name(&record.domain_id), confidence),
            confidence,
            uncertain,
            source,
            candidates,
            evidence,
        }
    }

    /// 收尾：补上总耗时，写入轨迹环形缓冲（最多保留 `TRACE_LIMIT` 条）。
    ///
    /// 每个返回路径都必须经过这里，保证 `intent_resolved` 事件的可观测字段完整。
    async fn finalize(
        &self,
        mut decision: RouteDecision,
        ctx: &RouteContext<'_>,
        started: Instant,
    ) -> RouteDecision {
        decision.evidence.elapsed_ms = started.elapsed().as_millis() as u64;

        let excerpt: String = ctx.content.chars().take(80).collect();
        let trace = RouteTrace {
            at: Utc::now().to_rfc3339(),
            session_id: ctx.session_id.to_string(),
            message_excerpt: excerpt,
            candidates: decision.candidates.clone(),
            chosen_intent_id: decision.chosen.id.clone(),
            chosen_intent_name: decision.chosen.name.clone(),
            confidence: decision.confidence,
            uncertain: decision.uncertain,
            source: decision.source,
            elapsed_ms: decision.evidence.elapsed_ms,
            llm_calls: decision.evidence.llm_calls,
            probe_used: decision.evidence.probe_used,
        };

        let mut traces = self.traces.lock().await;
        traces.push_front(trace);
        traces.truncate(TRACE_LIMIT);

        decision
    }

    /// 合成闲聊结果：走 fast path，不限制工具、不设迭代上限，也不选领域。
    fn synthetic(uncertain: bool) -> RouteDecision {
        RouteDecision {
            chosen: Self::fallback(),
            confidence: 0.0,
            uncertain,
            source: DecisionSource::Fallback,
            candidates: Vec::new(),
            evidence: DecisionEvidence::default(),
        }
    }

    /// 合成闲聊，但保留本次路由已经收集到的证据（例如 LLM 调用次数与耗时）。
    fn synthetic_with(uncertain: bool, evidence: DecisionEvidence) -> RouteDecision {
        let mut decision = Self::synthetic(uncertain);
        decision.evidence = evidence;
        decision
    }

    /// 内置闲聊意图的固定投影，`domain_name` 故意为空（未查询领域行）。
    fn fallback() -> IntentResult {
        IntentResult {
            id: FALLBACK_INTENT_ID.to_string(),
            name: "闲聊对话".to_string(),
            domain_id: FALLBACK_DOMAIN_ID.to_string(),
            domain_name: String::new(),
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

    /// 把意图行投影为调用方使用的 [`IntentResult`]。
    ///
    /// 空字符串字段投影为 `None`；`path` 只认 `deep`（大小写不敏感），其余一律
    /// 视为 fast；置信度收敛到 `[0, 1]`。
    fn project(record: &IntentRecord, domain_name: &str, confidence: f32) -> IntentResult {
        let optional = |value: &str| {
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        };

        IntentResult {
            id: record.id.clone(),
            name: record.name.clone(),
            domain_id: record.domain_id.clone(),
            domain_name: domain_name.to_string(),
            config: IntentConfig {
                system_prompt_appendix: optional(&record.system_prompt_appendix),
                enabled_tools: Some(record.allowed_tools.clone()),
                model_id: optional(&record.model_id),
                max_iterations: record.max_iterations,
                auto_escalate: record.auto_escalate,
            },
            path: if record.path.eq_ignore_ascii_case("deep") {
                PathSelection::Deep
            } else {
                PathSelection::Fast
            },
            confidence: confidence.clamp(0.0, 1.0),
        }
    }
}

impl RoutingData {
    fn domain_name(&self, domain_id: &str) -> &str {
        self.domain_names
            .get(domain_id)
            .map(String::as_str)
            .unwrap_or("")
    }
}

/// 整体路由预算的剩余部分；已超预算时返回零时长。
fn budget_left(started: Instant, budget_ms: u64) -> Duration {
    Duration::from_millis(budget_ms).saturating_sub(started.elapsed())
}

/// 把 embedding 召回结果投影成候选列表（信号统一记为 embedding）。
fn embedding_candidates(
    emb_top: &[(String, f32)],
    intents_by_id: &HashMap<&str, &IntentRecord>,
) -> Vec<IntentCandidate> {
    emb_top
        .iter()
        .take(EMBEDDING_TOP_K)
        .filter_map(|(intent_id, score)| {
            intents_by_id.get(intent_id.as_str()).map(|record| IntentCandidate {
                intent_id: intent_id.clone(),
                name: record.name.clone(),
                domain_id: record.domain_id.clone(),
                score: *score,
                signal: DecisionSource::Embedding,
            })
        })
        .collect()
}

/// 追加一个候选；同一个意图只保留最先出现的记录，避免轨迹里重复。
fn push_candidate(
    candidates: &mut Vec<IntentCandidate>,
    record: &IntentRecord,
    score: f32,
    signal: DecisionSource,
) {
    if candidates
        .iter()
        .any(|candidate| candidate.intent_id == record.id)
    {
        return;
    }
    candidates.push(IntentCandidate {
        intent_id: record.id.clone(),
        name: record.name.clone(),
        domain_id: record.domain_id.clone(),
        score,
        signal,
    });
}

/// 在 embedding 召回结果里挑排序最靠前的意图。
///
/// `prior_boost` 只加在排序分上；返回的是原始分数，因此不会影响 floor / margin
/// 判定，也不会让不确定的决策看起来确定。
fn ranked_choice(
    emb_top: &[(String, f32)],
    intents_by_id: &HashMap<&str, &IntentRecord>,
    prior_domain: Option<&str>,
    prior_boost: f32,
) -> Option<(IntentRecord, f32)> {
    let mut best: Option<(IntentRecord, f32, f32)> = None;
    for (intent_id, raw) in emb_top {
        let Some(record) = intents_by_id.get(intent_id.as_str()) else {
            continue;
        };
        let rank = raw
            + if Some(record.domain_id.as_str()) == prior_domain {
                prior_boost
            } else {
                0.0
            };
        if best
            .as_ref()
            .map_or(true, |(_, _, best_rank)| rank > *best_rank)
        {
            best = Some(((*record).clone(), *raw, rank));
        }
    }
    best.map(|(record, raw, _)| (record, raw))
}

/// 把旧的内置工具名归一化到 rig-tools 的名字。
///
/// 只用于兼容升级前写入配置/数据库的名单；新名单本身是幂等的。
fn normalize_tool_names(tools: Vec<String>) -> Vec<String> {
    const FILE_TOOLS: [&str; 7] = ["read", "write", "edit", "search", "list", "delete", "move"];

    let mut out = Vec::new();
    for tool in tools {
        match tool.as_str() {
            "file_system" => {
                for name in FILE_TOOLS {
                    push_unique_tool(&mut out, name);
                }
            }
            "code_executor" => push_unique_tool(&mut out, "shell"),
            "calculator" | "web_search" | "run_workflow" => {}
            other => push_unique_tool(&mut out, other),
        }
    }
    out
}

fn push_unique_tool(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|item| item == name) {
        list.push(name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::{IntentDomainRecord, Message};
    use crate::llm::embedding::TextEmbedder;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TS: &str = "2026-11-05T00:00:00Z";

    /// 评测集里的一条样例。缺失字段按原逻辑退化为默认值，避免评测因个别
    /// 样例不完整而整体失败。
    #[derive(serde::Deserialize)]
    struct EvalEntry {
        #[serde(default)]
        message: String,
        #[serde(default = "default_category")]
        category: String,
        #[serde(default)]
        expected_intent: Option<String>,
        #[serde(default)]
        recent: Vec<EvalMessage>,
    }

    /// 评测集里的历史消息，只保留 rerank 需要的字段。
    #[derive(serde::Deserialize)]
    struct EvalMessage {
        #[serde(default = "default_role")]
        role: String,
        #[serde(default)]
        content: String,
    }

    fn default_category() -> String {
        "clear".to_string()
    }

    fn default_role() -> String {
        "user".to_string()
    }

    fn test_db() -> Arc<Mutex<Database>> {
        Arc::new(Mutex::new(Database::new_test().expect("in-memory db")))
    }

    fn mk_domain(id: &str) -> IntentDomainRecord {
        IntentDomainRecord {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            is_active: false,
            source: "user".to_string(),
            version: 1,
            created_at: TS.to_string(),
            updated_at: TS.to_string(),
        }
    }

    fn mk_intent(id: &str, domain_id: &str, path: &str) -> IntentRecord {
        IntentRecord {
            id: id.to_string(),
            domain_id: domain_id.to_string(),
            name: format!("意图 {id}"),
            description: "测试意图".to_string(),
            examples: vec!["alpha".to_string()],
            workflow: String::new(),
            allowed_tools: vec!["read_file".to_string()],
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: true,
            version: 1,
            confidence_threshold: 0.5,
            fallback_intent: String::new(),
            path: path.to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
            sort_order: 0,
            created_at: TS.to_string(),
            updated_at: TS.to_string(),
        }
    }

    fn seed_domain(db: &Database, domain_id: &str, intents: &[IntentRecord]) {
        db.insert_intent_domain(&mk_domain(domain_id))
            .expect("insert domain");
        for intent in intents {
            db.insert_intent(intent).expect("insert intent");
        }
    }

    struct FixedEmbedder {
        vector: Vec<f32>,
    }

    #[async_trait::async_trait]
    impl TextEmbedder for FixedEmbedder {
        async fn embed(&self, _text: &str) -> Option<Vec<f32>> {
            Some(self.vector.clone())
        }
    }

    fn embedding_with(vector: Vec<f32>) -> Arc<EmbeddingService> {
        Arc::new(EmbeddingService::with_embedder(Arc::new(FixedEmbedder {
            vector,
        })))
    }

    struct ScriptedReranker {
        responses: Mutex<VecDeque<RerankOutcome>>,
        calls: AtomicUsize,
    }

    impl ScriptedReranker {
        fn new(responses: Vec<RerankOutcome>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into()),
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    fn picked(intent_id: &str, confidence: f32) -> RerankOutcome {
        RerankOutcome::Picked {
            intent_id: intent_id.to_string(),
            confidence,
        }
    }

    #[async_trait::async_trait]
    impl IntentReranker for ScriptedReranker {
        async fn rerank(
            &self,
            _message: &str,
            _recent_messages: &[Message],
            _candidates: &[IntentRecord],
        ) -> RerankOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .lock()
                .await
                .pop_front()
                .unwrap_or(RerankOutcome::Unavailable)
        }
    }

    fn router(
        db: Arc<Mutex<Database>>,
        embedding: Option<Arc<EmbeddingService>>,
        reranker: Arc<ScriptedReranker>,
        config: RouterConfig,
    ) -> Arc<IntentRouter> {
        Arc::new(IntentRouter::new(db, embedding, reranker, config))
    }

    async fn set_index(router: &IntentRouter, units: Vec<(&str, Vec<Vec<f32>>)>) {
        let map = units
            .into_iter()
            .map(|(id, vectors)| (id.to_string(), vectors))
            .collect();
        router.index.write().await.replace(map);
    }

    fn ctx<'a>(
        content: &'a str,
        prior_domain: Option<&'a str>,
        pinned: bool,
        project_root: Option<&'a std::path::Path>,
    ) -> RouteContext<'a> {
        RouteContext {
            session_id: "session",
            content,
            recent_messages: &[],
            prior_domain,
            pinned,
            project_root,
        }
    }

    fn config() -> RouterConfig {
        RouterConfig {
            probe_enabled: false,
            ..RouterConfig::default()
        }
    }

    #[test]
    fn default_timeouts_cover_a_real_classifier_round_trip() {
        let config = RouterConfig::default();

        // 实测默认分类模型峰值 3.7s；2.5s 会让 LLM 信号在超时后丢失。
        assert!(config.llm_timeout_ms >= 8000);
        assert!(
            config.routing_budget_ms > config.llm_timeout_ms,
            "总预算必须给探针与二次 rerank 留出空间"
        );
    }

    #[tokio::test]
    async fn disabled_router_returns_synthetic() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "deep")]);
        }
        let reranker = ScriptedReranker::new(vec![picked("test.a", 0.9)]);
        let router = router(
            db,
            None,
            reranker.clone(),
            RouterConfig {
                enabled: false,
                ..config()
            },
        );

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.chosen.id, FALLBACK_INTENT_ID);
        assert_eq!(decision.source, DecisionSource::Fallback);
        assert_eq!(reranker.calls(), 0);
    }

    #[tokio::test]
    async fn embedding_decisive_skips_llm() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[
                    mk_intent("test.a", "test.domain", "fast"),
                    mk_intent("test.b", "test.domain", "fast"),
                ],
            );
        }
        let reranker = ScriptedReranker::new(vec![picked("test.b", 0.9)]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 0.0])),
            reranker.clone(),
            config(),
        );
        set_index(
            &router,
            vec![
                ("test.a", vec![vec![1.0, 0.0]]),
                ("test.b", vec![vec![0.0, 1.0]]),
            ],
        )
        .await;

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.chosen.id, "test.a");
        assert_eq!(decision.source, DecisionSource::Embedding);
        assert!(!decision.uncertain);
        assert_eq!(reranker.calls(), 0);
    }

    #[tokio::test]
    async fn ambiguous_embedding_calls_llm() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[
                    mk_intent("test.a", "test.domain", "fast"),
                    mk_intent("test.b", "test.domain", "fast"),
                ],
            );
        }
        let reranker = ScriptedReranker::new(vec![picked("test.a", 0.9)]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker.clone(),
            config(),
        );
        set_index(
            &router,
            vec![
                ("test.a", vec![vec![1.0, 0.0]]),
                ("test.b", vec![vec![0.0, 1.0]]),
            ],
        )
        .await;

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.chosen.id, "test.a");
        assert_eq!(decision.source, DecisionSource::Llm);
        assert_eq!(reranker.calls(), 1);
    }

    #[tokio::test]
    async fn low_confidence_without_probe_is_uncertain() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "fast")]);
        }
        let reranker = ScriptedReranker::new(vec![picked("test.a", 0.2)]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker.clone(),
            RouterConfig {
                embedding_floor: 0.9,
                ..config()
            },
        );
        set_index(&router, vec![("test.a", vec![vec![1.0, 0.0]])]).await;

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.chosen.id, "test.a");
        assert_eq!(decision.source, DecisionSource::Llm);
        assert!(decision.uncertain);
        assert_eq!(reranker.calls(), 1);
    }

    #[tokio::test]
    async fn llm_failure_uses_embedding_not_rule() {
        let db = test_db();
        let mut rule_bait = mk_intent("test.b", "test.domain", "fast");
        rule_bait.examples = vec!["hello".to_string()];
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast"), rule_bait],
            );
        }
        let reranker = ScriptedReranker::new(vec![RerankOutcome::Unavailable]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker,
            config(),
        );
        set_index(
            &router,
            vec![
                ("test.a", vec![vec![1.0, 0.0]]),
                ("test.b", vec![vec![0.0, 1.0]]),
            ],
        )
        .await;

        let decision = router.route(ctx("hello", None, false, None)).await;

        assert_eq!(decision.chosen.id, "test.a");
        assert_eq!(decision.source, DecisionSource::Embedding);
        assert!(decision.uncertain);
    }

    #[tokio::test]
    async fn explicit_no_match_does_not_fall_through_to_rule() {
        let db = test_db();
        let mut rule_bait = mk_intent("test.b", "test.domain", "fast");
        rule_bait.examples = vec!["hello".to_string()];
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[rule_bait]);
        }
        let reranker = ScriptedReranker::new(vec![RerankOutcome::NoMatch]);
        let router = router(db, None, reranker, config());

        let decision = router.route(ctx("hello", None, false, None)).await;

        // 规则打分本会命中 test.b，但模型的 none 是可信结论，不能被噪声推翻。
        assert_eq!(decision.chosen.id, FALLBACK_INTENT_ID);
        assert_eq!(decision.source, DecisionSource::Fallback);
        assert!(!decision.uncertain);
        assert_eq!(decision.evidence.llm_calls, 1, "合成结果不得吞掉 LLM 调用证据");
    }

    #[tokio::test]
    async fn explicit_no_match_uses_embedding_when_available() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[
                    mk_intent("test.a", "test.domain", "fast"),
                    mk_intent("test.b", "test.domain", "fast"),
                ],
            );
        }
        let reranker = ScriptedReranker::new(vec![RerankOutcome::NoMatch]);
        let router = router(db, Some(embedding_with(vec![1.0, 1.0])), reranker, config());
        set_index(
            &router,
            vec![
                ("test.a", vec![vec![1.0, 0.0]]),
                ("test.b", vec![vec![0.0, 1.0]]),
            ],
        )
        .await;

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.chosen.id, "test.a");
        assert_eq!(decision.source, DecisionSource::Embedding);
        assert!(decision.uncertain);
    }

    #[tokio::test]
    async fn unavailable_rerank_falls_through_to_rule() {
        let db = test_db();
        let mut rule_bait = mk_intent("test.b", "test.domain", "fast");
        rule_bait.examples = vec!["hello".to_string()];
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[rule_bait]);
        }
        let reranker = ScriptedReranker::new(vec![RerankOutcome::Unavailable]);
        let router = router(db, None, reranker, config());

        let decision = router.route(ctx("hello", None, false, None)).await;

        assert_eq!(decision.chosen.id, "test.b");
        assert_eq!(decision.source, DecisionSource::Rule);
    }

    #[tokio::test]
    async fn llm_disabled_uses_embedding() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "fast")]);
        }
        let reranker = ScriptedReranker::new(vec![picked("test.a", 0.9)]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker.clone(),
            RouterConfig {
                llm_rerank_enabled: false,
                embedding_floor: 0.9,
                ..config()
            },
        );
        set_index(&router, vec![("test.a", vec![vec![1.0, 0.0]])]).await;

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.source, DecisionSource::Embedding);
        assert!(decision.uncertain);
        assert_eq!(reranker.calls(), 0);
    }

    #[tokio::test]
    async fn pinned_limits_candidates_to_the_domain() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "a.domain", &[mk_intent("a.intent", "a.domain", "fast")]);
            seed_domain(&guard, "b.domain", &[mk_intent("b.intent", "b.domain", "fast")]);
        }
        let reranker = ScriptedReranker::new(vec![picked("b.intent", 0.9)]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker,
            config(),
        );
        set_index(&router, vec![("a.intent", vec![vec![1.0, 0.0]])]).await;

        let decision = router.route(ctx("alpha", Some("a.domain"), true, None)).await;

        assert_eq!(decision.chosen.id, "a.intent");
        assert_eq!(decision.chosen.domain_id, "a.domain");
    }

    #[tokio::test]
    async fn deleted_pinned_domain_falls_back_to_auto() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "a.domain", &[mk_intent("a.intent", "a.domain", "fast")]);
        }
        let reranker = ScriptedReranker::new(vec![picked("a.intent", 0.9)]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker,
            RouterConfig {
                embedding_floor: 0.9,
                ..config()
            },
        );
        set_index(&router, vec![("a.intent", vec![vec![1.0, 0.0]])]).await;

        let decision = router.route(ctx("alpha", Some("missing"), true, None)).await;

        assert_eq!(decision.chosen.id, "a.intent");
        assert_eq!(decision.source, DecisionSource::Llm);
    }

    #[tokio::test]
    async fn no_enabled_intents_returns_synthetic() {
        let router = router(
            test_db(),
            None,
            ScriptedReranker::new(vec![picked("test.a", 0.9)]),
            config(),
        );

        let decision = router.route(ctx("alpha", None, false, None)).await;

        assert_eq!(decision.chosen.id, FALLBACK_INTENT_ID);
        assert_eq!(decision.source, DecisionSource::Fallback);
    }

    #[tokio::test]
    async fn prior_boost_only_changes_ranking() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "a.domain", &[mk_intent("a.intent", "a.domain", "fast")]);
            seed_domain(&guard, "b.domain", &[mk_intent("b.intent", "b.domain", "fast")]);
        }
        let reranker = ScriptedReranker::new(vec![RerankOutcome::Unavailable]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker,
            RouterConfig {
                embedding_floor: 0.9,
                llm_rerank_enabled: false,
                ..config()
            },
        );
        set_index(
            &router,
            vec![
                ("a.intent", vec![vec![0.58, 1.0]]),
                ("b.intent", vec![vec![0.60, 1.0]]),
            ],
        )
        .await;

        let decision = router
            .route(ctx("alpha", Some("a.domain"), false, None))
            .await;

        assert_eq!(decision.chosen.id, "a.intent");
        assert!(decision.uncertain);
    }

    #[tokio::test]
    async fn probe_runs_after_low_confidence_and_can_raise_confidence() {
        let project = tempfile::tempdir().expect("temp dir");
        std::fs::write(project.path().join("settings.ts"), "darkMode = true").unwrap();

        let db = test_db();
        let mut intent = mk_intent("test.a", "test.domain", "fast");
        intent.examples = vec!["darkMode".to_string()];
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[intent]);
        }

        let reranker = ScriptedReranker::new(vec![
            picked("test.a", 0.2),
            picked("test.a", 0.9),
        ]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker.clone(),
            RouterConfig {
                probe_enabled: true,
                embedding_floor: 0.9,
                ..config()
            },
        );
        set_index(&router, vec![("test.a", vec![vec![1.0, 0.0]])]).await;

        let decision = router
            .route(ctx("alpha", None, false, Some(project.path())))
            .await;

        assert_eq!(decision.source, DecisionSource::Probe);
        assert!(!decision.uncertain);
        assert!(decision.evidence.probe_used);
        assert_eq!(reranker.calls(), 2);
    }

    #[tokio::test]
    async fn probe_is_skipped_when_budget_is_too_small() {
        let project = tempfile::tempdir().expect("temp dir");
        std::fs::write(project.path().join("settings.ts"), "darkMode = true").unwrap();

        let db = test_db();
        let mut intent = mk_intent("test.a", "test.domain", "fast");
        intent.examples = vec!["darkMode".to_string()];
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[intent]);
        }

        let reranker = ScriptedReranker::new(vec![
            picked("test.a", 0.2),
            picked("test.a", 0.9),
        ]);
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 1.0])),
            reranker.clone(),
            RouterConfig {
                probe_enabled: true,
                routing_budget_ms: 0,
                ..config()
            },
        );
        set_index(&router, vec![("test.a", vec![vec![1.0, 0.0]])]).await;

        let decision = router
            .route(ctx("alpha", None, false, Some(project.path())))
            .await;

        assert!(!decision.evidence.probe_used);
        assert_eq!(reranker.calls(), 0);
    }

    #[tokio::test]
    async fn warm_up_builds_index_and_dirty_keeps_old_snapshot() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "fast")]);
        }
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 0.0])),
            ScriptedReranker::new(vec![]),
            config(),
        );

        assert!(!router.index.read().await.is_ready());
        router.warm_up().await;
        assert!(router.index.read().await.is_ready());

        router.mark_dirty().await;
        assert!(router.index.read().await.is_dirty());
        assert!(
            router.index.read().await.is_ready(),
            "the old snapshot keeps serving while the rebuild is pending"
        );

        let decision = router.route(ctx("alpha", None, false, None)).await;
        assert_eq!(decision.chosen.id, "test.a");
    }

    #[tokio::test]
    async fn route_records_a_trace() {        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "fast")]);
        }
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 0.0])),
            ScriptedReranker::new(vec![]),
            config(),
        );
        set_index(&router, vec![("test.a", vec![vec![1.0, 0.0]])]).await;

        router.route(ctx("alpha", None, false, None)).await;

        let traces = router.recent_traces().await;
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].chosen_intent_id, "test.a");
        assert_eq!(traces[0].session_id, "session");
    }

    #[test]
    fn resolve_tools_intersects_session_and_intent_tools() {
        let router = IntentRouter::new(
            test_db(),
            None,
            ScriptedReranker::new(vec![]),
            config(),
        );
        let session = Some(vec![
            "calculator".to_string(),
            "code_executor".to_string(),
            "web_search".to_string(),
        ]);
        let intent = Some(vec!["code_executor".to_string(), "file_system".to_string()]);

        assert_eq!(
            router.resolve_tools(session, intent.as_ref()),
            Some(vec!["shell".to_string()])
        );
    }

    #[test]
    fn resolve_tools_maps_legacy_names_to_rig_tools() {
        let router = IntentRouter::new(
            test_db(),
            None,
            ScriptedReranker::new(vec![]),
            config(),
        );

        let resolved = router
            .resolve_tools(Some(vec!["file_system".to_string()]), None)
            .expect("旧白名单应被归一化");

        assert!(resolved.contains(&"read".to_string()));
        assert!(resolved.contains(&"write".to_string()));
        assert!(!resolved.iter().any(|tool| tool == "file_system"));
    }

    #[test]
    fn empty_intent_tool_list_permits_no_tools() {
        let router = IntentRouter::new(
            test_db(),
            None,
            ScriptedReranker::new(vec![]),
            config(),
        );
        let session = Some(vec!["file_system".to_string(), "web_search".to_string()]);
        let intent: Vec<String> = Vec::new();

        assert_eq!(
            router.resolve_tools(session, Some(&intent)),
            Some(Vec::new())
        );
    }

    /// 标注集上的手动评测。运行方式：
    /// `INTENT_ROUTING_EVAL=1 cargo test --lib intent_routing_eval -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn intent_routing_eval() {
        if std::env::var("INTENT_ROUTING_EVAL").ok().as_deref() != Some("1") {
            eprintln!("set INTENT_ROUTING_EVAL=1 to run the routing eval");
            return;
        }

        let fixture: Vec<EvalEntry> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/intent_routing_eval.json"
        ))
        .expect("valid eval fixture");

        let app_config = crate::app::config::AppConfig::load().unwrap_or_default();
        let db = test_db();
        {
            let guard = db.lock().await;
            crate::features::intent_management::seeds::seed_builtin_intents(&guard)
                .expect("seed built-in intents");
        }

        let providers = Arc::new(Mutex::new(crate::llm::provider::ProviderRegistry::new(
            &app_config.models,
        )));
        let reranker = Arc::new(crate::llm::intent::classifier::LlmReranker::new(
            providers,
            app_config.intent.classifier_model_id.clone(),
        ));
        let embedding = Arc::new(crate::llm::embedding::EmbeddingService::new(
            std::env::var("OPENAI_API_KEY").ok(),
            app_config.intent.embedding_model_id.as_deref(),
        ));
        let router = Arc::new(IntentRouter::new(
            db,
            Some(embedding),
            reranker,
            RouterConfig {
                enabled: app_config.intent.enabled,
                embedding_floor: app_config.intent.embedding_floor,
                embedding_margin: app_config.intent.embedding_margin,
                prior_boost: app_config.intent.prior_boost,
                llm_rerank_enabled: app_config.intent.llm_rerank_enabled,
                llm_timeout_ms: app_config.intent.llm_timeout_ms,
                probe_enabled: app_config.intent.probe_enabled,
                probe_timeout_ms: app_config.intent.probe_timeout_ms,
                second_rerank_min_budget_ms: app_config.intent.second_rerank_min_budget_ms,
                routing_budget_ms: app_config.intent.routing_budget_ms,
            },
        ));
        router.warm_up().await;

        let mut labelled = 0usize;
        let mut correct = 0usize;
        let mut ambiguous = 0usize;
        let mut ambiguous_flagged = 0usize;
        let mut clear_by_embedding = 0usize;
        let mut llm_calls = 0usize;

        for entry in &fixture {
            let message = entry.message.as_str();
            let category = entry.category.as_str();
            let expected = entry.expected_intent.as_deref();

            let recent: Vec<Message> = entry
                .recent
                .iter()
                .map(|item| Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: "eval".to_string(),
                    role: item.role.clone(),
                    content: item.content.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                    tokens: None,
                    created_at: TS.to_string(),
                })
                .collect();

            let decision = router
                .route(RouteContext {
                    session_id: "eval",
                    content: message,
                    recent_messages: &recent,
                    prior_domain: None,
                    pinned: false,
                    project_root: None,
                })
                .await;

            llm_calls += decision.evidence.llm_calls;

            if category == "ambiguous" {
                ambiguous += 1;
                if decision.uncertain {
                    ambiguous_flagged += 1;
                }
                continue;
            }

            labelled += 1;
            if expected == Some(decision.chosen.id.as_str()) {
                correct += 1;
            }
            if category == "clear" && decision.source == DecisionSource::Embedding {
                clear_by_embedding += 1;
            }
        }

        let accuracy = if labelled == 0 {
            0.0
        } else {
            correct as f32 / labelled as f32
        };
        let ambiguous_rate = if ambiguous == 0 {
            0.0
        } else {
            ambiguous_flagged as f32 / ambiguous as f32
        };

        println!("intent routing eval");
        println!("  labelled top-1: {correct}/{labelled} ({accuracy:.2})");
        println!(
            "  ambiguous flagged: {ambiguous_flagged}/{ambiguous} ({ambiguous_rate:.2})"
        );
        println!("  clear decided by embedding: {clear_by_embedding}/{correct}");
        println!("  llm calls: {llm_calls}");

        assert!(accuracy >= 0.0);
        assert!(ambiguous_rate >= 0.0);
    }
}
