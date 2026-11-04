use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::IntentRecord;
use crate::core::db::repository::Database;
use crate::llm::intent::classifier::{apply_confidence, score_intents, LlmClassifier};
use crate::llm::intent::{IntentConfig, IntentResult, PathSelection};
use crate::llm::provider::ProviderRegistry;

/// Stable id reported when routing is disabled or cannot be performed.
const FALLBACK_INTENT_ID: &str = "builtin.software_dev.chat";

/// The intent router: classifies a user message against the enabled intents of
/// the active intent domain, then projects the winning row into [`IntentResult`].
pub struct IntentRouter {
    db: Arc<Mutex<Database>>,
    classifier: LlmClassifier,
    /// Whether routing is enabled at all.
    enabled: bool,
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
        }
    }

    /// Classify a user message into an intent. Never fails: every degraded case
    /// returns the same "plain chat" shape so the message still gets answered.
    pub async fn classify(&self, content: &str) -> IntentResult {
        if !self.enabled {
            return Self::fallback();
        }

        // Read the domain and intents into an owned `Vec`, then drop the guard.
        // Holding the DB guard across the classifier's `.await` would stall
        // every other DB user for the duration of an LLM round-trip.
        let intents = {
            let db = self.db.lock().await;
            match db.get_active_intent_domain() {
                Ok(Some(domain)) => match db.list_enabled_intents(&domain.id) {
                    Ok(intents) if !intents.is_empty() => Some(intents),
                    Ok(_) => {
                        log::warn!(
                            "intent routing: active domain '{}' has no enabled intents, falling back to plain chat",
                            domain.id
                        );
                        None
                    }
                    Err(error) => {
                        log::warn!(
                            "intent routing: failed to list intents for domain '{}': {error}, falling back to plain chat",
                            domain.id
                        );
                        None
                    }
                },
                Ok(None) => {
                    log::warn!(
                        "intent routing: no active intent domain, falling back to plain chat"
                    );
                    None
                }
                Err(error) => {
                    log::warn!(
                        "intent routing: failed to read the active intent domain: {error}, falling back to plain chat"
                    );
                    None
                }
            }
        };

        let Some(intents) = intents else {
            return Self::fallback();
        };

        // LLM first, then the data-driven scorer, then `clarify` (else the
        // first enabled intent). The classifier returns `None` on every
        // failure, so this chain always resolves to an id from `intents`.
        //
        // No-match case: route to `clarify` when the domain has one, else to
        // the first enabled intent, scoring `0.0` because nothing matched.
        let (chosen, score) = match self.classifier.classify(content, &intents).await {
            Some(hit) => hit,
            None => match score_intents(content, &intents) {
                Some(hit) => hit,
                None => {
                    // `intents` is non-empty (enforced by the guard above).
                    let record = intents
                        .iter()
                        .find(|intent| intent.id.ends_with(".clarify"))
                        .unwrap_or(&intents[0]);
                    (record.id.clone(), 0.0)
                }
            },
        };

        let intent_id = apply_confidence(&intents, &chosen, score);
        // `apply_confidence` can only return an id from `intents`: it either
        // echoes `chosen` (which came from `intents`) or names another row in
        // the same slice. Index 0 keeps the lookup total without inventing a
        // second degraded path.
        let record = intents
            .iter()
            .find(|intent| intent.id == intent_id)
            .unwrap_or(&intents[0]);

        Self::project(record, score)
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

    /// Plain-chat result: fast path, no tool restriction, no iteration cap.
    fn fallback() -> IntentResult {
        IntentResult {
            id: FALLBACK_INTENT_ID.to_string(),
            // Matches the seeded `builtin.software_dev.chat` display name so
            // the synthetic and DB-backed chat paths log the same label.
            name: "闲聊对话".to_string(),
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
    fn project(record: &IntentRecord, confidence: f32) -> IntentResult {
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

    /// Insert a domain plus one enabled intent and make the domain active.
    fn seed_active_domain(db: &Database, intent: &IntentRecord) {
        db.insert_intent_domain(&mk_domain(&intent.domain_id))
            .expect("insert domain");
        db.insert_intent(intent).expect("insert intent");
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
            seed_active_domain(
                &guard,
                &mk_intent("test.deep", "test.domain", "deep"),
            );
        }

        let result = router(db, false).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "builtin.software_dev.chat");
        assert_eq!(result.name, "闲聊对话");
        assert_eq!(result.path, PathSelection::Fast);
        assert!(result.config.enabled_tools.is_none());
        assert!(result.config.max_iterations.is_none());
        assert!(!result.config.auto_escalate);
    }

    #[tokio::test]
    async fn no_active_domain_returns_fast_path_without_panicking() {
        let db = test_db();
        // Domain exists but is not active.
        {
            let guard = db.lock().await;
            guard
                .insert_intent_domain(&mk_domain("test.inactive"))
                .expect("insert domain");
            guard
                .insert_intent(&mk_intent("test.inactive.intent", "test.inactive", "deep"))
                .expect("insert intent");
        }

        let result = router(db, true).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "builtin.software_dev.chat");
        assert_eq!(result.path, PathSelection::Fast);
        assert!(result.config.enabled_tools.is_none());
        assert!(result.config.max_iterations.is_none());
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

        let result = router(db, true).classify(MATCHING_MESSAGE).await;

        assert_eq!(result.id, "builtin.software_dev.chat");
        assert_eq!(result.path, PathSelection::Fast);
        assert!(result.config.max_iterations.is_none());
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
            guard
                .insert_intent_domain(&mk_domain("test.domain"))
                .expect("insert domain");
            for intent in [&alpha, &beta, &gamma] {
                guard.insert_intent(intent).expect("insert intent");
            }
            guard
                .set_active_intent_domain("test.domain")
                .expect("activate domain");
        }

        // No example matches; the only score comes from the bigram overlap
        // between the message and alpha's description (0.3), which is below
        // alpha's 0.9 threshold and therefore reroutes to gamma.
        let result = router(db, true).classify("实现功能代码").await;

        assert_eq!(result.id, "test.gamma");
        assert_eq!(
            result.path,
            PathSelection::Deep,
            "path must come from the reroute target's row, not from alpha or first()"
        );
        assert_eq!(
            result.config.enabled_tools,
            Some(vec!["file_system".to_string(), "calculator".to_string()]),
            "tool list must come from the reroute target's row"
        );
        assert!((result.confidence - 0.3).abs() < 0.2, "pre-reroute score");
    }
}
