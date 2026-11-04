use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    OpenAI,
    Anthropic,
    Google,
    Groq,
    DeepSeek,
    Zhipu,
    Moonshot,
    SiliconFlow,
    Ollama,
    LMStudio,
    Custom,
}

impl std::fmt::Display for ModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelProvider::OpenAI => write!(f, "openai"),
            ModelProvider::Anthropic => write!(f, "anthropic"),
            ModelProvider::Google => write!(f, "google"),
            ModelProvider::Groq => write!(f, "groq"),
            ModelProvider::DeepSeek => write!(f, "deepseek"),
            ModelProvider::Zhipu => write!(f, "zhipu"),
            ModelProvider::Moonshot => write!(f, "moonshot"),
            ModelProvider::SiliconFlow => write!(f, "siliconflow"),
            ModelProvider::Ollama => write!(f, "ollama"),
            ModelProvider::LMStudio => write!(f, "lmstudio"),
            ModelProvider::Custom => write!(f, "custom"),
        }
    }
}

impl std::str::FromStr for ModelProvider {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "openai" => Ok(ModelProvider::OpenAI),
            "anthropic" => Ok(ModelProvider::Anthropic),
            "google" => Ok(ModelProvider::Google),
            "groq" => Ok(ModelProvider::Groq),
            "deepseek" => Ok(ModelProvider::DeepSeek),
            "zhipu" => Ok(ModelProvider::Zhipu),
            "moonshot" => Ok(ModelProvider::Moonshot),
            "siliconflow" => Ok(ModelProvider::SiliconFlow),
            "ollama" => Ok(ModelProvider::Ollama),
            "lmstudio" => Ok(ModelProvider::LMStudio),
            "custom" => Ok(ModelProvider::Custom),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub provider: ModelProvider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
}

impl ModelConfig {
    pub fn default_base_url(&self) -> String {
        match self.provider {
            ModelProvider::OpenAI => "https://api.openai.com/v1/chat/completions".to_string(),
            ModelProvider::Anthropic => "https://api.anthropic.com/v1/messages".to_string(),
            ModelProvider::Google => {
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
                    .to_string()
            }
            ModelProvider::Groq => "https://api.groq.com/openai/v1/chat/completions".to_string(),
            ModelProvider::DeepSeek => "https://api.deepseek.com/v1/chat/completions".to_string(),
            ModelProvider::Zhipu => {
                "https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string()
            }
            ModelProvider::Moonshot => "https://api.moonshot.cn/v1/chat/completions".to_string(),
            ModelProvider::SiliconFlow => {
                "https://api.siliconflow.cn/v1/chat/completions".to_string()
            }
            ModelProvider::Ollama => "http://localhost:11434/v1/chat/completions".to_string(),
            ModelProvider::LMStudio => "http://localhost:1234/v1/chat/completions".to_string(),
            ModelProvider::Custom => String::new(),
        }
    }

    pub fn effective_base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| self.default_base_url())
    }

    pub fn is_compatible_with_openai_api(&self) -> bool {
        matches!(
            self.provider,
            ModelProvider::OpenAI
                | ModelProvider::Google
                | ModelProvider::Groq
                | ModelProvider::DeepSeek
                | ModelProvider::Zhipu
                | ModelProvider::Moonshot
                | ModelProvider::SiliconFlow
                | ModelProvider::Ollama
                | ModelProvider::LMStudio
                | ModelProvider::Custom
        )
    }
}

/// Default model: first `is_default && enabled`, else first `enabled`.
pub fn default_model(models: &[ModelConfig]) -> Option<&ModelConfig> {
    models
        .iter()
        .find(|m| m.is_default && m.enabled)
        .or_else(|| models.iter().find(|m| m.enabled))
}
