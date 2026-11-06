//! 模型配置 IPC 命令。
//!
//! 模型列表是 `AppConfig` 的一部分，落盘在 `config.json`。注意：
//! `get_models` 只返回 provider 注册表里真正可用的模型，而不是配置里的全部
//! 条目——没有 API key 的旧默认模型不应出现在下拉框里。

use tauri::State;

use crate::llm::model::ModelConfig;
use crate::app::commands::ModelInfo;
use crate::core::error::{AppError, Result};
use crate::app::state::AppState;

/// 列出可用模型（仅限 provider 注册表中已注册的）。
///
/// 注册意味着「有有效 API key 或本身无需 key」，因此这条命令天然过滤掉了
/// 配置里残留的失效默认模型。
#[tauri::command]
pub async fn get_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>> {
    let config = state.config.lock().await;
    let providers = state.providers.lock().await;
    // 只返回真正注册进 provider 注册表的模型（有有效 API key / 配置完整）。
    // 这样可以避免残留的失效默认模型（没有 api_key 的 default-openai）
    // 出现在下拉框里。
    let models: Vec<ModelInfo> = config.models.iter()
        .filter(|m| providers.is_registered(&m.id))
        .map(ModelInfo::from)
        .collect();
    Ok(models)
}

/// 直接向配置追加一个模型。
///
/// 与 `setup_provider` 的区别：这里不做 provider 级联、不写 keychain、也不
/// 注册进运行时 provider 注册表，只落配置。需要立即可用请走 `setup_provider`
/// 或重启后重新加载。
#[tauri::command]
pub async fn add_model(
    state: State<'_, AppState>,
    id: String,
    name: String,
    display_name: String,
    provider: String,
    api_key: String,
    base_url: Option<String>,
    context_window: Option<u32>,
    max_tokens: Option<u32>,
) -> Result<()> {
    let provider_enum = provider
        .parse()
        .map_err(|e: String| AppError::InvalidInput(e))?;

    let new_model = ModelConfig {
        id: id.clone(),
        name,
        display_name,
        provider: provider_enum,
        api_key,
        base_url,
        is_default: false,
        enabled: true,
        context_window,
        max_tokens,
    };

    let mut config = state.config.lock().await;
    config.models.push(new_model);
    config.save()
}

/// 从配置中移除一个模型（按 id）。
///
/// 只删配置；keychain 里的 key 与 provider 注册表中的条目需要走
/// `remove_provider` 才会清理。
#[tauri::command]
pub async fn remove_model(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut config = state.config.lock().await;
    config.models.retain(|m| m.id != id);
    config.save()
}

/// 部分更新一个模型；`None` 字段保持原值。
///
/// `is_default: Some(true)` 会先把所有模型置为非默认，再设置目标模型；
/// `Some(false)` 只影响目标模型自身。模型不存在时返回 `NotFound`。
#[tauri::command]
pub async fn update_model(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    display_name: Option<String>,
    provider: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    is_default: Option<bool>,
    enabled: Option<bool>,
    context_window: Option<u32>,
    max_tokens: Option<u32>,
) -> Result<()> {
    let mut config = state.config.lock().await;

    // 先处理 is_default：需要遍历整个 vec，不能与下面那条可变借用共存。
    if let Some(true) = is_default {
        for m in config.models.iter_mut() {
            m.is_default = false;
        }
    }

    let model = config
        .models
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::NotFound(format!("Model {} not found", id)))?;

    if let Some(v) = name {
        model.name = v;
    }
    if let Some(v) = display_name {
        model.display_name = v;
    }
    if let Some(v) = provider {
        model.provider = v.parse().map_err(|e: String| AppError::InvalidInput(e))?;
    }
    if let Some(v) = api_key {
        model.api_key = v;
    }
    if let Some(v) = base_url {
        model.base_url = Some(v);
    }
    // is_default: Some(false) 只影响这一个模型。
    if let Some(v) = is_default {
        model.is_default = v;
    }
    if let Some(v) = enabled {
        model.enabled = v;
    }
    if let Some(v) = context_window {
        model.context_window = Some(v);
    }
    if let Some(v) = max_tokens {
        model.max_tokens = Some(v);
    }

    config.save()
}

/// 把指定模型设为唯一默认模型；不存在时返回 `NotFound`。
#[tauri::command]
pub async fn set_default_model(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut config = state.config.lock().await;
    let found = config.set_default_model(&id);
    if !found {
        return Err(AppError::NotFound(format!("Model {} not found", id)));
    }
    config.save()
}

/// 读取当前默认模型；未设置时返回 `Ok(None)`。
///
/// 注意这里不校验 provider 注册状态，可能与 `get_models` 的结果不一致。
#[tauri::command]
pub async fn get_default_model(state: State<'_, AppState>) -> Result<Option<ModelInfo>> {
    let config = state.config.lock().await;
    Ok(config
        .models
        .iter()
        .find(|m| m.is_default)
        .map(ModelInfo::from))
}
