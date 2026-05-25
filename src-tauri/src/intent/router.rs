use crate::intent::{IntentConfig, IntentResult, IntentRouterConfig};
use regex::Regex;

/// The intent router: classifies user messages, handles reclassification,
/// and resolves intent configs.
pub struct IntentRouter {
    /// Compiled rules: (Regex, intent_name)
    rules: Vec<(Regex, String, i32)>,
    /// Map from intent name → IntentConfig
    configs: std::collections::HashMap<String, IntentConfig>,
    /// Default config (for "general")
    general_config: IntentConfig,
    /// Whether routing is enabled at all
    enabled: bool,
}

impl IntentRouter {
    /// Build an IntentRouter from config.
    pub fn new(config: &IntentRouterConfig) -> Self {
        // Compile rules, skipping invalid regex
        let mut rules: Vec<(Regex, String, i32)> = Vec::new();
        for rule in &config.rules {
            match Regex::new(&rule.pattern) {
                Ok(re) => rules.push((re, rule.intent.clone(), rule.priority)),
                Err(e) => {
                    log::warn!(
                        "Skipping invalid intent rule pattern '{}': {}",
                        rule.pattern,
                        e
                    );
                }
            }
        }
        // Sort by priority descending (highest priority first)
        rules.sort_by(|a, b| b.2.cmp(&a.2));

        // Merge configured intents with defaults
        let mut configs = super::default_intents();
        for (name, cfg) in &config.intents {
            configs.insert(name.clone(), cfg.clone());
        }

        Self {
            rules,
            configs,
            general_config: IntentConfig {
                system_prompt_appendix: None,
                enabled_tools: None,
                model_id: None,
                max_iterations: None,
                reclassify_triggers: vec![],
            },
            enabled: config.enabled,
        }
    }

    /// Classify a user message into an intent.
    ///
    /// Returns the first matching rule by priority, or "general" if no rule matches.
    pub fn classify(&self, content: &str) -> IntentResult {
        if !self.enabled {
            return IntentResult {
                name: "general".to_string(),
                config: self.general_config.clone(),
                matched_rule: None,
            };
        }

        for (re, intent_name, _priority) in &self.rules {
            if re.is_match(content) {
                let config = self
                    .configs
                    .get(intent_name)
                    .cloned()
                    .unwrap_or_else(|| self.general_config.clone());
                return IntentResult {
                    name: intent_name.clone(),
                    config,
                    matched_rule: Some(re.as_str().to_string()),
                };
            }
        }

        IntentResult {
            name: "general".to_string(),
            config: self.general_config.clone(),
            matched_rule: None,
        }
    }

    /// Reclassify based on tool result content.
    ///
    /// Checks the current intent's `reclassify_triggers`. If a trigger's pattern
    /// matches the tool result, returns the target intent name along with its config.
    /// Otherwise returns `None` (stay on current intent).
    pub fn reclassify(&self, tool_result: &str, current_intent: &str) -> Option<IntentResult> {
        if !self.enabled {
            return None;
        }

        let config = self.configs.get(current_intent)?;
        for trigger in &config.reclassify_triggers {
            if trigger.from != current_intent {
                continue;
            }
            if let Ok(re) = Regex::new(&trigger.pattern) {
                if re.is_match(tool_result) {
                    let new_config = self
                        .configs
                        .get(&trigger.to_intent)
                        .cloned()
                        .unwrap_or_else(|| self.general_config.clone());
                    return Some(IntentResult {
                        name: trigger.to_intent.clone(),
                        config: new_config,
                        matched_rule: Some(trigger.pattern.clone()),
                    });
                }
            }
        }

        None
    }

    /// Resolve the final tool list by intersecting session tools with intent tools.
    ///
    /// - `session_tools`: tools enabled at the session level (from session config).
    /// - `intent_tools`: tools enabled by the matched intent (if any).
    ///
    /// Returns `None` if no intent-level restriction applies (use session tools only),
    /// or `Some(list)` with the intersection.
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
                // If intersection is empty, fall back to session tools
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

    /// Get the config for a given intent name.
    pub fn get_config(&self, intent: &str) -> Option<&IntentConfig> {
        self.configs.get(intent)
    }

    /// Whether routing is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::IntentRule;

    fn make_router(enabled: bool) -> IntentRouter {
        let cfg = IntentRouterConfig {
            enabled,
            classifier_model_id: None,
            rules: vec![
                IntentRule {
                    pattern: r"(?i)\b(code|implement|write)\b".to_string(),
                    intent: "code".to_string(),
                    priority: 10,
                },
                IntentRule {
                    pattern: r"(?i)\b(search|find|research)\b".to_string(),
                    intent: "research".to_string(),
                    priority: 10,
                },
            ],
            intents: std::collections::HashMap::new(),
        };
        IntentRouter::new(&cfg)
    }

    #[test]
    fn test_classify_code() {
        let router = make_router(true);
        let result = router.classify("implement a sorting algorithm");
        assert_eq!(result.name, "code");
        assert!(result.matched_rule.is_some());
    }

    #[test]
    fn test_classify_research() {
        let router = make_router(true);
        let result = router.classify("search for Rust async patterns");
        assert_eq!(result.name, "research");
        assert!(result.matched_rule.is_some());
    }

    #[test]
    fn test_classify_general_default() {
        let router = make_router(true);
        let result = router.classify("hello, how are you?");
        assert_eq!(result.name, "general");
        assert!(result.matched_rule.is_none());
    }

    #[test]
    fn test_classify_priority_ordering() {
        let cfg = IntentRouterConfig {
            enabled: true,
            classifier_model_id: None,
            rules: vec![
                IntentRule {
                    pattern: "code".to_string(),
                    intent: "code".to_string(),
                    priority: 5,
                },
                IntentRule {
                    pattern: "code".to_string(),
                    intent: "research".to_string(),
                    priority: 10,
                },
            ],
            intents: std::collections::HashMap::new(),
        };
        let router = IntentRouter::new(&cfg);
        let result = router.classify("code");
        // Higher priority (10) should win → "research"
        assert_eq!(result.name, "research");
    }

    #[test]
    fn test_classify_disabled_returns_general() {
        let router = make_router(false);
        let result = router.classify("implement this code now");
        assert_eq!(result.name, "general");
        assert!(result.matched_rule.is_none());
    }

    #[test]
    fn test_invalid_regex_skipped() {
        let cfg = IntentRouterConfig {
            enabled: true,
            classifier_model_id: None,
            rules: vec![
                IntentRule {
                    pattern: r"[invalid".to_string(), // invalid regex
                    intent: "code".to_string(),
                    priority: 10,
                },
                IntentRule {
                    pattern: r"hello".to_string(),
                    intent: "general".to_string(),
                    priority: 5,
                },
            ],
            intents: std::collections::HashMap::new(),
        };
        let router = IntentRouter::new(&cfg);
        // Should not crash; should still match "hello"
        let result = router.classify("hello world");
        assert_eq!(result.name, "general");
    }

    #[test]
    fn test_reclassify_matches_trigger() {
        let router = make_router(true);
        // "code" intent has reclassify trigger for "search results" → "research"
        let result = router.reclassify("here are the search results for Rust", "code");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "research");
    }

    #[test]
    fn test_reclassify_no_match() {
        let router = make_router(true);
        // "code" intent's reclassify patterns: "search results|found|according to|citation"
        // This content doesn't match any of them → should return None
        let result = router.reclassify("xyz something completely different 123", "code");
        assert!(result.is_none());
    }

    #[test]
    fn test_reclassify_no_intent_config() {
        let router = make_router(true);
        let result = router.reclassify("anything", "nonexistent_intent");
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_tools_intersection() {
        let router = make_router(true);
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
    fn test_resolve_tools_empty_intersection_falls_back() {
        let router = make_router(true);
        let session = Some(vec!["calculator".to_string(), "web_search".to_string()]);
        let intent = Some(vec!["code_executor".to_string(), "file_system".to_string()]);
        let result = router.resolve_tools(session, intent.as_ref());
        // Empty intersection → return None, caller will use session tools
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_tools_no_intent_restriction() {
        let router = make_router(true);
        let session = Some(vec!["calculator".to_string(), "web_search".to_string()]);
        let result = router.resolve_tools(session, None);
        assert_eq!(result, Some(vec!["calculator".to_string(), "web_search".to_string()]));
    }
}
