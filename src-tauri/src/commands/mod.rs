use serde::{Deserialize, Serialize};

pub mod session;
pub mod environment;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod persona;
pub mod pipeline;
pub mod prompt;
pub mod settings;
pub mod skill;
pub mod tool;
pub mod window;

pub use session::*;
pub use environment::*;
pub use mcp::*;
pub use memory::*;
pub use model::*;
pub use persona::*;
pub use pipeline::*;
pub use prompt::*;
pub use settings::*;
pub use skill::*;
pub use tool::*;
pub use window::*;

// ── Shared types ──

#[derive(Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub content: String,
    pub done: bool,
    pub tool_calls: Option<Vec<ToolCallEvent>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    pub status: String,
    pub result: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub provider: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
}

impl From<&crate::config::ModelConfig> for ModelInfo {
    fn from(m: &crate::config::ModelConfig) -> Self {
        Self {
            id: m.id.clone(),
            name: m.name.clone(),
            display_name: m.display_name.clone(),
            provider: m.provider.to_string(),
            api_key: m.api_key.clone(),
            base_url: m.base_url.clone(),
            is_default: m.is_default,
            enabled: m.enabled,
            context_window: m.context_window,
            max_tokens: m.max_tokens,
        }
    }
}

pub const PROVIDER_OPTIONS: &[(&str, &str, &str, &str)] = &[
    ("openai", "OpenAI", "https://api.openai.com/v1/chat/completions", "gpt-5.5"),
    ("anthropic", "Anthropic", "https://api.anthropic.com/v1/messages", "claude-sonnet-4-6"),
    ("google", "Google (Gemini)", "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions", "gemini-2.5-flash"),
    ("groq", "Groq", "https://api.groq.com/openai/v1/chat/completions", "llama-3.3-70b-versatile"),
    ("deepseek", "DeepSeek", "https://api.deepseek.com/v1/chat/completions", "deepseek-v4-flash"),
    ("zhipu", "智谱清言", "https://open.bigmodel.cn/api/paas/v4/chat/completions", "glm-5"),
    ("moonshot", "月之暗面", "https://api.moonshot.cn/v1/chat/completions", "kimi-k2.6"),
    ("siliconflow", "硅基流动", "https://api.siliconflow.cn/v1/chat/completions", "Qwen/Qwen2.5-72B-Instruct"),
    ("ollama", "Ollama (本地)", "http://localhost:11434/v1/chat/completions", "llama3.3"),
    ("lmstudio", "LM Studio (本地)", "http://localhost:1234/v1/chat/completions", "local-model"),
    ("custom", "自定义", "", "custom-model"),
];

/// Known model IDs per provider, used as fallback when no models are configured.
pub const PROVIDER_MODELS: &[(&str, &[&str])] = &[
    ("openai", &["gpt-5.5", "gpt-5.4-mini", "gpt-5.4-nano", "gpt-4o", "gpt-4o-mini", "o3", "o4-mini"]),
    ("anthropic", &["claude-opus-4-7", "claude-sonnet-4-6", "claude-haiku-4-5", "claude-opus-4-6", "claude-sonnet-4-5"]),
    ("google", &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.0-flash", "gemini-2.0-flash-lite"]),
    ("groq", &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "llama-4-scout-17b-16e-instruct", "qwen/qwen3-32b", "mixtral-8x7b-32768", "gemma2-9b-it", "openai/gpt-oss-120b", "openai/gpt-oss-20b"]),
    ("deepseek", &["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-chat", "deepseek-reasoner"]),
    ("zhipu", &["glm-5", "glm-5.1", "glm-4-plus", "glm-4-air", "glm-4-flash"]),
    ("moonshot", &["kimi-k2.6", "kimi-k2.5", "moonshot-v1-128k", "moonshot-v1-32k", "moonshot-v1-8k"]),
    ("siliconflow", &["Qwen/Qwen2.5-72B-Instruct", "Qwen/Qwen2.5-32B-Instruct", "Qwen/Qwen3-35B-Instruct", "deepseek-ai/DeepSeek-V4-Pro", "deepseek-ai/DeepSeek-V4-Flash", "Z-ai/GLM-5", "Z-ai/GLM-5.1"]),
    ("ollama", &["llama3.3", "llama3.2", "qwen3", "qwen2.5", "deepseek-r1", "deepseek-v4-flash", "gemma3", "mistral", "phi4"]),
    ("lmstudio", &["local-model"]),
    ("custom", &["custom-model"]),
];
