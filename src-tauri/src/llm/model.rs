use serde::{Deserialize, Serialize};

/// 支持的模型供应商。序列化名为小写字符串，与配置文件中的 `provider` 字段一致。
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

/// 一个已配置模型的完整信息。
///
/// `id` 是配置内的唯一键（用于注册表查找），`name` 是发给供应商的模型名，
/// `display_name` 只用于界面展示。
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
    /// 该供应商的内置 API 地址；`Custom` 没有默认值，返回空串。
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

    /// 实际生效的地址：用户配置的 `base_url` 优先，否则回退到内置默认值。
    pub fn effective_base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| self.default_base_url())
    }

    /// 该供应商是否说 OpenAI 风格的 Chat Completions 协议。
    ///
    /// 注意 Anthropic 是唯一不兼容的例外。
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

/// 默认模型：第一个 `is_default && enabled`，否则第一个 `enabled`。
pub fn default_model(models: &[ModelConfig]) -> Option<&ModelConfig> {
    models
        .iter()
        .find(|m| m.is_default && m.enabled)
        .or_else(|| models.iter().find(|m| m.enabled))
}
