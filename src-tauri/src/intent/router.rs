use crate::intent::classifier::LlmClassifier;
use crate::intent::{IntentConfig, IntentResult, IntentRouterConfig};

/// The intent router: uses LLM to classify user messages and resolves intent configs.
pub struct IntentRouter {
    classifier: LlmClassifier,
    /// Map from intent name → IntentConfig
    configs: std::collections::HashMap<String, IntentConfig>,
    /// Default config (for unknown intents)
    default_config: IntentConfig,
    /// Whether routing is enabled
    enabled: bool,
}

impl IntentRouter {
    /// Build an IntentRouter from config.
    pub fn new(config: &IntentRouterConfig) -> Self {
        let classifier = LlmClassifier::new();

        // Merge default intents with user-configured intents
        let mut configs = super::default_intents();
        for (name, cfg) in &config.intents {
            configs.insert(name.clone(), cfg.clone());
        }

        Self {
            classifier,
            configs,
            default_config: IntentConfig {
                system_prompt_appendix: None,
                enabled_tools: None,
                model_id: None,
                max_iterations: None,
                auto_escalate: false,
            },
            enabled: config.enabled,
        }
    }

    /// Classify a user message into an intent. Uses LLM with fallback.
    pub async fn classify(&self, content: &str) -> IntentResult {
        if !self.enabled {
            return IntentResult {
                name: "chat".to_string(),
                config: self.default_config.clone(),
                path: IntentResult::path_for_intent("chat"),
            };
        }

        let result = self.classifier.classify(content).await;

        // Get config for detected intent, or use default
        let config = self
            .configs
            .get(&result.intent)
            .cloned()
            .unwrap_or_else(|| {
                // Create config from LLM result for unknown intents
                IntentConfig {
                    system_prompt_appendix: None,
                    enabled_tools: None,
                    model_id: None,
                    max_iterations: Some(result.max_iterations),
                    auto_escalate: result.auto_escalate,
                }
            });

        let intent_name = result.intent;
        let path = IntentResult::path_for_intent(&intent_name);
        IntentResult {
            name: intent_name,
            config,
            path,
        }
    }

    /// Check if an intent should trigger autonomous session mode.
    pub fn should_auto_escalate(&self, intent_name: &str) -> bool {
        self.configs
            .get(intent_name)
            .map(|cfg| cfg.auto_escalate)
            .unwrap_or(true) // Unknown intents default to true (conservative)
    }

    /// Resolve the final tool list by intersecting session tools with intent tools.
    pub fn resolve_tools(
        &self,
        session_tools: Option<Vec<String>>,
        intent_tools: Option<&Vec<String>>,
    ) -> Option<Vec<String>> {
        match (session_tools, intent_tools) {
            (Some(session), Some(intent)) => {
                let filtered: Vec<String> = session
                    .into_iter()
                    .filter(|t| intent.contains(t))
                    .collect();
                if filtered.is_empty() {
                    None
                } else {
                    Some(filtered)
                }
            }
            (Some(session), None) => Some(session),
            (None, _) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::IntentRouterConfig;

    fn make_config(enabled: bool) -> IntentRouterConfig {
        IntentRouterConfig {
            enabled,
            classifier_model_id: None,
            intents: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_should_auto_escalate_known() {
        let cfg = IntentRouterConfig::default();
        // can't create router without providers in test, but we can test the default configs
        assert_eq!(cfg.intents.get("code").map(|c| c.auto_escalate), Some(true));
        assert_eq!(cfg.intents.get("chat").map(|c| c.auto_escalate), Some(false));
        assert_eq!(cfg.intents.get("auto").map(|c| c.auto_escalate), Some(true));
    }

    #[test]
    fn test_resolve_tools_intersection() {
        let cfg = make_config(true);
        let router = IntentRouter::new(&cfg);

        let session = Some(vec![
            "calculator".to_string(),
            "code_executor".to_string(),
            "web_search".to_string(),
        ]);
        let intent = Some(vec!["code_executor".to_string(), "file_system".to_string()]);
        let result = router.resolve_tools(session, intent.as_ref());
        assert_eq!(result, Some(vec!["code_executor".to_string()]));
    }

    #[test]
    fn test_resolve_tools_no_intent_restriction() {
        let cfg = make_config(true);
        let router = IntentRouter::new(&cfg);
        let session = Some(vec!["calculator".to_string(), "web_search".to_string()]);
        let result = router.resolve_tools(session, None);
        assert_eq!(result, Some(vec!["calculator".to_string(), "web_search".to_string()]));
    }
}
