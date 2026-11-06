//! Provider（模型服务商）配置 IPC 命令。
//!
//! 一个 provider 对应多个模型条目；`setup_provider` 是一次性的完整配置
//! （新增 + 清理 + 存 key + 注册），`update_provider_config` 则是增量修改。
//!
//! key 的存放顺序：优先 OS keychain，失败时回落到 `config.json` 的
//! `api_key` 字段（mock keychain、CI 等环境）。
//!
//! 安全边界：日志里永远不打印 key；返回给前端的 DTO 也不包含 key。

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app::config::AppConfig;
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::llm::provider::ProviderRegistry;
use crate::core::error::{AppError, Result};
use crate::core::keychain;
use crate::app::state::AppState;

/// 前端展示用的 provider 状态。
#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub id: String,
    pub name: String,
    /// 是否已配置完成（无需 key 的 provider 恒为 true）。
    pub configured: bool,
    pub base_url: Option<String>,
    pub enabled_models: Vec<String>,
    pub available_models: Vec<AvailableModel>,
    pub requires_api_key: bool,
}

/// 可选择的模型条目。
#[derive(Clone, Serialize, Deserialize)]
pub struct AvailableModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u32>,
}

/// 列出所有内置 provider 的配置状态。
///
/// 对每个 provider 计算：是否已配置、已启用哪些模型、可供选择的模型列表。
/// 尚未配置任何模型的 provider 会回退到内置模型目录
/// （`PROVIDER_MODELS`），让用户有东西可选。
#[tauri::command]
pub async fn list_providers_cmd(state: State<'_, AppState>) -> Result<Vec<ProviderStatus>> {
    let config = state.config.lock().await;
    let provider_registry = state.providers.lock().await;
    let registered_ids = provider_registry.get_registered_model_ids();
    drop(provider_registry);

    let providers = crate::app::commands::PROVIDER_OPTIONS.iter().map(|(id, name, base_url, default_model)| {
        let models: Vec<&ModelConfig> = config.models.iter()
            .filter(|m| m.provider.to_string() == *id)
            .collect();
        let requires_key = *id != "ollama" && *id != "lmstudio";
        let has_configured = models.iter().any(|m| {
            if !m.enabled { return false; }
            // 模型必须真正注册进 provider 注册表，才算「有有效 API key
            // 且初始化成功」。
            registered_ids.contains(&m.id)
        });
        let available_models: Vec<AvailableModel> = if models.is_empty() || !has_configured {
            // 该 provider 还没有可用模型：展示内置模型目录，供用户挑选。
            crate::app::commands::PROVIDER_MODELS.iter()
                .find(|(pid, _)| *pid == *id)
                .map(|(_, model_ids)| model_ids.iter().map(|mid| AvailableModel {
                    id: mid.to_string(),
                    name: mid.to_string(),
                    context_window: None,
                }).collect())
                .unwrap_or_else(|| vec![AvailableModel {
                    id: default_model.to_string(),
                    name: default_model.to_string(),
                    context_window: None,
                }])
        } else {
            models.iter().map(|m| AvailableModel {
                id: m.id.clone(),
                name: m.name.clone(),
                context_window: m.context_window,
            }).collect()
        };

        ProviderStatus {
            id: id.to_string(),
            name: name.to_string(),
            configured: !requires_key || has_configured,
            base_url: Some(base_url.to_string()),
            enabled_models: models.iter().filter(|m| m.enabled).map(|m| m.id.clone()).collect(),
            available_models,
            requires_api_key: requires_key,
        }
    }).collect();
    Ok(providers)
}

/// 完整配置一个 provider：补齐模型条目、清理旧条目、存 key、注册到运行时。
///
/// `enabled_models` 是用户勾选的模型 id 全集：同 provider 下不在这个列表
/// 里的旧模型会被删除（例如没有 key 的旧默认模型），列表第一项成为新的
/// 默认模型。`api_key` 只对需要 key 的 provider 生效。
#[tauri::command]
pub async fn setup_provider(
    state: State<'_, AppState>,
    provider: String,
    api_key: String,
    base_url: Option<String>,
    enabled_models: Vec<String>,
) -> Result<()> {
    let provider_enum: ModelProvider = provider.parse()
        .map_err(|e: String| AppError::InvalidInput(e))?;

    let mut config = state.config.lock().await;

    ensure_models_configured(&mut config, &provider_enum, &enabled_models, &base_url);
    // 删除同 provider 下不在启用列表里的旧模型
    // （例如没有配置 API key 的旧 default-openai）。
    retain_enabled_models(&mut config, &provider, &enabled_models);

    // 第一个启用模型作为新的默认模型。
    if let Some(first) = enabled_models.first() {
        if config.get_model(first).is_some() {
            config.set_default_model(first);
        }
    }
    config.save()?;

    // 清理 provider 注册表里的旧条目（旧默认模型等）。
    {
        let mut providers = state.providers.lock().await;
        prune_registry(&mut providers, &config, &provider, &enabled_models);
    }

    // 把 API key 存进系统 keychain；失败时回落到 config.json。
    let requires_api_key = provider != "ollama" && provider != "lmstudio";
    let mut keychain_failed = false;
    if requires_api_key && !api_key.is_empty() {
        keychain_failed = !store_api_keys(&enabled_models, &api_key);
    }

    // keychain 不可用（mock 后端、CI 等）时，把 key 写进 config.json。
    if keychain_failed {
        fallback_store_api_key(&mut config, &provider, &api_key)?;
    }

    let mut providers = state.providers.lock().await;
    if !requires_api_key || !api_key.is_empty() {
        register_models(
            &mut providers,
            &config,
            &enabled_models,
            &api_key,
            requires_api_key,
        );
    }

    Ok(())
}

/// 增量更新 provider 配置；`None` 字段保持原值。
///
/// 与 [`setup_provider`] 的区别：不新增/删除模型条目，只改已有条目的
/// key、`base_url` 与启用状态，然后重建该 provider 在注册表中的条目。
#[tauri::command]
pub async fn update_provider_config(
    state: State<'_, AppState>,
    provider: String,
    api_key: Option<String>,
    base_url: Option<String>,
    enabled_models: Option<Vec<String>>,
) -> Result<()> {
    let mut config = state.config.lock().await;

    // 有新 key 时先写 keychain。
    let mut keychain_failed = false;
    if let Some(key) = &api_key {
        if !key.is_empty() {
            for model in &config.models {
                if model.provider.to_string() == provider
                    && keychain::store_api_key(&model.id, key).is_err() {
                        keychain_failed = true;
                    }
            }
        }
    }

    for model in &mut config.models {
        if model.provider.to_string() == provider {
            if let Some(key) = &api_key {
                if !key.is_empty() {
                    // keychain 失败时把 key 落到配置里。
                    if keychain_failed {
                        model.api_key = key.clone();
                    } else {
                        model.api_key = String::new(); // 已存进 keychain
                    }
                }
            }
            if let Some(url) = &base_url {
                model.base_url = Some(url.clone());
            }
            if let Some(models) = &enabled_models {
                model.enabled = models.contains(&model.id);
            }
        }
    }
    config.save()?;

    let mut providers = state.providers.lock().await;
    for model in &config.models {
        if model.provider.to_string() == provider {
            providers.remove_model(&model.id);
            if model.enabled {
                let resolved = keychain::resolve_api_key(&model.id, &model.api_key);
                let needs_key = model.provider.to_string() != "ollama"
                    && model.provider.to_string() != "lmstudio";
                if !needs_key || !resolved.is_empty() {
                    let mut cfg = model.clone();
                    cfg.api_key = resolved;
                    providers.add_model(cfg);
                }
            }
        }
    }

    Ok(())
}

/// 移除一个 provider：删除它名下所有模型、注销运行时条目并清理 keychain。
#[tauri::command]
pub async fn remove_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    let ids: Vec<String> = config.models.iter()
        .filter(|m| m.provider.to_string() == provider)
        .map(|m| m.id.clone())
        .collect();

    config.models.retain(|m| m.provider.to_string() != provider);
    config.save()?;

    let mut providers = state.providers.lock().await;
    for id in &ids {
        providers.remove_model(id);
        let _ = keychain::delete_api_key(id);
    }

    Ok(())
}

/// 列出某 provider 在配置中登记的全部模型（不校验注册状态）。
///
/// 与 `get_models` 的区别：这里不要求模型已在运行时注册，因此可能包含
/// 尚未配置好 key 的条目。
#[tauri::command]
pub async fn get_provider_models(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Vec<AvailableModel>> {
    let config = state.config.lock().await;
    Ok(config.models.iter()
        .filter(|m| m.provider.to_string() == provider)
        .map(|m| AvailableModel {
            id: m.id.clone(),
            name: m.name.clone(),
            context_window: m.context_window,
        })
        .collect())
}

/// 按 provider 分组返回配置中的全部模型，key 为 provider 字符串。
#[tauri::command]
pub async fn get_available_models(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<AvailableModel>>> {
    let config = state.config.lock().await;
    let mut result = HashMap::new();
    for model in &config.models {
        let provider = model.provider.to_string();
        result.entry(provider).or_insert_with(Vec::new).push(AvailableModel {
            id: model.id.clone(),
            name: model.name.clone(),
            context_window: model.context_window,
        });
    }
    Ok(result)
}

// ── 内部辅助 ──

/// 把 `enabled_models` 中尚不存在的模型补进配置。
///
/// 模型 id 直接作为 API 模型名；`api_key` 留空，由后续 keychain 步骤填充。
fn ensure_models_configured(
    config: &mut AppConfig,
    provider_enum: &ModelProvider,
    enabled_models: &[String],
    base_url: &Option<String>,
) {
    for model_id in enabled_models {
        if config.get_model(model_id).is_none() {
            let model = ModelConfig {
                id: model_id.clone(),
                name: model_id.clone(),       // 用 model_id 作为 API 模型名
                display_name: model_id.clone(),
                provider: provider_enum.clone(),
                api_key: String::new(), // 存 keychain，不落明文
                base_url: base_url.clone(),
                is_default: false,
                enabled: true,
                context_window: None,
                max_tokens: None,
            };
            config.add_model(model);
        }
    }
}

/// 删除同 provider 下不在启用列表中的模型。
fn retain_enabled_models(config: &mut AppConfig, provider: &str, enabled_models: &[String]) {
    config.models.retain(|m| m.provider.to_string() != provider || enabled_models.contains(&m.id));
}

/// 把 key 写入 keychain；任一模型写入失败即返回 `false`。
fn store_api_keys(enabled_models: &[String], api_key: &str) -> bool {
    let mut all_ok = true;
    for model_id in enabled_models {
        if keychain::store_api_key(model_id, api_key).is_err() {
            all_ok = false;
        }
    }
    all_ok
}

/// keychain 不可用时，把 key 回落到 `config.json` 并落盘。
fn fallback_store_api_key(config: &mut AppConfig, provider: &str, api_key: &str) -> Result<()> {
    for model in &mut config.models {
        if model.provider.to_string() == provider {
            model.api_key = api_key.to_string();
        }
    }
    config.save()
}

/// 从注册表中移除该 provider 下不在启用列表里的模型。
fn prune_registry(
    providers: &mut ProviderRegistry,
    config: &AppConfig,
    provider: &str,
    enabled_models: &[String],
) {
    for model_id in config.models.iter()
        .filter(|m| m.provider.to_string() == provider && !enabled_models.contains(&m.id))
        .map(|m| m.id.clone())
        .collect::<Vec<_>>()
    {
        providers.remove_model(&model_id);
    }
}

/// 用最新配置重建该 provider 在注册表中的模型条目。
///
/// 直接把 key 传给注册表，绕过可能不可靠的 keychain（mock/内存实现）。
fn register_models(
    providers: &mut ProviderRegistry,
    config: &AppConfig,
    enabled_models: &[String],
    api_key: &str,
    requires_api_key: bool,
) {
    for model_id in enabled_models {
        if let Some(mut cfg) = config.get_model(model_id).cloned() {
            cfg.api_key = if requires_api_key { api_key.to_string() } else { String::new() };
            providers.add_model(cfg);
        }
    }
}
