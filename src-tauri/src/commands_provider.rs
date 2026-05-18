use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::config::{ModelConfig, ModelProvider};
use crate::error::{AppError, Result};
use crate::state::AppState;

#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub id: String,
    pub name: String,
    pub configured: bool,
    pub base_url: Option<String>,
    pub enabled_models: Vec<String>,
    pub available_models: Vec<AvailableModel>,
    pub requires_api_key: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AvailableModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u32>,
}

#[tauri::command]
pub async fn list_providers_cmd(state: State<'_, AppState>) -> Result<Vec<ProviderStatus>> {
    let config = state.config.lock().await;
    let providers = crate::commands::PROVIDER_OPTIONS.iter().map(|(id, name, base_url)| {
        let models: Vec<&ModelConfig> = config.models.iter()
            .filter(|m| m.provider.to_string() == *id)
            .collect();
        let has_configured = models.iter().any(|m| m.enabled && !m.api_key.is_empty());
        let requires_key = *id != "ollama" && *id != "lmstudio";
        let available_models: Vec<AvailableModel> = if models.is_empty() {
            vec![AvailableModel {
                id: id.to_string(),
                name: name.to_string(),
                context_window: None,
            }]
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
            configured: requires_key && has_configured,
            base_url: Some(base_url.to_string()),
            enabled_models: models.iter().filter(|m| m.enabled).map(|m| m.id.clone()).collect(),
            available_models,
            requires_api_key: requires_key,
        }
    }).collect();
    Ok(providers)
}

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

    for model_id in &enabled_models {
        if config.get_model(model_id).is_none() {
            let model = ModelConfig {
                id: model_id.clone(),
                name: model_id.clone(),
                display_name: model_id.clone(),
                provider: provider_enum.clone(),
                api_key: api_key.clone(),
                base_url: base_url.clone(),
                is_default: config.models.is_empty(),
                enabled: true,
                context_window: None,
                max_tokens: None,
            };
            config.add_model(model);
        }
    }
    config.save()?;

    let mut providers = state.providers.lock().await;
    if !api_key.is_empty() {
        for model_id in &enabled_models {
            if let Some(cfg) = config.get_model(model_id) {
                providers.add_model(cfg.clone());
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn update_provider_config(
    state: State<'_, AppState>,
    provider: String,
    api_key: Option<String>,
    base_url: Option<String>,
    enabled_models: Option<Vec<String>>,
) -> Result<()> {
    let mut config = state.config.lock().await;

    for model in &mut config.models {
        if model.provider.to_string() == provider {
            if let Some(key) = &api_key {
                if !key.is_empty() {
                    model.api_key = key.clone();
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
                providers.add_model(model.clone());
            }
        }
    }

    Ok(())
}

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
    }

    Ok(())
}

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
