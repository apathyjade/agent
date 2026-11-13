use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

use crate::core::db::models::IntentRecord;
use crate::core::db::repository::Database;
use crate::llm::embedding::EmbeddingService;
use crate::llm::intent::classifier::{DecomposeOutcome, IntentDecomposer};
use crate::llm::intent::index::{intent_units, IntentVectorIndex};
use crate::llm::intent::probe::probe;
use crate::llm::intent::scope::{
    self, CatalogTool, IntentScopeV1, ResourceCatalog, ScopeDraft,
};
use crate::llm::intent::{
    DecisionEvidence, DecisionSource, DecomposedIntent, Decomposition, IntentCandidate,
    IntentConfig, IntentResult, PathSelection, RouteContext, RouteTrace, RouterConfig, ScopeStatus,
    ScopeTrace, TraceIntent,
};

/// 不参与拆解的意图：闲聊的配方等价于普通聊天，澄清是模型占位意图。
const EXCLUDED_INTENT_IDS: [&str; 2] = [
    "builtin.software_dev.chat",
    "builtin.software_dev.clarify",
];

/// 进程内保留的路由轨迹条数。
const TRACE_LIMIT: usize = 50;

/// 候选资源少于该数量时不做第二级收窄：收益不足，且会白花一次调用。
const SCOPE_NARROW_THRESHOLD: usize = 8;

/// 剩余路由预算低于该值时跳过第二级收窄。
const SCOPE_MIN_BUDGET_MS: u64 = 800;

/// 一次路由所需的全量数据快照：领域 id → 名称，以及所有启用意图。
struct RoutingData {
    domain_names: HashMap<String, String>,
    intents: Vec<IntentRecord>,
}

/// 意图路由器：把一条消息拆解为零个或多个意图，记录决策来源，并在进程内
/// 维护一个轨迹环形缓冲。
pub struct IntentRouter {
    db: Arc<Mutex<Database>>,
    embedding: Option<Arc<EmbeddingService>>,
    index: RwLock<IntentVectorIndex>,
    decomposer: Arc<dyn IntentDecomposer>,
    config: RouterConfig,
    traces: Mutex<VecDeque<RouteTrace>>,
    /// 可裁剪资源的只读快照；为空时跳过第二级收窄。
    ///
    /// 用 `std` 锁而不是 tokio 锁：只做整体替换与克隆，不跨 await 持有，
    /// 构造期（同步上下文）即可装配。
    catalog: std::sync::RwLock<ResourceCatalog>,
    /// 第二级收窄结果缓存，键为 `domain|intent_ids|catalog_sig`。
    scope_cache: std::sync::Mutex<HashMap<String, ScopeDraft>>,
}

impl IntentRouter {
    /// 用数据库句柄、可选的 embedding 服务、拆解器与配置构造路由器。
    ///
    /// `embedding` 为 `None` 时 embedding 索引永不就绪；拆解不再使用它，
    /// 保留该字段只为兼容记忆召回共享的客户端与既有预热流程。
    pub fn new(
        db: Arc<Mutex<Database>>,
        embedding: Option<Arc<EmbeddingService>>,
        decomposer: Arc<dyn IntentDecomposer>,
        config: RouterConfig,
    ) -> Self {
        Self {
            db,
            embedding,
            index: RwLock::new(IntentVectorIndex::new()),
            decomposer,
            config,
            traces: Mutex::new(VecDeque::new()),
            catalog: std::sync::RwLock::new(ResourceCatalog::default()),
            scope_cache: std::sync::Mutex::new(HashMap::new()),
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
        intersect_tools(session_tools, intent_tools)
    }

    /// 解析本回合的最终作用域：`会话 ∩ 意图 scope ∩ 收窄建议`。
    ///
    /// 没有目录、零意图或三层都没有任何声明时保持 `resolved = None`，调用方
    /// 沿用旧的白名单逻辑，因此既有行为不变。
    async fn resolve_scope(&self, ctx: &RouteContext<'_>, decomposition: &mut Decomposition) {
        if decomposition.intents.is_empty() {
            return;
        }
        let catalog = self
            .catalog
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        if catalog.tools().is_empty() {
            return;
        }

        let mut scopes: Vec<IntentScopeV1> = Vec::new();
        for item in &decomposition.intents {
            let stored = {
                let db = self.db.lock().await;
                db.get_scope(
                    crate::core::db::scope_repo::OWNER_INTENT,
                    &item.intent.id,
                )
                .ok()
                .flatten()
            };
            let scope = stored
                .and_then(|json| serde_json::from_str::<IntentScopeV1>(&json).ok())
                .or_else(|| {
                    item.intent
                        .config
                        .enabled_tools
                        .as_ref()
                        .map(|tools| IntentScopeV1::allow_exact(&normalize_tool_names(tools.clone())))
                });
            if let Some(scope) = scope {
                scopes.push(scope);
            }
        }

        let session_scope = ctx
            .session_tools
            .map(|tools| IntentScopeV1::allow_exact(&normalize_tool_names(tools.to_vec())));
        if session_scope.is_none() && scopes.is_empty() && decomposition.scope.is_none() {
            return;
        }

        let intent_refs: Vec<&IntentScopeV1> = scopes.iter().collect();
        decomposition.resolved = Some(scope::resolve(
            &catalog,
            scope::ScopeLayers {
                session: session_scope.as_ref(),
                intents: &intent_refs,
                draft: decomposition.scope.as_ref(),
                ..scope::ScopeLayers::default()
            },
        ));
    }

    /// 把一条消息拆解为零个或多个意图。永不返回 `Err`：任何失败、超时或
    /// 没有候选都降级为零意图（普通聊天兜底），保证消息仍能被回答。
    ///
    /// 每次拆解都会写入一条 [`RouteTrace`]（记录意图列表），作为只读调试数据。
    pub async fn decompose(&self, ctx: RouteContext<'_>) -> Decomposition {
        let started = Instant::now();
        let mut decomposition = self.decompose_inner(&ctx).await;
        self.apply_scope_draft(&ctx, &mut decomposition, started).await;
        self.resolve_scope(&ctx, &mut decomposition).await;
        decomposition.evidence.elapsed_ms = started.elapsed().as_millis() as u64;
        self.record_trace(&decomposition, &ctx).await;
        decomposition
    }

    /// 用新的资源目录替换快照，并清空收窄缓存。
    ///
    /// 目录变化（MCP 连接/断开、工具开关）后必须调用一次，否则缓存会给出
    /// 已经失效的资源集合。
    pub fn set_catalog(&self, catalog: ResourceCatalog) {
        if let Ok(mut guard) = self.catalog.write() {
            *guard = catalog;
        }
        if let Ok(mut cache) = self.scope_cache.lock() {
            cache.clear();
        }
    }

    /// 第二级路由：在候选资源里让模型挑出本回合真正要用的子集。
    ///
    /// 任何失败都只意味着「本回合不收窄」，绝不阻断意图路由。
    async fn apply_scope_draft(
        &self,
        ctx: &RouteContext<'_>,
        decomposition: &mut Decomposition,
        started: Instant,
    ) {
        if decomposition.intents.is_empty() || !self.config.llm_rerank_enabled {
            return;
        }

        let catalog = self
            .catalog
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        if catalog.tools().is_empty() {
            decomposition.scope_status = ScopeStatus::SkippedNoCatalog;
            return;
        }

        let candidates: Vec<CatalogTool> = catalog
            .tools()
            .iter()
            .filter(|tool| tool.enabled)
            .cloned()
            .collect();
        let catalog_size = candidates.len();
        if catalog_size <= SCOPE_NARROW_THRESHOLD {
            decomposition.scope_status = ScopeStatus::SkippedSmallCatalog;
            return;
        }

        let left = budget_left(started, self.config.routing_budget_ms);
        if left < Duration::from_millis(SCOPE_MIN_BUDGET_MS) {
            decomposition.scope_status = ScopeStatus::SkippedNoBudget;
            return;
        }

        let key = scope_cache_key(ctx, decomposition, &catalog);
        if let Some(cached) = self
            .scope_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
        {
            let selected = cached.tools.as_ref().map_or(0, |tools| tools.len());
            decomposition.scope = Some(cached);
            decomposition.scope_status = ScopeStatus::Applied {
                selected,
                catalog_size,
                cached: true,
            };
            return;
        }

        let timeout_ms = self
            .config
            .llm_timeout_ms
            .min(left.as_millis() as u64)
            .max(1);
        let outcome = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            self.decomposer.narrow_scope(ctx.content, &candidates),
        )
        .await
        .ok()
        .flatten();

        let Some(draft) = outcome else {
            decomposition.scope_status = ScopeStatus::SkippedUnavailable;
            return;
        };        let selected = draft.tools.as_ref().map_or(0, |tools| tools.len());
        decomposition.scope_status = ScopeStatus::Applied {
            selected,
            catalog_size,
            cached: false,
        };
        decomposition.scope = Some(draft.clone());
        if let Ok(mut cache) = self.scope_cache.lock() {
            cache.insert(key, draft);
        }
    }

    async fn decompose_inner(&self, ctx: &RouteContext<'_>) -> Decomposition {
        let started = Instant::now();

        if !self.config.enabled {
            return Self::finish_zero(
                DecisionSource::Fallback,
                Vec::new(),
                DecisionEvidence::default(),
                started,
            );
        }

        let Some(data) = self.load_routing_data().await else {
            return Self::finish_zero(
                DecisionSource::Fallback,
                Vec::new(),
                DecisionEvidence::default(),
                started,
            );
        };

        if data.intents.is_empty() {
            log::warn!("intent routing: no enabled intents in any domain");
            return Self::finish_zero(
                DecisionSource::Fallback,
                Vec::new(),
                DecisionEvidence::default(),
                started,
            );
        }

        let Some(candidates) = Self::select_candidates(&data, &ctx) else {
            return Self::finish_zero(
                DecisionSource::Fallback,
                Vec::new(),
                DecisionEvidence::default(),
                started,
            );
        };

        let candidates = Self::decompose_candidates(candidates);
        if candidates.is_empty() {
            log::warn!("intent routing: pinned domain has no decomposable intents");
            return Self::finish_zero(
                DecisionSource::Fallback,
                Vec::new(),
                DecisionEvidence::default(),
                started,
            );
        }

        let mut evidence = DecisionEvidence::default();
        if !self.config.llm_rerank_enabled {
            return Self::finish_zero(DecisionSource::Fallback, Vec::new(), evidence, started);
        }

        let wait = budget_left(started, self.config.routing_budget_ms)
            .min(Duration::from_millis(self.config.llm_timeout_ms));
        if wait.is_zero() {
            // 预算在调用前就耗尽：不是「模型判定零意图」，而是没能完成拆解。
            return Self::finish_zero(DecisionSource::Unavailable, Vec::new(), evidence, started);
        }

        evidence.llm_calls += 1;
        let outcome = tokio::time::timeout(
            wait,
            self.decomposer
                .decompose(ctx.content, ctx.recent_messages, &candidates),
        )
        .await;

        let items = match outcome {
            Ok(DecomposeOutcome::Parsed(items)) => items,
            // 超时、调用失败或没有可用候选：标记为不可用，调用方不得把它缓存成
            // 零意图会话（否则一次瞬时失败会被永久固化）。
            Ok(DecomposeOutcome::Unavailable) | Err(_) => {
                return Self::finish_zero(
                    DecisionSource::Unavailable,
                    Vec::new(),
                    evidence,
                    started,
                );
            }
        };

        // 模型明确返回空数组：这是合法的零意图，走普通聊天兜底。
        if items.is_empty() {
            return Self::finish_zero(DecisionSource::Fallback, Vec::new(), evidence, started);
        }

        let by_id: HashMap<&str, &IntentRecord> = candidates
            .iter()
            .map(|intent| (intent.id.as_str(), intent))
            .collect();

        let mut intents = Vec::with_capacity(items.len());
        let mut candidates_meta = Vec::new();
        for item in &items {
            let Some(record) = by_id.get(item.intent_id.as_str()).copied() else {
                continue;
            };
            let uncertain = item.confidence < record.confidence_threshold as f32;
            push_candidate(
                &mut candidates_meta,
                record,
                item.confidence,
                DecisionSource::Llm,
            );
            intents.push(DecomposedIntent::new(
                Self::project(record, data.domain_name(&record.domain_id), item.confidence),
                item.instruction.clone(),
                item.confidence,
                uncertain,
            ));
        }

        if intents.is_empty() {
            return Self::finish_zero(DecisionSource::Fallback, candidates_meta, evidence, started);
        }

        // 全部低置信 → 整体视同零意图，不执行任何领域意图。
        if intents.iter().all(|item| item.uncertain) {
            return Self::finish_zero(DecisionSource::Fallback, candidates_meta, evidence, started);
        }

        evidence.llm_confidence = Some(
            intents
                .iter()
                .map(|item| item.confidence)
                .fold(f32::INFINITY, f32::min),
        );

        // 只读探针：只为不确定项附加项目证据，不重排、不改置信度。
        if self.config.probe_enabled && intents.iter().any(|item| item.uncertain) {
            if let Some(project_root) = ctx.project_root {
                for index in 0..intents.len() {
                    if !intents[index].uncertain {
                        continue;
                    }
                    let Some(record) = by_id
                        .get(intents[index].intent.id.as_str())
                        .copied()
                    else {
                        continue;
                    };
                    let wait = budget_left(started, self.config.routing_budget_ms)
                        .min(Duration::from_millis(self.config.probe_timeout_ms));
                    if wait.is_zero() {
                        break;
                    }
                    evidence.probe_used = true;
                    if let Some(text) = probe(record, project_root, wait).await {
                        intents[index].probe_evidence = Some(text.clone());
                        intents[index].instruction =
                            format!("Project evidence:\n{text}\n\n{}", intents[index].instruction);
                    }
                }
            }
        }

        let mut decomposition = Decomposition::from_intents(
            intents,
            DecisionSource::Llm,
            candidates_meta,
            evidence,
        );
        decomposition.evidence.elapsed_ms = started.elapsed().as_millis() as u64;
        decomposition
    }

    /// 按会话的领域绑定裁剪候选集。
    ///
    /// 领域是硬过滤：没有绑定、领域已不存在或领域内没有启用意图都返回 `None`，
    /// 调用方据此走普通聊天兜底。不存在 `auto`（全领域）分支。
    fn select_candidates(data: &RoutingData, ctx: &RouteContext<'_>) -> Option<Vec<IntentRecord>> {
        let Some(domain_id) = ctx.prior_domain.filter(|id| {
            let exists = data.domain_names.contains_key(*id);
            if !exists {
                log::warn!(
                    "intent routing: pinned domain '{}' no longer exists, falling back to chat",
                    id
                );
            }
            exists
        }) else {
            return None;
        };

        let candidates: Vec<IntentRecord> = data
            .intents
            .iter()
            .filter(|intent| intent.domain_id == domain_id)
            .cloned()
            .collect();

        if candidates.is_empty() {
            log::warn!("intent routing: pinned domain has no enabled intents");
            return None;
        }

        Some(candidates)
    }

    /// 拆解候选：排除 `chat` 与 `clarify` 两个内置意图。
    fn decompose_candidates(candidates: Vec<IntentRecord>) -> Vec<IntentRecord> {
        candidates
            .into_iter()
            .filter(|intent| !EXCLUDED_INTENT_IDS.contains(&intent.id.as_str()))
            .collect()
    }

    /// 构造零意图结果并补齐耗时。
    fn finish_zero(
        source: DecisionSource,
        candidates: Vec<IntentCandidate>,
        mut evidence: DecisionEvidence,
        started: Instant,
    ) -> Decomposition {
        evidence.elapsed_ms = started.elapsed().as_millis() as u64;
        Decomposition::zero_intent(source, candidates, evidence)
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

    /// 把拆解结果写入轨迹环形缓冲（最多保留 `TRACE_LIMIT` 条）。
    ///
    /// 轨迹记录完整意图列表，与 `intent_resolved` 事件保持同一口径。
    async fn record_trace(&self, decomposition: &Decomposition, ctx: &RouteContext<'_>) {
        let excerpt: String = ctx.content.chars().take(80).collect();
        let intents = decomposition
            .intents
            .iter()
            .map(|item| TraceIntent {
                intent_id: item.intent.id.clone(),
                intent_name: item.intent.name.clone(),
                instruction: item.instruction.clone(),
                confidence: item.confidence,
                uncertain: item.uncertain,
            })
            .collect();

        let trace = RouteTrace {
            at: Utc::now().to_rfc3339(),
            session_id: ctx.session_id.to_string(),
            message_excerpt: excerpt,
            intents,
            candidates: decomposition.candidates.clone(),
            confidence: decomposition.confidence,
            uncertain: decomposition.uncertain,
            source: decomposition.source,
            elapsed_ms: decomposition.evidence.elapsed_ms,
            llm_calls: decomposition.evidence.llm_calls,
            probe_used: decomposition.evidence.probe_used,
            scope: match decomposition.scope_status {
                ScopeStatus::NotRequested => None,
                ScopeStatus::Applied {
                    selected,
                    catalog_size,
                    cached,
                } => Some(ScopeTrace {
                    catalog_size,
                    selected,
                    cached,
                    skipped: None,
                }),
                skipped => Some(ScopeTrace {
                    catalog_size: 0,
                    selected: 0,
                    cached: false,
                    skipped: Some(skipped.label().to_string()),
                }),
            },
        };

        let mut traces = self.traces.lock().await;
        traces.push_front(trace);
        traces.truncate(TRACE_LIMIT);
    }

    /// 把意图行投影为调用方使用的 [`IntentResult`]。
    ///
    /// 空字符串字段投影为 `None`；`path` 只认 `deep`（大小写不敏感），其余一律
    /// 视为 fast；置信度收敛到 `[0, 1]`。
    pub fn project(record: &IntentRecord, domain_name: &str, confidence: f32) -> IntentResult {
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
            description: record.description.clone(),
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
    /// 后续消息沿用的工具白名单：会话白名单 ∩ 各已选意图白名单的**并集**。
    ///
    /// `intent_tools` 为空表示会话没有已选意图（不施加意图限制）；其中任一项
    /// 为 `None` 表示该意图不限制工具，并集因此也不限制。空数组仍然是
    /// 「没有工具」，与会话白名单求交后保持 `Some(vec![])`。
    pub fn resolve_union_tools(
        &self,
        session_tools: Option<Vec<String>>,
        intent_tools: &[Option<Vec<String>>],
    ) -> Option<Vec<String>> {
        if intent_tools.is_empty() {
            return self.resolve_tools(session_tools, None);
        }

        let mut union: Vec<String> = Vec::new();
        for tools in intent_tools {
            let Some(tools) = tools else {
                // 任一意图不限制工具 → 并集不限制。
                return self.resolve_tools(session_tools, None);
            };
            for tool in normalize_tool_names(tools.clone()) {
                push_unique_tool(&mut union, &tool);
            }
        }

        self.resolve_tools(session_tools, Some(&union))
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

/// 收窄缓存键：领域、意图集合与目录指纹共同决定结果。
fn scope_cache_key(
    ctx: &RouteContext<'_>,
    decomposition: &Decomposition,
    catalog: &ResourceCatalog,
) -> String {
    let intents: Vec<&str> = decomposition
        .intents
        .iter()
        .map(|item| item.intent.id.as_str())
        .collect();
    format!(
        "{}|{}|{:x}",
        ctx.prior_domain.unwrap_or(""),
        intents.join(","),
        catalog.signature()
    )
}

/// 根据会话限制与意图限制求最终工具列表（交集）。
///
/// 意图侧的显式列表是权威的：它列出该意图允许的全部工具，因此交集为空
/// 表示「没有任何工具」（`Some(vec![])`），而不是「不限制」。意图侧为
/// `None` 才表示该意图不施加限制。
pub fn intersect_tools(
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
    use crate::llm::intent::classifier::parse_decompose_reply;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const TS: &str = "2026-11-05T00:00:00Z";

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

    struct CountingEmbedder {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl TextEmbedder for CountingEmbedder {
        async fn embed(&self, _text: &str) -> Option<Vec<f32>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(vec![1.0, 0.0])
        }
    }

    enum ScriptedReply {
        Parsed(String),
        Unavailable,
        /// 永不返回的调用，用于验证超时被标记为不可用。
        Hang,
    }

    /// 用固定回复替身模拟 provider 输出；同时记录收到的候选与调用次数。
    struct ScriptedDecomposer {
        reply: ScriptedReply,
        calls: AtomicUsize,
        last_candidates: Mutex<Vec<String>>,
        /// 第二级路由返回的工具集合；`None` 表示不参与收窄。
        scope: Option<Vec<String>>,
        scope_calls: AtomicUsize,
    }

    impl ScriptedDecomposer {
        fn parsed(reply: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: ScriptedReply::Parsed(reply.to_string()),
                calls: AtomicUsize::new(0),
                last_candidates: Mutex::new(Vec::new()),
                scope: None,
                scope_calls: AtomicUsize::new(0),
            })
        }

        fn unavailable() -> Arc<Self> {
            Arc::new(Self {
                reply: ScriptedReply::Unavailable,
                calls: AtomicUsize::new(0),
                last_candidates: Mutex::new(Vec::new()),
                scope: None,
                scope_calls: AtomicUsize::new(0),
            })
        }

        fn hanging() -> Arc<Self> {
            Arc::new(Self {
                reply: ScriptedReply::Hang,
                calls: AtomicUsize::new(0),
                last_candidates: Mutex::new(Vec::new()),
                scope: None,
                scope_calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        /// 在 `parsed` 的基础上让第二级路由返回固定的一组工具。
        fn with_scope(reply: &str, tools: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                reply: ScriptedReply::Parsed(reply.to_string()),
                calls: AtomicUsize::new(0),
                last_candidates: Mutex::new(Vec::new()),
                scope: Some(tools.iter().map(|tool| tool.to_string()).collect()),
                scope_calls: AtomicUsize::new(0),
            })
        }

        fn scope_calls(&self) -> usize {
            self.scope_calls.load(Ordering::SeqCst)
        }

        async fn last_candidates(&self) -> Vec<String> {
            self.last_candidates.lock().await.clone()
        }
    }

    #[async_trait::async_trait]
    impl IntentDecomposer for ScriptedDecomposer {
        async fn narrow_scope(
            &self,
            _message: &str,
            _resources: &[CatalogTool],
        ) -> Option<ScopeDraft> {
            self.scope_calls.fetch_add(1, Ordering::SeqCst);
            self.scope.as_ref().map(|tools| ScopeDraft {
                tools: Some(tools.clone()),
                prompts: None,
            })
        }

        async fn decompose(
            &self,
            _message: &str,
            _recent_messages: &[Message],
            candidates: &[IntentRecord],
        ) -> DecomposeOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_candidates.lock().await =
                candidates.iter().map(|intent| intent.id.clone()).collect();
            match &self.reply {
                ScriptedReply::Parsed(reply) => {
                    DecomposeOutcome::Parsed(parse_decompose_reply(reply, candidates))
                }
                ScriptedReply::Unavailable => DecomposeOutcome::Unavailable,
                ScriptedReply::Hang => {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    DecomposeOutcome::Unavailable
                }
            }
        }
    }

    fn router(
        db: Arc<Mutex<Database>>,
        embedding: Option<Arc<EmbeddingService>>,
        decomposer: Arc<ScriptedDecomposer>,
        config: RouterConfig,
    ) -> Arc<IntentRouter> {
        Arc::new(IntentRouter::new(db, embedding, decomposer, config))
    }

    fn ctx<'a>(
        content: &'a str,
        domain: Option<&'a str>,
        project_root: Option<&'a std::path::Path>,
    ) -> RouteContext<'a> {
        RouteContext {
            session_id: "session",
            content,
            recent_messages: &[],
            prior_domain: domain,
            pinned: domain.is_some(),
            project_root,
            session_tools: None,
        }
    }

    fn config() -> RouterConfig {
        RouterConfig {
            probe_enabled: false,
            ..RouterConfig::default()
        }
    }

    fn reply(items: &[(&str, &str, f32)]) -> String {
        let entries: Vec<String> = items
            .iter()
            .map(|(id, instruction, confidence)| {
                format!(
                    r#"{{"intent_id":"{id}","instruction":"{instruction}","confidence":{confidence}}}"#
                )
            })
            .collect();
        format!(r#"{{"intents":[{}]}}"#, entries.join(","))
    }

    #[test]
    fn default_timeouts_cover_a_real_classifier_round_trip() {
        let config = RouterConfig::default();

        assert!(config.llm_timeout_ms >= 20000);
        assert!(
            config.routing_budget_ms > config.llm_timeout_ms,
            "总预算必须给探针留出空间"
        );
    }

    #[tokio::test]
    async fn second_stage_scope_is_applied_and_cached() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("alpha", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::with_scope(
            &reply(&[("alpha", "do it", 0.9)]),
            &["tool_0"],
        );
        let router = router(db, None, decomposer.clone(), config());
        router.set_catalog(ResourceCatalog::from_tools(
            (0..12).map(|index| CatalogTool::new(format!("tool_{index}"), "builtin")),
        ));

        let first = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;
        assert!(
            matches!(
                first.scope_status,
                ScopeStatus::Applied {
                    selected: 1,
                    cached: false,
                    ..
                }
            ),
            "首次必须真实调用收窄: {:?}",
            first.scope_status
        );
        assert_eq!(
            first.scope.as_ref().and_then(|draft| draft.tools.clone()),
            Some(vec!["tool_0".to_string()])
        );

        let second = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;
        assert!(
            matches!(
                second.scope_status,
                ScopeStatus::Applied { cached: true, .. }
            ),
            "第二次必须命中缓存: {:?}",
            second.scope_status
        );
        assert_eq!(decomposer.scope_calls(), 1, "缓存命中时不得再次调用模型");
    }

    #[tokio::test]
    async fn disabled_router_returns_zero_intent() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[("test.a", "do a", 0.9)]));
        let router = router(
            db,
            None,
            decomposer.clone(),
            RouterConfig {
                enabled: false,
                ..config()
            },
        );

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert!(decomposition.is_empty());
        assert_eq!(decomposer.calls(), 0);
    }

    #[tokio::test]
    async fn no_bound_domain_returns_zero_intent() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[("test.a", "do a", 0.9)]));
        let router = router(db, None, decomposer.clone(), config());

        let decomposition = router.decompose(ctx("alpha", None, None)).await;

        assert!(decomposition.is_empty(), "无领域绑定必须走普通聊天兜底");
        assert_eq!(decomposer.calls(), 0);
    }

    #[tokio::test]
    async fn domain_without_enabled_intents_returns_zero_intent() {
        let db = test_db();
        {
            let guard = db.lock().await;
            // 领域存在但没有任何意图。
            seed_domain(&guard, "test.domain", &[]);
            seed_domain(
                &guard,
                "other.domain",
                &[mk_intent("other.intent", "other.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[("other.intent", "do it", 0.9)]));
        let router = router(db, None, decomposer.clone(), config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert!(
            decomposition.is_empty(),
            "绑定领域无启用意图时必须走普通聊天兜底"
        );
        assert_eq!(decomposer.calls(), 0);
    }

    #[tokio::test]
    async fn deleted_domain_returns_zero_intent() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[("test.a", "do a", 0.9)]));
        let router = router(db, None, decomposer.clone(), config());

        let decomposition = router.decompose(ctx("alpha", Some("missing"), None)).await;

        assert!(decomposition.is_empty(), "领域不存在时不得跨领域挑选");
        assert_eq!(decomposer.calls(), 0);
    }

    #[tokio::test]
    async fn decomposes_multiple_intents_in_model_order() {
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
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.b", "do b", 0.8),
            ("test.a", "do a", 0.9),
        ]));
        let router = router(db, None, decomposer.clone(), config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert_eq!(decomposition.intents.len(), 2);
        assert_eq!(decomposition.intents[0].intent.id, "test.b");
        assert_eq!(decomposition.intents[0].instruction, "do b");
        assert_eq!(decomposition.intents[1].intent.id, "test.a");
        assert_eq!(decomposition.source, DecisionSource::Llm);
        assert_eq!(decomposition.evidence.llm_calls, 1);
        assert_eq!(decomposer.calls(), 1);
    }

    #[tokio::test]
    async fn hallucinated_intent_is_dropped_from_router_result() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.ghost", "hallucinated", 0.99),
            ("test.a", "do a", 0.9),
        ]));
        let router = router(db, None, decomposer, config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert_eq!(decomposition.intents.len(), 1);
        assert_eq!(decomposition.intents[0].intent.id, "test.a");
        assert!(
            decomposition
                .candidates
                .iter()
                .all(|candidate| candidate.intent_id != "test.ghost"),
            "幻觉 id 不得出现在候选与结果中"
        );
    }

    #[tokio::test]
    async fn empty_intents_reply_is_zero_intent() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(r#"{"intents": []}"#);
        let router = router(db, None, decomposer.clone(), config());

        let decomposition = router
            .decompose(ctx("你好", Some("test.domain"), None))
            .await;

        assert!(decomposition.is_empty());
        assert_eq!(
            decomposition.source,
            DecisionSource::Fallback,
            "模型明确返回空数组是合法零意图"
        );
        assert_eq!(decomposition.evidence.llm_calls, 1, "零意图仍是一次真实调用");
        assert_eq!(decomposer.calls(), 1);
    }

    #[tokio::test]
    async fn unavailable_decomposer_is_marked_unavailable() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::unavailable();
        let router = router(db, None, decomposer.clone(), config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert!(decomposition.is_empty(), "拆解失败必须走普通聊天兜底");
        assert_eq!(
            decomposition.source,
            DecisionSource::Unavailable,
            "调用失败必须与「模型明确返回空数组」区分开"
        );
        assert_eq!(decomposition.evidence.llm_calls, 1);
        assert_eq!(decomposer.calls(), 1);
    }

    /// 超时与调用失败同样标记为不可用，而不是合法零意图。
    #[tokio::test]
    async fn timed_out_decomposer_is_marked_unavailable() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::hanging();
        let router = router(
            db,
            None,
            decomposer,
            RouterConfig {
                llm_timeout_ms: 10,
                routing_budget_ms: 1000,
                ..config()
            },
        );

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert!(decomposition.is_empty());
        assert_eq!(decomposition.source, DecisionSource::Unavailable);
    }

    #[tokio::test]
    async fn candidates_exclude_chat_clarify_and_other_domains() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[
                    mk_intent("test.a", "test.domain", "fast"),
                    mk_intent("builtin.software_dev.chat", "test.domain", "fast"),
                    mk_intent("builtin.software_dev.clarify", "test.domain", "fast"),
                ],
            );
            seed_domain(
                &guard,
                "other.domain",
                &[mk_intent("other.intent", "other.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(r#"{"intents": []}"#);
        let router = router(db, None, decomposer.clone(), config());

        router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        let candidates = decomposer.last_candidates().await;
        assert_eq!(candidates, vec!["test.a".to_string()]);
    }

    #[tokio::test]
    async fn low_confidence_item_is_kept_and_flagged() {
        let db = test_db();
        {
            let guard = db.lock().await;
            let mut uncertain = mk_intent("test.a", "test.domain", "fast");
            uncertain.confidence_threshold = 0.9;
            seed_domain(
                &guard,
                "test.domain",
                &[uncertain, mk_intent("test.b", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.a", "do a", 0.2),
            ("test.b", "do b", 0.9),
        ]));
        let router = router(db, None, decomposer, config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert_eq!(decomposition.intents.len(), 2);
        assert!(decomposition.intents[0].uncertain);
        assert!(!decomposition.intents[1].uncertain);
        assert!(decomposition.uncertain);
    }

    #[tokio::test]
    async fn all_low_confidence_becomes_zero_intent() {
        let db = test_db();
        {
            let guard = db.lock().await;
            let mut a = mk_intent("test.a", "test.domain", "fast");
            a.confidence_threshold = 0.9;
            let mut b = mk_intent("test.b", "test.domain", "fast");
            b.confidence_threshold = 0.9;
            seed_domain(&guard, "test.domain", &[a, b]);
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.a", "do a", 0.2),
            ("test.b", "do b", 0.3),
        ]));
        let router = router(db, None, decomposer, config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert!(
            decomposition.is_empty(),
            "全部低置信必须整体视同零意图"
        );
        assert_eq!(decomposition.source, DecisionSource::Fallback);
    }

    #[tokio::test]
    async fn probe_attaches_evidence_to_uncertain_items_only() {
        let project = tempfile::tempdir().expect("temp dir");
        std::fs::write(project.path().join("settings.ts"), "darkMode = true").unwrap();

        let db = test_db();
        {
            let guard = db.lock().await;
            let mut uncertain = mk_intent("test.a", "test.domain", "fast");
            uncertain.confidence_threshold = 0.9;
            uncertain.examples = vec!["darkMode".to_string()];
            seed_domain(
                &guard,
                "test.domain",
                &[uncertain, mk_intent("test.b", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.a", "do a", 0.2),
            ("test.b", "do b", 0.9),
        ]));
        let router = router(
            db,
            None,
            decomposer,
            RouterConfig {
                probe_enabled: true,
                ..config()
            },
        );

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), Some(project.path())))
            .await;

        assert_eq!(decomposition.intents.len(), 2);
        assert_eq!(decomposition.intents[0].intent.id, "test.a");
        assert_eq!(decomposition.intents[1].intent.id, "test.b", "探针不得重排");
        assert_eq!(decomposition.intents[0].confidence, 0.2, "探针不得改置信度");
        assert_eq!(decomposition.intents[0].confidence, 0.2);
        assert!(decomposition.intents[0].probe_evidence.is_some());
        assert!(decomposition.intents[0]
            .instruction
            .starts_with("Project evidence:"));
        assert!(decomposition.intents[1].probe_evidence.is_none());
        assert!(decomposition.evidence.probe_used);
    }

    #[tokio::test]
    async fn probe_is_skipped_without_project_root() {
        let db = test_db();
        {
            let guard = db.lock().await;
            let mut uncertain = mk_intent("test.a", "test.domain", "fast");
            uncertain.confidence_threshold = 0.9;
            seed_domain(
                &guard,
                "test.domain",
                &[uncertain, mk_intent("test.b", "test.domain", "fast")],
            );
        }
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.a", "do a", 0.2),
            ("test.b", "do b", 0.9),
        ]));
        let router = router(
            db,
            None,
            decomposer,
            RouterConfig {
                probe_enabled: true,
                ..config()
            },
        );

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert_eq!(decomposition.intents.len(), 2);
        assert!(decomposition.intents[0].probe_evidence.is_none());
        assert!(!decomposition.evidence.probe_used);
    }

    #[tokio::test]
    async fn embedding_and_rule_signals_are_not_used() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let embedder = Arc::new(CountingEmbedder {
            calls: AtomicUsize::new(0),
        });
        let embedding = Arc::new(EmbeddingService::with_embedder(embedder.clone()));
        let decomposer = ScriptedDecomposer::parsed(&reply(&[("test.a", "do a", 0.9)]));
        let router = router(db, Some(embedding), decomposer.clone(), config());

        let decomposition = router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        assert_eq!(
            embedder.calls.load(Ordering::SeqCst),
            0,
            "拆解不得调用 embedding 召回"
        );
        assert_eq!(decomposition.source, DecisionSource::Llm);
        assert_eq!(decomposer.calls(), 1);
    }

    #[tokio::test]
    async fn decompose_records_a_trace_with_the_intent_list() {
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
        let decomposer = ScriptedDecomposer::parsed(&reply(&[
            ("test.b", "do b", 0.8),
            ("test.a", "do a", 0.9),
        ]));
        let router = router(db, None, decomposer, config());

        router
            .decompose(ctx("alpha", Some("test.domain"), None))
            .await;

        let traces = router.recent_traces().await;
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].session_id, "session");
        assert_eq!(traces[0].intents.len(), 2);
        assert_eq!(traces[0].intents[0].intent_id, "test.b");
        assert_eq!(traces[0].intents[0].instruction, "do b");
        assert_eq!(traces[0].intents[1].intent_name, "意图 test.a");
        assert_eq!(traces[0].intents[1].confidence, 0.9);
    }

    #[tokio::test]
    async fn warm_up_builds_index_and_dirty_keeps_old_snapshot() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.a", "test.domain", "fast")],
            );
        }
        let router = router(
            db,
            Some(embedding_with(vec![1.0, 0.0])),
            ScriptedDecomposer::unavailable(),
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
    }

    #[test]
    fn resolve_tools_intersects_session_and_intent_tools() {
        let router = IntentRouter::new(
            test_db(),
            None,
            ScriptedDecomposer::unavailable(),
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
            ScriptedDecomposer::unavailable(),
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
            ScriptedDecomposer::unavailable(),
            config(),
        );
        let session = Some(vec!["file_system".to_string(), "web_search".to_string()]);
        let intent: Vec<String> = Vec::new();

        assert_eq!(
            router.resolve_tools(session, Some(&intent)),
            Some(Vec::new())
        );
    }

    #[test]
    fn union_tools_takes_the_union_then_intersects_the_session() {
        let router = IntentRouter::new(
            test_db(),
            None,
            ScriptedDecomposer::unavailable(),
            config(),
        );
        let session = Some(vec!["read".to_string(), "shell".to_string()]);
        let intents = [
            Some(vec!["read".to_string()]),
            Some(vec!["read".to_string(), "shell".to_string()]),
        ];

        let resolved = router.resolve_union_tools(session, &intents);

        assert_eq!(
            resolved,
            Some(vec!["read".to_string(), "shell".to_string()])
        );
    }

    #[test]
    fn union_tools_stays_empty_when_no_intent_allows_any_tool() {
        let router = IntentRouter::new(
            test_db(),
            None,
            ScriptedDecomposer::unavailable(),
            config(),
        );
        let session = Some(vec!["read".to_string()]);
        let intents = [Some(Vec::new()), Some(Vec::new())];

        assert_eq!(
            router.resolve_union_tools(session, &intents),
            Some(Vec::new())
        );
    }

    /// 拆解评测集里的一条样例。
    #[derive(serde::Deserialize)]
    struct DecomposeEvalEntry {
        message: String,
        #[serde(default)]
        expected_intents: Vec<String>,
        #[serde(default = "default_decompose_category")]
        category: String,
    }

    fn default_decompose_category() -> String {
        "single".to_string()
    }

    fn eval_p50(mut samples: Vec<u64>) -> u64 {
        if samples.is_empty() {
            return 0;
        }
        samples.sort_unstable();
        samples[samples.len() / 2]
    }

    /// 多意图拆解评测的手动通道。运行方式：
    /// `INTENT_ROUTING_EVAL=1 cargo test --lib intent_decomposition_eval -- --ignored --nocapture`
    ///
    /// 指标：集合准确率（全部 / 排除 ambiguous / single / multi）、多意图顺序准确率、
    /// 意图召回、零意图准确率、ambiguous 的 uncertain 覆盖率、幻觉率、调用次数与 p50 耗时。
    #[tokio::test]
    #[ignore]
    async fn intent_decomposition_eval() {
        if std::env::var("INTENT_ROUTING_EVAL").ok().as_deref() != Some("1") {
            eprintln!("set INTENT_ROUTING_EVAL=1 to run the decomposition eval");
            return;
        }

        let fixture: Vec<DecomposeEvalEntry> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/intent_decomposition_eval.json"
        ))
        .expect("valid decomposition eval fixture");

        let app_config = crate::app::config::AppConfig::load().unwrap_or_default();
        // `AppConfig::load` 不会填充 keychain 里的 key；这里补齐，否则所有
        // provider 都会因空 key 失败。
        let mut models = app_config.models.clone();
        for model in &mut models {
            model.api_key = crate::core::keychain::resolve_api_key(&model.id, &model.api_key);
        }

        let db = test_db();
        let allowed_ids: HashSet<String> = {
            let guard = db.lock().await;
            crate::features::intent_management::seeds::seed_builtin_intents(&guard)
                .expect("seed built-in intents");
            guard
                .list_all_enabled_intents()
                .expect("list intents")
                .into_iter()
                .map(|intent| intent.id)
                .collect()
        };

        let providers = Arc::new(Mutex::new(crate::llm::provider::ProviderRegistry::new(
            &models,
        )));
        // 允许用 `INTENT_EVAL_MODEL_ID` 覆盖拆解模型，便于横向比较不同模型的
        // 准确率与延迟（例如换用更快的模型来消除超时）。
        let eval_model_id = std::env::var("INTENT_EVAL_MODEL_ID")
            .ok()
            .filter(|id| !id.trim().is_empty())
            .or_else(|| app_config.intent.classifier_model_id.clone());
        let decomposer = Arc::new(crate::llm::intent::classifier::LlmReranker::new(
            providers,
            eval_model_id,
        ));
        let router = Arc::new(IntentRouter::new(
            db,
            None,
            decomposer,
            RouterConfig {
                enabled: app_config.intent.enabled,
                llm_rerank_enabled: app_config.intent.llm_rerank_enabled,
                llm_timeout_ms: app_config.intent.llm_timeout_ms,
                probe_enabled: app_config.intent.probe_enabled,
                probe_timeout_ms: app_config.intent.probe_timeout_ms,
                routing_budget_ms: app_config.intent.routing_budget_ms,
            },
        ));

        let mut single_total = 0usize;
        let mut single_correct = 0usize;
        let mut zero_total = 0usize;
        let mut zero_correct = 0usize;
        let mut multi_total = 0usize;
        let mut multi_correct = 0usize;
        let mut multi_ordered_correct = 0usize;
        let mut ambiguous_total = 0usize;
        let mut ambiguous_uncertain = 0usize;
        let mut set_correct = 0usize;
        let mut non_ambiguous_total = 0usize;
        let mut non_ambiguous_correct = 0usize;
        let mut recall_hit = 0usize;
        let mut recall_total = 0usize;
        let mut predicted_total = 0usize;
        let mut hallucinated = 0usize;
        let mut llm_calls = 0usize;
        let mut latencies = Vec::new();

        for entry in &fixture {
            let started = Instant::now();
            let decomposition = router
                .decompose(RouteContext {
                    session_id: "eval",
                    content: &entry.message,
                    recent_messages: &[],
                    prior_domain: Some("builtin.software_dev"),
                    pinned: true,
                    project_root: None,
                    session_tools: None,
                })
                .await;
            latencies.push(started.elapsed().as_millis() as u64);
            llm_calls += decomposition.evidence.llm_calls;

            let predicted: HashSet<String> = decomposition
                .intents
                .iter()
                .map(|item| item.intent.id.clone())
                .collect();
            let expected: HashSet<String> = entry.expected_intents.iter().cloned().collect();

            predicted_total += predicted.len();
            hallucinated += predicted.iter().filter(|id| !allowed_ids.contains(*id)).count();

            // 集合准确率：预测集合与期望集合完全一致（跨全部子集）。
            if predicted == expected {
                set_correct += 1;
            }
            // ambiguous 是设计上允许判错的子集（spec 只要求标 uncertain），
            // 混进总准确率会低估模型，因此单独给一个排除后的口径。
            if entry.category != "ambiguous" {
                non_ambiguous_total += 1;
                if predicted == expected {
                    non_ambiguous_correct += 1;
                }
            }
            // 单意图召回：期望集合里的每个意图是否都被拆出来（漏一个即失败）。
            for id in &expected {
                recall_total += 1;
                if predicted.contains(id) {
                    recall_hit += 1;
                }
            }

            match entry.category.as_str() {
                "single" => {
                    single_total += 1;
                    if predicted == expected {
                        single_correct += 1;
                    }
                }
                "multi" => {
                    multi_total += 1;
                    // 顺序：模型返回的意图顺序必须与标注顺序一致。
                    let predicted_order: Vec<String> = decomposition
                        .intents
                        .iter()
                        .map(|item| item.intent.id.clone())
                        .collect();
                    if predicted == expected {
                        multi_correct += 1;
                    } else {
                        // 失败样本：把消息、期望集合、实际集合、耗时与决策来源都打出来，
                        // 便于区分「模型真的返回空数组」「响应超时被吞成零意图」
                        // 与「标注本身有歧义」三种原因。
                        println!(
                            "  [multi miss] message: {}\n    expected: {:?}\n    predicted: {:?}\n    source: {:?} elapsed: {} ms",
                            entry.message,
                            entry.expected_intents,
                            predicted_order,
                            decomposition.source,
                            started.elapsed().as_millis()
                        );
                    }
                    if predicted_order == entry.expected_intents {
                        multi_ordered_correct += 1;
                    }
                }
                "zero" => {
                    zero_total += 1;
                    if predicted.is_empty() {
                        zero_correct += 1;
                    }
                }
                "ambiguous" => {
                    ambiguous_total += 1;
                    if decomposition.uncertain {
                        ambiguous_uncertain += 1;
                    }
                }
                _ => {}
            }
        }

        let single_accuracy = if single_total == 0 {
            0.0
        } else {
            single_correct as f32 / single_total as f32
        };
        let zero_accuracy = if zero_total == 0 {
            0.0
        } else {
            zero_correct as f32 / zero_total as f32
        };
        let hallucination_rate = if predicted_total == 0 {
            0.0
        } else {
            hallucinated as f32 / predicted_total as f32
        };
        let multi_accuracy = if multi_total == 0 {
            0.0
        } else {
            multi_correct as f32 / multi_total as f32
        };
        let multi_ordered_accuracy = if multi_total == 0 {
            0.0
        } else {
            multi_ordered_correct as f32 / multi_total as f32
        };
        let set_accuracy = if fixture.is_empty() {
            0.0
        } else {
            set_correct as f32 / fixture.len() as f32
        };
        let non_ambiguous_accuracy = if non_ambiguous_total == 0 {
            0.0
        } else {
            non_ambiguous_correct as f32 / non_ambiguous_total as f32
        };
        let ambiguous_uncertain_rate = if ambiguous_total == 0 {
            0.0
        } else {
            ambiguous_uncertain as f32 / ambiguous_total as f32
        };
        let recall_rate = if recall_total == 0 {
            0.0
        } else {
            recall_hit as f32 / recall_total as f32
        };

        println!("intent decomposition eval");
        println!(
            "  set accuracy (all): {set_correct}/{} ({set_accuracy:.2})",
            fixture.len()
        );
        println!(
            "  set accuracy (excluding ambiguous): {non_ambiguous_correct}/{non_ambiguous_total} ({non_ambiguous_accuracy:.2})"
        );
        println!(
            "  set accuracy (single): {single_correct}/{single_total} ({single_accuracy:.2})"
        );
        println!(
            "  set accuracy (multi): {multi_correct}/{multi_total} ({multi_accuracy:.2})"
        );
        println!(
            "  ordered accuracy (multi): {multi_ordered_correct}/{multi_total} ({multi_ordered_accuracy:.2})"
        );
        println!("  intent recall: {recall_hit}/{recall_total} ({recall_rate:.2})");
        println!("  zero-intent accuracy: {zero_correct}/{zero_total} ({zero_accuracy:.2})");
        println!(
            "  ambiguous uncertain: {ambiguous_uncertain}/{ambiguous_total} ({ambiguous_uncertain_rate:.2})"
        );
        println!("  hallucination rate: {hallucinated}/{predicted_total} ({hallucination_rate:.2})");
        println!("  llm calls: {llm_calls}");
        // 尾部延迟要先算：`eval_p50` 按值消耗 `latencies`，所以排序副本要提前取。
        let mut sorted = latencies.clone();
        sorted.sort_unstable();
        let p95 = sorted
            .get(((sorted.len() as f32 * 0.95) as usize).min(sorted.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        let max = sorted.last().copied().unwrap_or(0);
        println!("  p50 latency: {} ms", eval_p50(latencies));
        println!("  p95 latency: {p95} ms");
        println!("  max latency: {max} ms");

        assert!(single_accuracy >= 0.0);
        assert!(zero_accuracy >= 0.0);
    }
}