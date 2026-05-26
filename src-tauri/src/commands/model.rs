use tauri::State;

use crate::config::ModelConfig;
use crate::commands::ModelInfo;
use crate::error::{AppError, Result};
use crate::state::AppState;

#[tauri::command]
pub async fn get_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>> {
    let config = state.config.lock().await;
    let providers = state.providers.lock().await;
    // Only return models that are actually registered in the provider registry
    // (have valid API key / are properly configured).
    // This prevents stale defaults (default-openai with no api_key) from
    // showing in the dropdown.
    let models: Vec<ModelInfo> = config.models.iter()
        .filter(|m| providers.is_registered(&m.id))
        .map(|m| ModelInfo::from(m))
        .collect();
    Ok(models)
}

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

#[tauri::command]
pub async fn remove_model(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut config = state.config.lock().await;
    config.models.retain(|m| m.id != id);
    config.save()
}

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

    // Handle is_default before borrowing a specific model (needs full vec iteration)
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
    // is_default: Some(false) just sets it on the specific model
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

#[tauri::command]
pub async fn set_default_model(state: State<'_, AppState>, id: String) -> Result<()> {
    let mut config = state.config.lock().await;
    let found = config.set_default_model(&id);
    if !found {
        return Err(AppError::NotFound(format!("Model {} not found", id)));
    }
    config.save()
}

#[tauri::command]
pub async fn get_default_model(state: State<'_, AppState>) -> Result<Option<ModelInfo>> {
    let config = state.config.lock().await;
    Ok(config
        .models
        .iter()
        .find(|m| m.is_default)
        .map(|m| ModelInfo::from(m)))
}
