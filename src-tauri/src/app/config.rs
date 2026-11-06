//! 应用配置（`config.json`）。
//!
//! 这是唯一一份跨进程持久化的应用级配置：模型列表、启用的工具、MCP
//! server 定义、工作流变量与运行时安装设置。会话级、意图级配置不在这里，
//! 它们在 SQLite 里。
//!
//! 兼容性约定：所有新增字段都必须带 `#[serde(default)]`，且结构体不能加
//! `deny_unknown_fields`——用户机器上的旧配置必须永远能加载，不能因为一个
//! 废弃字段就让应用起不来。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::error::{AppError, Result};
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::features::mcp::config::McpServerConfig;

/// 应用级配置，序列化为 `<config_dir>/agent/config.json`。
///
/// 字段全部是「用户可见设置」；运行时状态（缓存、连接句柄）不放这里。
/// 写入只通过 [`AppConfig::save`]，避免并发调用方各自持有过期副本。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 已配置的模型。`is_default` 全为 false 时由 `default_model()` 兜底。
    pub models: Vec<ModelConfig>,
    /// 全局启用的工具名，与 `ToolRegistry` 中的注册名一致。
    pub enabled_tools: Vec<String>,
    /// 已配置的 MCP server，启动时按 `startup` 策略决定是否自动连接。
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    /// 工作流变量，注入到步骤参数时可见。
    #[serde(default)]
    pub workflow_vars: std::collections::HashMap<String, String>,
    /// 工作流密钥，只在执行期使用，读取接口永远不回传值。
    #[serde(default)]
    pub workflow_secrets: std::collections::HashMap<String, String>,
    /// 内置运行时的自定义安装目录。
    /// 为 None 时使用 `<app_data_dir>/agent/runtimes/`。
    #[serde(default)]
    pub runtime_install_dir: Option<String>,
    /// 下载运行时时使用的可选 HTTP 代理，例如 "http://127.0.0.1:7890"。
    #[serde(default)]
    pub download_proxy: Option<String>,
    /// 每种运行时的当前版本管理器。
    /// Key = 运行时类型字符串（如 "node"），Value = 管理器 id（如 "fnm"）。
    #[serde(default)]
    pub active_managers: std::collections::HashMap<String, String>,
    /// 每种运行时持久化的默认版本。
    /// Key = 运行时类型字符串（如 "node"），Value = 精确版本（如 "20.18.3"）。
    #[serde(default)]
    pub runtime_defaults: std::collections::HashMap<String, String>,
    /// 应用级意图路由开关。单个意图的配置存在 `intents` 表里，不在配置文件中。
    #[serde(default)]
    pub intent: IntentAppConfig,
}

/// 应用级意图路由开关。
///
/// 这里的值对所有意图生效；单意图覆盖（模型、工具白名单等）在数据库里。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAppConfig {
    /// 总开关：关闭时所有消息都按普通聊天处理，路由完全跳过。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 分类使用的模型；`None` 表示沿用默认模型。
    #[serde(default)]
    pub classifier_model_id: Option<String>,
    /// 意图召回使用的 embedding 模型。`None` 表示
    /// `text-embedding-3-small`。
    #[serde(default)]
    pub embedding_model_id: Option<String>,
    /// embedding 判定「确定命中」所需的最低原始 cosine 分数。
    /// 由路由评测集标定，改动前请重跑评测。
    #[serde(default = "default_embedding_floor")]
    pub embedding_floor: f32,
    /// top1 与 top2 原始 cosine 分数之间的最小差值。
    #[serde(default = "default_embedding_margin")]
    pub embedding_margin: f32,
    /// 仅影响排序：对会话上一轮所属领域的加分。
    #[serde(default = "default_prior_boost")]
    pub prior_boost: f32,
    /// embedding 结果不够确定时，是否允许再跑一次 LLM 重排。
    #[serde(default = "default_true")]
    pub llm_rerank_enabled: bool,
    /// LLM 重排的硬超时（毫秒）。
    #[serde(default = "default_llm_timeout_ms")]
    pub llm_timeout_ms: u64,
    /// LLM 结果置信度偏低时，是否允许执行只读探针。
    #[serde(default = "default_true")]
    pub probe_enabled: bool,
    /// 只读探针的硬超时（毫秒）。
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
    /// 启动「探针 + 二次重排」所需的最小剩余预算（毫秒）。
    #[serde(default = "default_second_rerank_min_budget_ms")]
    pub second_rerank_min_budget_ms: u64,
    /// 单次路由决策的墙钟总预算（毫秒）。
    #[serde(default = "default_routing_budget_ms")]
    pub routing_budget_ms: u64,
}

/// 追加一个不重复的工具名。
fn push_unique(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|item| item == name) {
        list.push(name.to_string());
    }
}

fn default_embedding_floor() -> f32 {
    0.55
}

fn default_embedding_margin() -> f32 {
    0.05
}

fn default_prior_boost() -> f32 {
    0.04
}

/// 单次 LLM rerank 的默认超时。
///
/// 必须覆盖分类模型在真实消息上的往返：实测 DeepSeek 默认模型峰值 3.7s。
/// 低于这个量级会让 LLM 信号在超时后丢失，路由退化到噪声很大的规则打分。
fn default_llm_timeout_ms() -> u64 {
    8000
}

fn default_probe_timeout_ms() -> u64 {
    1500
}

fn default_second_rerank_min_budget_ms() -> u64 {
    1200
}

fn default_routing_budget_ms() -> u64 {
    12000
}

/// 上一版默认超时：会被旧配置原样写进 `config.json`，加载时迁移掉。
const LEGACY_LLM_TIMEOUT_MS: u64 = 2500;
const LEGACY_ROUTING_BUDGET_MS: u64 = 6000;

impl IntentAppConfig {
    /// 把上一版默认超时迁移到当前默认值。
    ///
    /// 旧配置把 2500 / 6000 固化在文件里，只改 [`Default`] 对已有安装无效。
    /// 只迁移恰好等于旧默认值的字段：用户显式改过的值保持不变。返回是否
    /// 发生了迁移，调用方据此决定是否写回磁盘。
    pub fn migrate_legacy_timeouts(&mut self) -> bool {
        let mut migrated = false;

        if self.llm_timeout_ms == LEGACY_LLM_TIMEOUT_MS {
            self.llm_timeout_ms = default_llm_timeout_ms();
            migrated = true;
        }
        if self.routing_budget_ms == LEGACY_ROUTING_BUDGET_MS {
            self.routing_budget_ms = default_routing_budget_ms();
            migrated = true;
        }

        migrated
    }
}

impl Default for IntentAppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            classifier_model_id: None,
            embedding_model_id: None,
            embedding_floor: default_embedding_floor(),
            embedding_margin: default_embedding_margin(),
            prior_boost: default_prior_boost(),
            llm_rerank_enabled: true,
            llm_timeout_ms: default_llm_timeout_ms(),
            probe_enabled: true,
            probe_timeout_ms: default_probe_timeout_ms(),
            second_rerank_min_budget_ms: default_second_rerank_min_budget_ms(),
            routing_budget_ms: default_routing_budget_ms(),
        }
    }
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            models: vec![
                ModelConfig {
                    id: "default-openai".to_string(),
                    name: "gpt-5.5".to_string(),
                    display_name: "GPT-5.5".to_string(),
                    provider: ModelProvider::OpenAI,
                    api_key: String::new(),
                    base_url: None,
                    is_default: true,
                    enabled: true,
                    context_window: Some(1_000_000),
                    max_tokens: Some(128_000),
                },
                ModelConfig {
                    id: "default-ollama".to_string(),
                    name: "llama3.3".to_string(),
                    display_name: "Llama 3.3 (Ollama)".to_string(),
                    provider: ModelProvider::Ollama,
                    api_key: String::new(),
                    base_url: None,
                    is_default: false,
                    enabled: false,
                    context_window: Some(131072),
                    max_tokens: Some(32768),
                },
            ],
            enabled_tools: vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "search".to_string(),
                "list".to_string(),
                "shell".to_string(),
                "delete".to_string(),
                "move".to_string(),
            ],
            mcp_servers: vec![],
            workflow_vars: std::collections::HashMap::new(),
            workflow_secrets: std::collections::HashMap::new(),
            runtime_install_dir: None,
            download_proxy: None,
            active_managers: std::collections::HashMap::new(),
            runtime_defaults: std::collections::HashMap::new(),
            intent: IntentAppConfig::default(),
        }
    }
}

impl AppConfig {
    /// 投影出独立工具链引擎需要的纯设置值。
    ///
    /// CLI 侧不持有 `AppConfig`，只拿这份快照，避免把 Tauri 相关类型带进
    /// `runtime` 层。
    pub fn runtime_options(&self) -> crate::runtime::toolchain::RuntimeOptions {
        crate::runtime::toolchain::RuntimeOptions {
            install_dir: self
                .runtime_install_dir
                .as_ref()
                .map(std::path::PathBuf::from),
            download_proxy: self.download_proxy.clone(),
            defaults: self.runtime_defaults.clone(),
        }
    }

    /// 从磁盘加载配置；文件不存在时写出一份默认配置并返回它。
    ///
    /// 解析失败视为错误而不是静默重置：用户的模型与 key 配置不能被悄悄
    /// 覆盖。
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let mut config: AppConfig = serde_json::from_str(&content)
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            if config.intent.migrate_legacy_timeouts() {
                log::info!(
                    "config: migrated legacy intent timeouts to llm={}ms budget={}ms",
                    config.intent.llm_timeout_ms,
                    config.intent.routing_budget_ms
                );
                // 写回失败不影响启动：内存里已经是迁移后的值，下次启动会重试。
                if let Err(error) = config.save() {
                    log::warn!("config: failed to persist migrated intent timeouts: {error}");
                }
            }
            if config.migrate_legacy_tool_names() {
                log::info!(
                    "config: migrated legacy tool names to rig-tools: {:?}",
                    config.enabled_tools
                );
                if let Err(error) = config.save() {
                    log::warn!("config: failed to persist migrated tool names: {error}");
                }
            }
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    /// 覆盖写回磁盘；父目录不存在时自动创建。
    /// 把旧的内置工具名迁移到 rig-tools 的名字；返回是否发生改动。
    ///
    /// `file_system` 拆成文件类工具，`code_executor` 落到 `shell`；
    /// `calculator` / `web_search` / `run_workflow` 没有替代，直接丢弃。
    fn migrate_legacy_tool_names(&mut self) -> bool {
        const LEGACY: [&str; 5] = [
            "file_system",
            "code_executor",
            "calculator",
            "web_search",
            "run_workflow",
        ];
        if !self
            .enabled_tools
            .iter()
            .any(|tool| LEGACY.contains(&tool.as_str()))
        {
            return false;
        }

        let mut migrated = Vec::new();
        for tool in std::mem::take(&mut self.enabled_tools) {
            match tool.as_str() {
                "file_system" => {
                    for name in ["read", "write", "edit", "search", "list", "delete", "move"] {
                        push_unique(&mut migrated, name);
                    }
                }
                "code_executor" => push_unique(&mut migrated, "shell"),
                "calculator" | "web_search" | "run_workflow" => {}
                other => push_unique(&mut migrated, other),
            }
        }
        self.enabled_tools = migrated;
        true
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// 配置文件位置：`<config_dir>/agent/config.json`。
    /// 取不到系统配置目录时退回当前工作目录，保证至少可写。
    fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("agent");
        path.push("config.json");
        path
    }

    /// 按 id 查找模型；不存在返回 `None`。
    pub fn get_model(&self, id: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.id == id)
    }

    /// 只返回启用状态的模型，供模型选择器使用。
    pub fn get_enabled_models(&self) -> Vec<&ModelConfig> {
        self.models.iter().filter(|m| m.enabled).collect()
    }

    /// 取默认模型；`is_default` 缺失时由 `default_model()` 兜底。
    pub fn get_default_model(&self) -> Option<&ModelConfig> {
        crate::llm::model::default_model(&self.models)
    }

    /// 追加一个模型，不校验 id 是否重复（调用方负责）。
    pub fn add_model(&mut self, model: ModelConfig) {
        self.models.push(model);
    }

    /// 按 id 删除模型；返回是否真的删掉了。
    pub fn remove_model(&mut self, id: &str) -> bool {
        let len = self.models.len();
        self.models.retain(|m| m.id != id);
        self.models.len() < len
    }

    /// 用 `updates` 整体替换同 id 的模型；返回是否找到目标。
    pub fn update_model(&mut self, id: &str, updates: ModelConfig) -> bool {
        if let Some(model) = self.models.iter_mut().find(|m| m.id == id) {
            *model = updates;
            true
        } else {
            false
        }
    }

    /// 把 `id` 设为唯一默认模型；返回该 id 是否存在。
    /// 不存在时仍会清空其他模型的 `is_default`，调用方需据此报错。
    pub fn set_default_model(&mut self, id: &str) -> bool {
        for model in &mut self.models {
            model.is_default = model.id == id;
        }
        self.models.iter().any(|m| m.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, IntentAppConfig};

    #[test]
    fn old_config_json_with_intent_routing_still_loads() {
        // `intent_routing` 已不存在；`AppConfig` 没有 `deny_unknown_fields`，
        // 因此这个键必须被忽略，而不是让用户的配置变成启动错误。
        let json = r#"{
            "models": [],
            "enabled_tools": [],
            "intent_routing": {
                "enabled": true,
                "classifier_model_id": null,
                "intents": { "code": { "auto_escalate": true } }
            }
        }"#;

        let config: AppConfig = serde_json::from_str(json).expect("旧配置必须仍然能加载");

        assert!(config.intent.enabled, "缺失的 intent 段必须默认开启");
        assert_eq!(config.intent.classifier_model_id, None);
    }

    #[test]
    fn empty_intent_section_defaults_to_enabled() {
        let json = r#"{
            "models": [],
            "enabled_tools": [],
            "intent": {}
        }"#;

        let config: AppConfig = serde_json::from_str(json).expect("intent 段缺省字段必须可加载");

        assert!(config.intent.enabled);
        assert_eq!(config.intent.classifier_model_id, None);
    }

    #[test]
    fn empty_intent_section_uses_routing_defaults() {
        let json = r#"{
            "models": [],
            "enabled_tools": [],
            "intent": {}
        }"#;

        let config: AppConfig = serde_json::from_str(json).expect("intent 段缺省字段必须可加载");
        let intent = config.intent;

        assert_eq!(intent.embedding_model_id, None);
        assert!((intent.embedding_floor - 0.55).abs() < f32::EPSILON);
        assert!((intent.embedding_margin - 0.05).abs() < f32::EPSILON);
        assert!((intent.prior_boost - 0.04).abs() < f32::EPSILON);
        assert!(intent.llm_rerank_enabled);
        assert_eq!(intent.llm_timeout_ms, 8000);
        assert!(intent.probe_enabled);
        assert_eq!(intent.probe_timeout_ms, 1500);
        assert_eq!(intent.second_rerank_min_budget_ms, 1200);
        assert_eq!(intent.routing_budget_ms, 12000);
    }

    /// 分类模型在真实消息上的往返时间实测到 3.7s；超时低于它会让 LLM
    /// 信号直接丢失，路由退化到规则打分。
    #[test]
    fn routing_timeouts_cover_real_classifier_latency() {
        let intent = IntentAppConfig::default();

        assert_eq!(intent.llm_timeout_ms, 8000);
        assert_eq!(intent.routing_budget_ms, 12000);
    }

    #[test]
    fn router_config_defaults_match_app_config() {
        let app = IntentAppConfig::default();
        let router = crate::llm::intent::RouterConfig::default();

        assert_eq!(router.llm_timeout_ms, app.llm_timeout_ms);
        assert_eq!(router.routing_budget_ms, app.routing_budget_ms);
    }

    #[test]
    fn legacy_timeout_defaults_are_migrated() {
        let mut intent = IntentAppConfig {
            llm_timeout_ms: 2500,
            routing_budget_ms: 6000,
            ..IntentAppConfig::default()
        };

        assert!(intent.migrate_legacy_timeouts());
        assert_eq!(intent.llm_timeout_ms, 8000);
        assert_eq!(intent.routing_budget_ms, 12000);
    }

    #[test]
    fn custom_timeouts_are_not_migrated() {
        let mut intent = IntentAppConfig {
            llm_timeout_ms: 4000,
            routing_budget_ms: 9000,
            ..IntentAppConfig::default()
        };

        assert!(!intent.migrate_legacy_timeouts());
        assert_eq!(intent.llm_timeout_ms, 4000);
        assert_eq!(intent.routing_budget_ms, 9000);
    }

    #[test]
    fn timeouts_are_migrated_independently() {
        let mut intent = IntentAppConfig {
            llm_timeout_ms: 2500,
            routing_budget_ms: 9000,
            ..IntentAppConfig::default()
        };

        assert!(intent.migrate_legacy_timeouts());
        assert_eq!(intent.llm_timeout_ms, 8000);
        assert_eq!(intent.routing_budget_ms, 9000);
    }
}
