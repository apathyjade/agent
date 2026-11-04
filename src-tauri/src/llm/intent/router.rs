use std::collections::HashMap;
#[cfg(test)]
use std::collections::VecDeque;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Mutex;

use crate::core::db::models::IntentRecord;
use crate::core::db::repository::Database;
use crate::llm::intent::classifier::{score_intents, LlmClassifier};
use crate::llm::intent::{
    IntentConfig, IntentResult, PathSelection, RouteOutcome, RouteRequest, RouteSource,
};
use crate::llm::provider::ProviderRegistry;

/// Stable id reported when routing is disabled or cannot be performed.
const FALLBACK_INTENT_ID: &str = "builtin.software_dev.chat";

/// Domain attributed to the synthetic result. It names the builtin domain the
/// fallback intent belongs to, but the result's `domain_name` is deliberately
/// empty because no domain row was consulted.
const FALLBACK_DOMAIN_ID: &str = "builtin.software_dev";

/// A consistent snapshot of the routing inputs.
///
/// The DB guard is dropped before classification: holding it across the
/// classifier's `.await` would stall every other DB user for the duration of
/// an LLM round-trip.
struct RoutingData {
    domain_names: HashMap<String, String>,
    /// Enabled intents per domain, in the same `sort_order, name` order as
    /// `Database::list_enabled_intents`.
    by_domain: HashMap<String, Vec<IntentRecord>>,
    /// Every enabled intent across all domains, ordered by domain.
    all_intents: Vec<IntentRecord>,
    active_domain_id: Option<String>,
}

impl RoutingData {
    fn domain_name(&self, domain_id: &str) -> &str {
        self.domain_names
            .get(domain_id)
            .map(String::as_str)
            .unwrap_or("")
    }

    fn intents_in(&self, domain_id: &str) -> &[IntentRecord] {
        self.by_domain
            .get(domain_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn intent_domain(&self, intent_id: &str) -> Option<String> {
        self.all_intents
            .iter()
            .find(|intent| intent.id == intent_id)
            .map(|intent| intent.domain_id.clone())
    }
}

/// The intent router: resolves a user message to one intent with a cascade
/// (LLM → data-driven scorer → in-domain fallback) and reports how it got
/// there.
pub struct IntentRouter {
    db: Arc<Mutex<Database>>,
    classifier: LlmClassifier,
    /// Whether routing is enabled at all.
    enabled: bool,
    /// Number of times the LLM boundary was invoked. Test-only so the
    /// cascade's "never call the LLM twice after a failure" rule can be
    /// asserted honestly instead of inferred.
    #[cfg(test)]
    llm_calls: AtomicUsize,
    /// Scripted LLM replies consumed in order. Empty means "use the real
    /// classifier", which is what tests with an empty provider registry do.
    #[cfg(test)]
    llm_script: Mutex<VecDeque<Option<(String, f32)>>>,
}

impl IntentRouter {
    pub fn new(
        db: Arc<Mutex<Database>>,
        providers: Arc<Mutex<ProviderRegistry>>,
        classifier_model_id: Option<String>,
        enabled: bool,
    ) -> Self {
        Self {
            db,
            classifier: LlmClassifier::new(providers, classifier_model_id),
            enabled,
            #[cfg(test)]
            llm_calls: AtomicUsize::new(0),
            #[cfg(test)]
            llm_script: Mutex::new(VecDeque::new()),
        }
    }

    /// Compatibility wrapper: route with no prior domain and no pinning.
    pub async fn classify(&self, content: &str) -> IntentResult {
        self.route(RouteRequest {
            content,
            prior_domain: None,
            pinned: false,
        })
        .await
        .intent
    }

    /// Resolve one message to an intent.
    ///
    /// Cascade (design §6.2):
    /// 1. Classify inside the prior domain when it still has enabled intents.
    ///    A clear hit wins. A low-confidence hit widens when `auto`; a failed
    ///    LLM call falls through to the in-domain scorer and never asks the
    ///    LLM again.
    /// 2. `pinned` stops widening after a low score.
    /// 3. Widening classifies against every enabled intent in every domain.
    /// 4. Anything unresolved runs the in-domain fallback chain:
    ///    `fallback_intent` → `.clarify` → lowest `sort_order` enabled intent.
    /// 5. No usable domain at all yields the synthetic plain-chat result.
    ///
    /// Never fails: every degraded case returns a result so the message still
    /// gets answered.
    pub async fn route(&self, req: RouteRequest<'_>) -> RouteOutcome {
        if !self.enabled {
            return Self::synthetic_outcome();
        }

        let Some(data) = self.load_routing_data().await else {
            return Self::synthetic_outcome();
        };

        if data.all_intents.is_empty() {
            log::warn!(
                "intent routing: no enabled intents in any domain, falling back to plain chat"
            );
            return Self::synthetic_outcome();
        }

        let prior_domain_id = req.prior_domain.map(str::to_string);
        let prior_exists = prior_domain_id
            .as_deref()
            .map_or(false, |id| data.domain_names.contains_key(id));

        if req.pinned && prior_domain_id.is_some() && !prior_exists {
            log::warn!(
                "intent routing: pinned domain '{}' no longer exists, treating the session as auto",
                req.prior_domain.unwrap_or_default()
            );
        }

        // Pinning is only meaningful while the pinned domain still exists.
        let pinned = req.pinned && prior_exists;
        let mut llm_available = true;
        let mut low_candidate: Option<(String, f32)> = None;

        // ── Step 1: classify inside the prior domain ──────────────────────
        if let Some(domain_id) = prior_domain_id.as_deref().filter(|_| prior_exists) {
            let intents = data.intents_in(domain_id);

            if intents.is_empty() {
                if pinned {
                    log::warn!(
                        "intent routing: pinned domain '{domain_id}' has no enabled intents"
                    );
                    return Self::synthetic_outcome();
                }
            } else {
                match self.classify_llm(req.content, intents).await {
                    Some((id, score)) => {
                        if Self::clears_threshold(intents, &id, score) {
                            return Self::record_outcome(
                                &data,
                                &id,
                                score,
                                RouteSource::Llm,
                                false,
                            );
                        }
                        // Low confidence, not failure: widening is the next
                        // step and the scorer must not run.
                        low_candidate = Some((id, score));
                    }
                    None => {
                        // The classifier failed. Do not ask it again for the
                        // widened scope; only the scorer may answer now.
                        llm_available = false;
                        match score_intents(req.content, intents) {
                            Some((id, score))
                                if Self::clears_threshold(intents, &id, score) =>
                            {
                                return Self::record_outcome(
                                    &data,
                                    &id,
                                    score,
                                    RouteSource::Scorer,
                                    false,
                                );
                            }
                            Some(hit) => low_candidate = Some(hit),
                            None => {}
                        }
                    }
                }

                // ── Step 2: pinned never widens ──────────────────────────
                if pinned {
                    return Self::fallback_outcome(
                        &data,
                        domain_id,
                        low_candidate.as_ref(),
                        false,
                    );
                }
            }
        }

        // ── Step 3: widen across every enabled intent ────────────────────
        let widened = prior_domain_id.is_some();
        let widen_intents = data.all_intents.as_slice();
        let mut wide_low: Option<(String, f32)> = None;

        if llm_available {
            match self.classify_llm(req.content, widen_intents).await {
                Some((id, score)) => {
                    if Self::clears_threshold(widen_intents, &id, score) {
                        return Self::record_outcome(
                            &data,
                            &id,
                            score,
                            RouteSource::Llm,
                            widened,
                        );
                    }
                    low_candidate = Some((id, score));
                    wide_low = low_candidate.clone();
                }
                None => {
                    if let Some((id, score)) = score_intents(req.content, widen_intents) {
                        if Self::clears_threshold(widen_intents, &id, score) {
                            return Self::record_outcome(
                                &data,
                                &id,
                                score,
                                RouteSource::Scorer,
                                widened,
                            );
                        }
                        wide_low = Some((id, score));
                    }
                }
            }
        } else if let Some((id, score)) = score_intents(req.content, widen_intents) {
            if Self::clears_threshold(widen_intents, &id, score) {
                return Self::record_outcome(
                    &data,
                    &id,
                    score,
                    RouteSource::Scorer,
                    widened,
                );
            }
            wide_low = Some((id, score));
        }

        // ── Step 4: fallback chain in the last candidate domain ──────────
        let fallback_domain_id = wide_low
            .as_ref()
            .and_then(|(id, _)| data.intent_domain(id))
            .or_else(|| prior_domain_id.filter(|id| !data.intents_in(id).is_empty()))
            .or_else(|| data.active_domain_id.clone());

        let Some(domain_id) = fallback_domain_id else {
            log::warn!("intent routing: no candidate domain for the fallback chain");
            return Self::synthetic_outcome();
        };

        let candidate = wide_low.as_ref().or(low_candidate.as_ref());
        Self::fallback_outcome(&data, &domain_id, candidate, widened)
    }

    /// Resolve the final tool list from the session and intent restrictions.
    ///
    /// An explicit intent list is authoritative: it names the complete set of
    /// tools the intent permits, so an empty intersection means no tools at all
    /// (`Some(vec![])`), not "unrestricted". `None` on the intent side means the
    /// intent imposes no restriction.
    pub fn resolve_tools(
        &self,
        session_tools: Option<Vec<String>>,
        intent_tools: Option<&Vec<String>>,
    ) -> Option<Vec<String>> {
        match (session_tools, intent_tools) {
            (Some(session), Some(intent)) => Some(
                session
                    .into_iter()
                    .filter(|t| intent.contains(t))
                    .collect(),
            ),
            // No session-level restriction, but the intent still decides.
            (None, Some(intent)) => Some(intent.clone()),
            (Some(session), None) => Some(session),
            (None, None) => None,
        }
    }

    /// Invoke the LLM boundary, or consume a scripted reply in tests.
    ///
    /// Production behaviour is unchanged. The test hook counts calls and lets
    /// tests exercise the "LLM hit" branches without a network provider.
    async fn classify_llm(&self, content: &str, intents: &[IntentRecord]) -> Option<(String, f32)> {
        #[cfg(test)]
        {
            self.llm_calls.fetch_add(1, Ordering::SeqCst);
            let mut script = self.llm_script.lock().await;
            if let Some(scripted) = script.pop_front() {
                // Mirror the real parser: an id outside this scope is a miss.
                return scripted.filter(|(id, _)| intents.iter().any(|intent| &intent.id == id));
            }
        }
        self.classifier.classify(content, intents).await
    }

    async fn load_routing_data(&self) -> Option<RoutingData> {
        let (domains, intents) = {
            let db = self.db.lock().await;
            (db.list_intent_domains(), db.list_all_enabled_intents())
        };

        let domains = match domains {
            Ok(domains) => domains,
            Err(error) => {
                log::warn!(
                    "intent routing: failed to list intent domains: {error}, falling back to plain chat"
                );
                return None;
            }
        };
        let all_intents = match intents {
            Ok(intents) => intents,
            Err(error) => {
                log::warn!(
                    "intent routing: failed to list enabled intents: {error}, falling back to plain chat"
                );
                return None;
            }
        };

        let mut domain_names = HashMap::new();
        let mut active_domain_id = None;
        for domain in &domains {
            if domain.is_active {
                active_domain_id = Some(domain.id.clone());
            }
            domain_names.insert(domain.id.clone(), domain.name.clone());
        }

        let mut by_domain: HashMap<String, Vec<IntentRecord>> = HashMap::new();
        for intent in &all_intents {
            by_domain
                .entry(intent.domain_id.clone())
                .or_default()
                .push(intent.clone());
        }

        Some(RoutingData {
            domain_names,
            by_domain,
            all_intents,
            active_domain_id,
        })
    }

    fn clears_threshold(intents: &[IntentRecord], intent_id: &str, score: f32) -> bool {
        intents
            .iter()
            .find(|intent| intent.id == intent_id)
            // Compare in one domain: widening `0.7f32` to `f64` yields
            // 0.699999988…, which would wrongly fail a threshold of 0.7.
            .map(|intent| score >= intent.confidence_threshold as f32)
            .unwrap_or(false)
    }

    /// Walk the in-domain fallback chain: `fallback_intent` (one hop) →
    /// `.clarify` → lowest `sort_order` enabled intent.
    ///
    /// `low_candidate` is the low-confidence intent that triggered the
    /// fallback, when there is one. Its score is reported as the outcome's
    /// confidence (it explains why the reroute happened).
    fn fallback_outcome(
        data: &RoutingData,
        domain_id: &str,
        low_candidate: Option<&(String, f32)>,
        widened: bool,
    ) -> RouteOutcome {
        let intents = data.intents_in(domain_id);
        if intents.is_empty() {
            log::warn!(
                "intent routing: fallback domain '{domain_id}' has no enabled intents, falling back to plain chat"
            );
            return Self::synthetic_outcome();
        }

        let score = low_candidate
            .map(|(_, score)| *score)
            .unwrap_or(0.0);

        // 1. The low-confidence intent's configured reroute target, one hop.
        //    An empty or dangling `fallback_intent` is skipped rather than
        //    followed, so a self-referential chain cannot loop.
        if let Some((candidate_id, _)) = low_candidate {
            if let Some(candidate) = intents.iter().find(|intent| &intent.id == candidate_id) {
                if !candidate.fallback_intent.is_empty() {
                    if let Some(target) = intents
                        .iter()
                        .find(|intent| intent.id == candidate.fallback_intent)
                    {
                        return Self::record_outcome(
                            data,
                            &target.id,
                            score,
                            RouteSource::Fallback,
                            widened,
                        );
                    }
                }
            }
        }

        // 2. The domain's clarify intent.
        if let Some(clarify) = intents
            .iter()
            .find(|intent| intent.id.ends_with(".clarify"))
        {
            return Self::record_outcome(
                data,
                &clarify.id,
                score,
                RouteSource::Fallback,
                widened,
            );
        }

        // 3. Lowest `sort_order`; `intents` already carries query order.
        let first = &intents[0];
        Self::record_outcome(data, &first.id, score, RouteSource::Fallback, widened)
    }

    fn record_outcome(
        data: &RoutingData,
        intent_id: &str,
        confidence: f32,
        source: RouteSource,
        widened: bool,
    ) -> RouteOutcome {
        match data
            .all_intents
            .iter()
            .find(|intent| intent.id == intent_id)
        {
            Some(record) => RouteOutcome {
                intent: Self::project(
                    record,
                    data.domain_name(&record.domain_id),
                    confidence,
                ),
                source,
                widened,
            },
            // Every id passed here came from `all_intents`; defensive only.
            None => Self::synthetic_outcome(),
        }
    }

    /// Synthetic plain-chat result: fast path, no tool restriction, no
    /// iteration cap, and no domain choice.
    fn synthetic_outcome() -> RouteOutcome {
        RouteOutcome {
            intent: Self::fallback(),
            source: RouteSource::Fallback,
            widened: false,
        }
    }

    fn fallback() -> IntentResult {
        IntentResult {
            id: FALLBACK_INTENT_ID.to_string(),
            // Matches the seeded `builtin.software_dev.chat` display name so
            // the synthetic and DB-backed chat paths log the same label.
            name: "闲聊对话".to_string(),
            // The synthetic result reuses the builtin domain id but reports
            // an empty domain name: no domain row was chosen. Callers that
            // must distinguish this case should check `RouteOutcome::source`
            // or the empty name.
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

    /// Project a stored intent row into the router's result type.
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
                // Never collapse an empty list to `None`: `None` means "any
                // tool", but an empty `allowed_tools` means "no tool" (chat,
                // clarify). The row is the only source of this list.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::IntentDomainRecord;

    const TS: &str = "2026-11-04T00:00:00Z";
    /// An example that `score_intents` can match by substring.
    const MATCHING_MESSAGE: &str = "把这段改成异步";

    fn test_db() -> Arc<Mutex<Database>> {
        Arc::new(Mutex::new(Database::new_test().expect("in-memory db")))
    }

    /// Empty registry: provider resolution fails, so every test exercises the
    /// data-driven fallback chain rather than a network call.
    fn empty_providers() -> Arc<Mutex<ProviderRegistry>> {
        Arc::new(Mutex::new(ProviderRegistry::new(&[])))
    }

    fn router(db: Arc<Mutex<Database>>, enabled: bool) -> IntentRouter {
        IntentRouter::new(db, empty_providers(), None, enabled)
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
            examples: vec![MATCHING_MESSAGE.to_string()],
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

    /// Insert a domain plus its intents. The domain stays inactive unless the
    /// caller activates it.
    fn seed_domain(db: &Database, domain_id: &str, intents: &[IntentRecord]) {
        db.insert_intent_domain(&mk_domain(domain_id))
            .expect("insert domain");
        for intent in intents {
            db.insert_intent(intent).expect("insert intent");
        }
    }

    /// Insert a domain plus one enabled intent and make the domain active.
    fn seed_active_domain(db: &Database, intent: &IntentRecord) {
        seed_domain(db, &intent.domain_id, &[intent.clone()]);
        db.set_active_intent_domain(&intent.domain_id)
            .expect("activate domain");
    }

    #[tokio::test]
    async fn disabled_router_returns_fast_path() {
        let db = test_db();
        // A deep intent is stored and active: if the router read the DB it
        // would route deep. Disabled must win before any query.
        {
            let guard = db.lock().await;
            seed_active_domain(&guard, &mk_intent("test.deep", "test.domain", "deep"));
        }

        let result = router(db, false).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "builtin.software_dev.chat");
        assert_eq!(result.name, "闲聊对话");
        assert_eq!(result.domain_id, "builtin.software_dev");
        assert!(result.domain_name.is_empty());
        assert_eq!(result.path, PathSelection::Fast);
        assert!(result.config.enabled_tools.is_none());
        assert!(result.config.max_iterations.is_none());
        assert!(!result.config.auto_escalate);
    }

    #[tokio::test]
    async fn inactive_domain_with_enabled_intents_is_routable_via_widening() {
        let db = test_db();
        // `is_active` only marks the default domain for new sessions; it must
        // not remove a domain from the widening candidate set.
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.inactive",
                &[mk_intent("test.inactive.intent", "test.inactive", "deep")],
            );
        }

        let outcome = router(db, true)
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: None,
                pinned: false,
            })
            .await;

        assert_eq!(outcome.intent.id, "test.inactive.intent");
        assert_eq!(outcome.intent.domain_id, "test.inactive");
        assert_eq!(outcome.source, RouteSource::Scorer);
        assert!(!outcome.widened);
    }

    #[tokio::test]
    async fn no_active_domain_and_no_enabled_intents_returns_synthetic() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.inactive",
                &[mk_intent("test.inactive.intent", "test.inactive", "deep")],
            );
            // Bypass the "cannot disable the last enabled intent" guard to
            // reproduce a database with no active domain and no enabled intents.
            guard
                .conn
                .execute("UPDATE intents SET enabled = 0", [])
                .expect("disable intents");
        }

        let router = router(db, true);
        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: None,
                pinned: false,
            })
            .await;

        assert_eq!(outcome.intent.id, "builtin.software_dev.chat");
        assert_eq!(outcome.source, RouteSource::Fallback);
        assert_eq!(outcome.intent.domain_id, "builtin.software_dev");
        assert!(outcome.intent.domain_name.is_empty());
        assert!(!outcome.widened);
        assert_eq!(router.llm_call_count(), 0, "no LLM request may be sent");
    }

    #[tokio::test]
    async fn domain_with_zero_enabled_intents_returns_fast_path() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_active_domain(&guard, &mk_intent("test.disabled", "test.domain", "deep"));
            // Bypass the "cannot disable the last enabled intent" guard to
            // reproduce a domain left without enabled intents.
            guard
                .conn
                .execute("UPDATE intents SET enabled = 0", [])
                .expect("disable intents");
        }

        let router = router(db, true);
        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("test.domain"),
                pinned: false,
            })
            .await;

        assert_eq!(outcome.intent.id, "builtin.software_dev.chat");
        assert_eq!(outcome.source, RouteSource::Fallback);
        assert!(outcome.intent.config.max_iterations.is_none());
        assert_eq!(router.llm_call_count(), 0);
    }

    #[tokio::test]
    async fn path_comes_from_the_intent_row() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_active_domain(&guard, &mk_intent("test.deep", "test.domain", "deep"));
        }

        let result = router(db, true).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "test.deep");
        assert_eq!(result.path, PathSelection::Deep);
    }

    #[tokio::test]
    async fn config_projects_row_fields() {
        let db = test_db();
        {
            let guard = db.lock().await;
            let mut intent = mk_intent("test.configured", "test.domain", "fast");
            intent.name = "配置意图".to_string();
            intent.allowed_tools = vec!["read_file".to_string(), "code_executor".to_string()];
            intent.max_iterations = Some(7);
            intent.model_id = "gpt-4o-mini".to_string();
            intent.auto_escalate = true;
            intent.system_prompt_appendix = "be strict".to_string();
            seed_active_domain(&guard, &intent);
        }

        let result = router(db, true).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "test.configured");
        assert_eq!(result.name, "配置意图");
        assert_eq!(
            result.config.enabled_tools,
            Some(vec!["read_file".to_string(), "code_executor".to_string()])
        );
        assert_eq!(result.config.max_iterations, Some(7));
        assert_eq!(result.config.model_id.as_deref(), Some("gpt-4o-mini"));
        assert!(result.config.auto_escalate);
        assert_eq!(
            result.config.system_prompt_appendix.as_deref(),
            Some("be strict")
        );
        assert_eq!(result.path, PathSelection::Fast);
    }

    #[test]
    fn resolve_tools_intersects_session_and_intent_tools() {
        let router = router(test_db(), true);
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
    fn resolve_tools_without_intent_restriction_keeps_session_tools() {
        let router = router(test_db(), true);
        let session = Some(vec!["calculator".to_string(), "web_search".to_string()]);

        assert_eq!(
            router.resolve_tools(session, None),
            Some(vec!["calculator".to_string(), "web_search".to_string()])
        );
    }

    #[tokio::test]
    async fn chat_intent_projects_an_empty_tool_list() {
        let db = test_db();
        {
            let guard = db.lock().await;
            // The seeded `chat` / `clarify` intents declare `allowed_tools: []`,
            // which means "no tools" — not "unrestricted".
            let mut intent = mk_intent("builtin.software_dev.chat", "test.domain", "fast");
            intent.allowed_tools = Vec::new();
            seed_active_domain(&guard, &intent);
        }

        let result = router(db, true).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "builtin.software_dev.chat");
        assert_eq!(result.config.enabled_tools, Some(Vec::new()));
    }

    #[test]
    fn empty_intent_tool_list_permits_no_tools() {
        let router = router(test_db(), true);
        let session = Some(vec!["file_system".to_string(), "web_search".to_string()]);
        let intent: Vec<String> = Vec::new();

        // An empty intersection with an explicit intent list must not degrade
        // to `None`, because `None` means "every tool is allowed".
        assert_eq!(router.resolve_tools(session, Some(&intent)), Some(Vec::new()));
    }

    #[test]
    fn intent_tool_list_is_authoritative_without_session_list() {
        let router = router(test_db(), true);
        let intent: Vec<String> = Vec::new();

        // A session without its own list (the default) must not erase the
        // intent's restriction; otherwise chat could still call tools.
        assert_eq!(router.resolve_tools(None, Some(&intent)), Some(Vec::new()));
    }

    #[tokio::test]
    async fn reroute_projects_the_fallback_intents_record() {
        let db = test_db();
        let mut alpha = mk_intent("test.alpha", "test.domain", "fast");
        alpha.name = "alpha".to_string();
        alpha.description = "实现功能".to_string();
        alpha.examples = Vec::new();
        alpha.allowed_tools = vec!["read_file".to_string()];
        alpha.confidence_threshold = 0.9;
        alpha.fallback_intent = "test.gamma".to_string();
        alpha.sort_order = 0;

        let mut beta = mk_intent("test.beta", "test.domain", "fast");
        beta.name = "beta".to_string();
        beta.description = "解释原理".to_string();
        beta.examples = Vec::new();
        beta.allowed_tools = vec!["web_search".to_string()];
        beta.confidence_threshold = 0.9;
        beta.fallback_intent = "test.alpha".to_string();
        beta.sort_order = 10;

        let mut gamma = mk_intent("test.gamma", "test.domain", "deep");
        gamma.name = "gamma".to_string();
        gamma.description = "闲聊对话".to_string();
        gamma.examples = Vec::new();
        gamma.allowed_tools = vec!["file_system".to_string(), "calculator".to_string()];
        gamma.confidence_threshold = 0.5;
        gamma.fallback_intent = String::new();
        gamma.sort_order = 20;

        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[alpha, beta, gamma]);
            guard
                .set_active_intent_domain("test.domain")
                .expect("activate domain");
        }

        let outcome = router(db, true)
            .route(RouteRequest {
                content: "实现功能代码",
                prior_domain: None,
                pinned: false,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Fallback);
        assert_eq!(outcome.intent.id, "test.gamma");
        assert_eq!(
            outcome.intent.path,
            PathSelection::Deep,
            "path must come from the reroute target's row, not from alpha or first()"
        );
        assert_eq!(
            outcome.intent.config.enabled_tools,
            Some(vec!["file_system".to_string(), "calculator".to_string()]),
            "tool list must come from the reroute target's row"
        );
        assert!(
            (outcome.intent.confidence - 0.3).abs() < 0.2,
            "pre-reroute score"
        );
    }

    // ── Cascade tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn in_domain_llm_hit_uses_llm_without_widening() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.domain",
                &[mk_intent("test.implement", "test.domain", "fast")],
            );
        }

        let router = router(db, true);
        router
            .set_llm_script(vec![Some(("test.implement".to_string(), 0.9))])
            .await;

        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("test.domain"),
                pinned: false,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Llm);
        assert!(!outcome.widened);
        assert_eq!(outcome.intent.id, "test.implement");
        assert_eq!(outcome.intent.domain_id, "test.domain");
        assert_eq!(outcome.intent.domain_name, "test.domain");
        assert_eq!(router.llm_call_count(), 1, "in-domain hit is one call");
    }

    #[tokio::test]
    async fn low_confidence_in_domain_widens_to_another_domain() {
        let db = test_db();
        let mut low = mk_intent("a.implement", "a.domain", "fast");
        low.confidence_threshold = 0.9;
        let high = mk_intent("b.explain", "b.domain", "fast");

        {
            let guard = db.lock().await;
            seed_domain(&guard, "a.domain", &[low]);
            seed_domain(&guard, "b.domain", &[high]);
        }

        let router = router(db, true);
        router
            .set_llm_script(vec![
                Some(("a.implement".to_string(), 0.2)),
                Some(("b.explain".to_string(), 0.9)),
            ])
            .await;

        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("a.domain"),
                pinned: false,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Llm);
        assert!(outcome.widened, "auto must widen after a low in-domain score");
        assert_eq!(outcome.intent.id, "b.explain");
        assert_eq!(outcome.intent.domain_id, "b.domain");
        assert_eq!(outcome.intent.domain_name, "b.domain");
        assert_eq!(router.llm_call_count(), 2, "one in-domain + one widening call");
    }

    #[tokio::test]
    async fn pinned_low_confidence_stays_in_domain_without_widening() {
        let db = test_db();
        let mut implement = mk_intent("test.implement", "test.domain", "fast");
        implement.confidence_threshold = 0.9;
        implement.fallback_intent = "test.domain.clarify".to_string();

        let mut clarify = mk_intent("test.domain.clarify", "test.domain", "fast");
        clarify.examples = Vec::new();
        clarify.sort_order = 10;

        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[implement, clarify]);
        }

        let router = router(db, true);
        router
            .set_llm_script(vec![Some(("test.implement".to_string(), 0.2))])
            .await;

        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("test.domain"),
                pinned: true,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Fallback);
        assert!(!outcome.widened, "pinned must not widen");
        assert_eq!(outcome.intent.id, "test.domain.clarify");
        assert_eq!(outcome.intent.domain_id, "test.domain");
        assert!(
            (outcome.intent.confidence - 0.2).abs() < 1e-6,
            "the low score explains the reroute"
        );
        assert_eq!(router.llm_call_count(), 1, "no widening call may be made");
    }

    #[tokio::test]
    async fn pinned_fallback_chain_ends_at_lowest_sort_order() {
        let db = test_db();
        let mut only = mk_intent("test.only", "test.domain", "fast");
        only.confidence_threshold = 0.9;
        only.examples = Vec::new();
        only.description = "唯一意图".to_string();

        {
            let guard = db.lock().await;
            seed_domain(&guard, "test.domain", &[only]);
        }

        let router = router(db, true);
        router
            .set_llm_script(vec![Some(("test.only".to_string(), 0.1))])
            .await;

        let outcome = router
            .route(RouteRequest {
                content: "随便说说",
                prior_domain: Some("test.domain"),
                pinned: true,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Fallback);
        assert!(!outcome.widened);
        assert_eq!(outcome.intent.id, "test.only");
        assert_eq!(router.llm_call_count(), 1);
    }

    #[tokio::test]
    async fn llm_failure_uses_scorer_and_never_retries_the_llm() {
        let db = test_db();
        let mut other = mk_intent("a.other", "a.domain", "fast");
        other.examples = Vec::new();
        other.description = "无关内容".to_string();

        let matching = mk_intent("b.implement", "b.domain", "fast");

        {
            let guard = db.lock().await;
            seed_domain(&guard, "a.domain", &[other]);
            seed_domain(&guard, "b.domain", &[matching]);
        }

        // Empty registry, empty script: the LLM boundary returns `None`.
        let router = router(db, true);

        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("a.domain"),
                pinned: false,
            })
            .await;

        assert_eq!(
            outcome.source,
            RouteSource::Scorer,
            "a failed LLM call must hand off to the scorer"
        );
        assert_eq!(outcome.intent.id, "b.implement");
        assert_eq!(outcome.intent.domain_id, "b.domain");
        assert!(outcome.widened);
        assert_eq!(
            router.llm_call_count(),
            1,
            "failure must not trigger a second LLM request, even when widening"
        );
    }

    #[tokio::test]
    async fn no_enabled_intents_returns_synthetic_without_a_domain() {
        let router = router(test_db(), true);

        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: None,
                pinned: false,
            })
            .await;

        assert_eq!(outcome.intent.id, "builtin.software_dev.chat");
        assert_eq!(outcome.source, RouteSource::Fallback);
        assert_eq!(outcome.intent.domain_id, "builtin.software_dev");
        assert!(
            outcome.intent.domain_name.is_empty(),
            "the synthetic result must not pretend a domain was chosen"
        );
        assert!(!outcome.widened);
        assert_eq!(router.llm_call_count(), 0);
    }

    #[tokio::test]
    async fn deleted_pinned_domain_degrades_to_auto() {
        let db = test_db();
        {
            let guard = db.lock().await;
            seed_domain(
                &guard,
                "test.live",
                &[mk_intent("test.live.intent", "test.live", "fast")],
            );
        }

        let router = router(db, true);
        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("test.deleted"),
                pinned: true,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Scorer);
        assert_eq!(outcome.intent.id, "test.live.intent");
        assert_eq!(outcome.intent.domain_id, "test.live");
        assert!(outcome.widened, "a deleted pinned domain falls back to auto");
        assert_eq!(router.llm_call_count(), 1);
    }

    #[tokio::test]
    async fn deprioritized_prior_domain_widens_to_another_domain() {
        let db = test_db();
        {
            let guard = db.lock().await;
            // Exists but has no enabled intents any more.
            seed_domain(&guard, "test.empty", &[]);
            seed_domain(
                &guard,
                "test.live",
                &[mk_intent("test.live.intent", "test.live", "fast")],
            );
        }

        let router = router(db, true);
        let outcome = router
            .route(RouteRequest {
                content: MATCHING_MESSAGE,
                prior_domain: Some("test.empty"),
                pinned: false,
            })
            .await;

        assert_eq!(outcome.source, RouteSource::Scorer);
        assert_eq!(outcome.intent.id, "test.live.intent");
        assert_eq!(outcome.intent.domain_id, "test.live");
        assert!(outcome.widened);
        assert_eq!(router.llm_call_count(), 1);
    }

    impl IntentRouter {
        #[cfg(test)]
        async fn set_llm_script(&self, responses: Vec<Option<(String, f32)>>) {
            let mut script = self.llm_script.lock().await;
            script.clear();
            script.extend(responses);
        }

        #[cfg(test)]
        fn llm_call_count(&self) -> usize {
            self.llm_calls.load(Ordering::SeqCst)
        }
    }
}
