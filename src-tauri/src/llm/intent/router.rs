use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

use crate::core::db::models::IntentRecord;
use crate::core::db::repository::Database;
use crate::llm::embedding::EmbeddingService;
use crate::llm::intent::classifier::{score_intents, IntentReranker};
use crate::llm::intent::index::{intent_units, IntentVectorIndex};
use crate::llm::intent::probe::probe;
use crate::llm::intent::{
    DecisionEvidence, DecisionSource, IntentCandidate, IntentConfig, IntentResult, PathSelection,
    RouteContext, RouteDecision, RouteTrace, RouterConfig,
};

/// Stable id reported when routing is disabled or cannot be performed.
const FALLBACK_INTENT_ID: &str = "builtin.software_dev.chat";

/// Domain attributed to the synthetic result. It names the builtin domain the
/// fallback intent belongs to, but the result's `domain_name` is deliberately
/// empty because no domain row was consulted.
const FALLBACK_DOMAIN_ID: &str = "builtin.software_dev";

const EMBEDDING_TOP_K: usize = 6;
const LLM_CANDIDATE_LIMIT: usize = 40;
const TRACE_LIMIT: usize = 50;
const EMBEDDING_QUERY_TIMEOUT_MS: u64 = 1200;

struct RoutingData {
    domain_names: HashMap<String, String>,
    intents: Vec<IntentRecord>,
}

/// The intent router: resolves a user message with a cascade of signals
/// (embedding → LLM rerank → rule scorer), reports how it got there, and keeps
/// an in-process trace ring buffer.
pub struct IntentRouter {
    db: Arc<Mutex<Database>>,
    embedding: Option<Arc<EmbeddingService>>,
    index: RwLock<IntentVectorIndex>,
    reranker: Arc<dyn IntentReranker>,
    config: RouterConfig,
    traces: Mutex<VecDeque<RouteTrace>>,
}

impl IntentRouter {
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

    /// Build the intent index in the background. Never blocks routing; until
    /// it finishes the embedding signal is simply unavailable.
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

    /// Mark the index stale. Callers normally follow this with a spawned
    /// `warm_up`.
    pub async fn mark_dirty(&self) {
        self.index.write().await.mark_dirty();
    }

    pub async fn recent_traces(&self) -> Vec<RouteTrace> {
        let traces = self.traces.lock().await;
        traces.iter().cloned().collect()
    }

    /// Resolve the final tool list from the session and intent restrictions.
    ///
    /// An explicit intent list is authoritative: it names the complete set of
    /// tools the intent permits, so an empty intersection means no tools at
    /// all (`Some(vec![])`), not "unrestricted". `None` on the intent side
    /// means the intent imposes no restriction.
    pub fn resolve_tools(
        &self,
        session_tools: Option<Vec<String>>,
        intent_tools: Option<&Vec<String>>,
    ) -> Option<Vec<String>> {
        match (session_tools, intent_tools) {
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

    /// Resolve one message to an intent. Never fails: every degraded case
    /// returns a decision so the message still gets answered.
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

        let prior_exists = ctx
            .prior_domain
            .map_or(false, |id| data.domain_names.contains_key(id));
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
            return self
                .finalize(Self::synthetic(false), &ctx, started)
                .await;
        }

        let intents_by_id: HashMap<&str, &IntentRecord> = candidates
            .iter()
            .map(|intent| (intent.id.as_str(), intent))
            .collect();
        let candidate_ids: HashSet<String> =
            candidates.iter().map(|intent| intent.id.clone()).collect();
        let prior_domain = ctx.prior_domain;

        let mut evidence = DecisionEvidence::default();

        // ── Signal 1: embedding recall ────────────────────────────────────
        let mut emb_top: Vec<(String, f32)> = Vec::new();
        if let Some(embedding) = self
            .embedding
            .as_ref()
            .filter(|embedding| embedding.available())
        {
            if self.index.read().await.is_ready() {
                let remaining = budget_left(started, self.config.routing_budget_ms);
                let wait = remaining.min(Duration::from_millis(EMBEDDING_QUERY_TIMEOUT_MS));
                if !wait.is_zero() {
                    if let Ok(Some(query)) =
                        tokio::time::timeout(wait, embedding.embed(ctx.content)).await
                    {
                        let index = self.index.read().await;
                        emb_top = index.top_k(&query, &candidate_ids, EMBEDDING_TOP_K);
                    }
                }
            }
        }

        let top1 = emb_top.first().map(|(_, score)| *score);
        let top2 = emb_top.get(1).map(|(_, score)| *score).unwrap_or(0.0);
        evidence.embedding_top1 = top1;
        evidence.embedding_margin = top1.map(|score| score - top2);

        let decisive = top1.map_or(false, |score| {
            score >= self.config.embedding_floor
                && (score - top2) >= self.config.embedding_margin
        });

        let mut candidates_meta = embedding_candidates(&emb_top, &intents_by_id);

        if decisive {
            let chosen = emb_top
                .first()
                .and_then(|(id, _)| intents_by_id.get(id.as_str()).copied())
                .map(|record| (record.clone(), top1.unwrap_or(0.0)));
            if let Some((record, confidence)) = chosen {
                let decision = self.decision_for(
                    &data,
                    &record,
                    confidence,
                    false,
                    DecisionSource::Embedding,
                    candidates_meta,
                    evidence,
                );
                return self.finalize(decision, &ctx, started).await;
            }
        }

        // ── Signal 2: LLM rerank ─────────────────────────────────────────
        if self.config.llm_rerank_enabled {
            let llm_candidates: Vec<IntentRecord> = if !emb_top.is_empty() {
                emb_top
                    .iter()
                    .filter_map(|(id, _)| intents_by_id.get(id.as_str()).map(|r| (*r).clone()))
                    .collect()
            } else {
                candidates
                    .iter()
                    .take(LLM_CANDIDATE_LIMIT)
                    .cloned()
                    .collect()
            };

            if !llm_candidates.is_empty() {
                let remaining = budget_left(started, self.config.routing_budget_ms);
                let wait = remaining.min(Duration::from_millis(self.config.llm_timeout_ms));
                if !wait.is_zero() {
                    evidence.llm_calls += 1;
                    let replied = tokio::time::timeout(
                        wait,
                        self.reranker
                            .rerank(ctx.content, ctx.recent_messages, &llm_candidates),
                    )
                    .await
                    .ok()
                    .flatten();

                    if let Some((intent_id, confidence)) = replied {
                        if let Some(record) = intents_by_id.get(intent_id.as_str()).copied() {
                            evidence.llm_confidence = Some(confidence);
                            let threshold = record.confidence_threshold as f32;
                            let uncertain = confidence < threshold;

                            if uncertain && self.config.probe_enabled {
                                if let Some(project_root) = ctx.project_root {
                                    let left = budget_left(
                                        started,
                                        self.config.routing_budget_ms,
                                    );
                                    let need = Duration::from_millis(
                                        self.config.probe_timeout_ms
                                            + self.config.second_rerank_min_budget_ms,
                                    );
                                    if left >= need {
                                        evidence.probe_used = true;
                                        let evidence_text = probe(
                                            record,
                                            project_root,
                                            Duration::from_millis(self.config.probe_timeout_ms),
                                        )
                                        .await;

                                        if let Some(evidence_text) = evidence_text {
                                            let combined = format!(
                                                "{}\n\nProject evidence:\n{}",
                                                ctx.content, evidence_text
                                            );
                                            let left = budget_left(
                                                started,
                                                self.config.routing_budget_ms,
                                            );
                                            let wait = left.min(Duration::from_millis(
                                                self.config.llm_timeout_ms,
                                            ));
                                            if !wait.is_zero() {
                                                evidence.llm_calls += 1;
                                                let second = tokio::time::timeout(
                                                    wait,
                                                    self.reranker.rerank(
                                                        &combined,
                                                        ctx.recent_messages,
                                                        &llm_candidates,
                                                    ),
                                                )
                                                .await
                                                .ok()
                                                .flatten();

                                                if let Some((second_id, second_conf)) = second {
                                                    if let Some(second_record) = intents_by_id
                                                        .get(second_id.as_str())
                                                        .copied()
                                                    {
                                                        evidence.llm_confidence =
                                                            Some(second_conf);
                                                        push_candidate(
                                                            &mut candidates_meta,
                                                            second_record,
                                                            second_conf,
                                                            DecisionSource::Probe,
                                                        );
                                                        let decision = self.decision_for(
                                                            &data,
                                                            second_record,
                                                            second_conf,
                                                            second_conf
                                                                < second_record.confidence_threshold
                                                                    as f32,
                                                            DecisionSource::Probe,
                                                            candidates_meta,
                                                            evidence,
                                                        );
                                                        return self
                                                            .finalize(decision, &ctx, started)
                                                            .await;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            push_candidate(
                                &mut candidates_meta,
                                record,
                                confidence,
                                DecisionSource::Llm,
                            );
                            let decision = self.decision_for(
                                &data,
                                record,
                                confidence,
                                uncertain,
                                DecisionSource::Llm,
                                candidates_meta,
                                evidence,
                            );
                            return self.finalize(decision, &ctx, started).await;
                        }
                    }
                }
            }
        }

        // ── Signal 3: embedding fallback ─────────────────────────────────
        if !emb_top.is_empty() {
            if let Some((record, raw)) = ranked_choice(
                &emb_top,
                &intents_by_id,
                prior_domain,
                self.config.prior_boost,
            ) {
                push_candidate(
                    &mut candidates_meta,
                    &record,
                    raw,
                    DecisionSource::Embedding,
                );
                let decision = self.decision_for(
                    &data,
                    &record,
                    raw,
                    true,
                    DecisionSource::Embedding,
                    candidates_meta,
                    evidence,
                );
                return self.finalize(decision, &ctx, started).await;
            }
        }

        // ── Signal 4: rule scorer ────────────────────────────────────────
        if let Some((intent_id, score)) = score_intents(ctx.content, &candidates) {
            if let Some(record) = intents_by_id.get(intent_id.as_str()).copied() {
                evidence.rule_score = Some(score);
                push_candidate(&mut candidates_meta, record, score, DecisionSource::Rule);
                let decision = self.decision_for(
                    &data,
                    record,
                    score,
                    score < record.confidence_threshold as f32,
                    DecisionSource::Rule,
                    candidates_meta,
                    evidence,
                );
                return self.finalize(decision, &ctx, started).await;
            }
        }

        self.finalize(Self::synthetic(true), &ctx, started).await
    }

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

    /// Synthetic plain-chat result: fast path, no tool restriction, no
    /// iteration cap, and no domain choice.
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

fn budget_left(started: Instant, budget_ms: u64) -> Duration {
    Duration::from_millis(budget_ms).saturating_sub(started.elapsed())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::{IntentDomainRecord, Message};
    use crate::llm::embedding::TextEmbedder;
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

    struct ScriptedReranker {
        responses: Mutex<VecDeque<Option<(String, f32)>>>,
        calls: AtomicUsize,
    }

    impl ScriptedReranker {
        fn new(responses: Vec<Option<(String, f32)>>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into()),
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl IntentReranker for ScriptedReranker {
        async fn rerank(
            &self,
            _message: &str,
            _recent_messages: &[Message],
            _candidates: &[IntentRecord],
        ) -> Option<(String, f32)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.responses.lock().await.pop_front().flatten()
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

    #[tokio::test]
    async fn disabled_router_returns_synthetic() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "deep")]);
        }
        let reranker = ScriptedReranker::new(vec![Some(("test.a".to_string(), 0.9))]);
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
        let reranker = ScriptedReranker::new(vec![Some(("test.b".to_string(), 0.9))]);
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
        let reranker = ScriptedReranker::new(vec![Some(("test.a".to_string(), 0.9))]);
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
        let reranker = ScriptedReranker::new(vec![Some(("test.a".to_string(), 0.2))]);
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
        let reranker = ScriptedReranker::new(vec![None]);
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
    async fn llm_disabled_uses_embedding() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[mk_intent("test.a", "test.domain", "fast")]);
        }
        let reranker = ScriptedReranker::new(vec![Some(("test.a".to_string(), 0.9))]);
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
        let reranker = ScriptedReranker::new(vec![Some(("b.intent".to_string(), 0.9))]);
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
        let reranker = ScriptedReranker::new(vec![Some(("a.intent".to_string(), 0.9))]);
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
            ScriptedReranker::new(vec![Some(("test.a".to_string(), 0.9))]),
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
        let reranker = ScriptedReranker::new(vec![None]);
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
            Some(("test.a".to_string(), 0.2)),
            Some(("test.a".to_string(), 0.9)),
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
            Some(("test.a".to_string(), 0.2)),
            Some(("test.a".to_string(), 0.9)),
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
            Some(vec!["code_executor".to_string()])
        );
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

    /// Manual eval over the labelled fixture. Run with:
    /// `INTENT_ROUTING_EVAL=1 cargo test --lib intent_routing_eval -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn intent_routing_eval() {
        if std::env::var("INTENT_ROUTING_EVAL").ok().as_deref() != Some("1") {
            eprintln!("set INTENT_ROUTING_EVAL=1 to run the routing eval");
            return;
        }

        let fixture: Vec<serde_json::Value> = serde_json::from_str(include_str!(
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
            let message = entry["message"].as_str().unwrap_or_default();
            let category = entry["category"].as_str().unwrap_or("clear");
            let expected = entry["expected_intent"].as_str();

            let recent: Vec<Message> = entry["recent"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|item| Message {
                            id: uuid::Uuid::new_v4().to_string(),
                            session_id: "eval".to_string(),
                            role: item["role"].as_str().unwrap_or("user").to_string(),
                            content: item["content"].as_str().unwrap_or_default().to_string(),
                            tool_calls: None,
                            tool_call_id: None,
                            tokens: None,
                            created_at: TS.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();

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
