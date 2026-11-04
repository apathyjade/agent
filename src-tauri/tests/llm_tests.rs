//! Smoke tests for the Rig provider wiring.
//!
//! These build a provider for every configured backend without sending a
//! request: `create_rig_provider` must turn a [`ModelConfig`] into a Rig model
//! on every supported wire.

#![allow(clippy::disallowed_methods)]

use agent_lib::llm::rig::create_rig_provider;
use agent_lib::llm::model::{ModelConfig, ModelProvider};

fn model_config(provider: ModelProvider, name: &str, base_url: Option<&str>) -> ModelConfig {
    ModelConfig {
        id: format!("{}-{}", provider, name),
        name: name.to_string(),
        display_name: name.to_string(),
        provider,
        api_key: "test-key".to_string(),
        base_url: base_url.map(|url| url.to_string()),
        is_default: false,
        enabled: true,
        context_window: None,
        max_tokens: None,
    }
}

#[test]
fn builds_a_provider_for_every_native_backend() {
    let cases = [
        (ModelProvider::OpenAI, "gpt-4o"),
        (ModelProvider::Anthropic, "claude-sonnet-4-5"),
        (ModelProvider::Google, "gemini-2.5-pro"),
        (ModelProvider::Groq, "llama-3.3-70b-versatile"),
        (ModelProvider::DeepSeek, "deepseek-chat"),
        (ModelProvider::Moonshot, "kimi-k2"),
        (ModelProvider::Ollama, "llama3.2"),
    ];

    for (provider, name) in cases {
        let config = model_config(provider.clone(), name, None);
        let built = create_rig_provider(&config)
            .unwrap_or_else(|e| panic!("{provider} provider failed to build: {e}"));
        assert_eq!(built.model_id(), name);
    }
}

#[test]
fn builds_openai_compatible_provider_from_a_full_chat_completions_url() {
    let config = model_config(
        ModelProvider::Zhipu,
        "glm-4-flash",
        Some("https://open.bigmodel.cn/api/paas/v4/chat/completions"),
    );

    let built = create_rig_provider(&config).expect("openai-compatible provider should build");
    assert_eq!(built.model_id(), "glm-4-flash");
}

#[test]
fn builds_openai_compatible_provider_without_a_config_path() {
    let config = model_config(ModelProvider::LMStudio, "local-model", None);

    let built = create_rig_provider(&config).expect("default openai-compatible provider should build");
    assert_eq!(built.model_id(), "local-model");
}
