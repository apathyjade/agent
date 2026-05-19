use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::config::{ModelConfig, ModelProvider};
use crate::error::{AppError, Result};
use crate::keychain;
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
    let provider_registry = state.providers.lock().await;
    let registered_ids = provider_registry.get_registered_model_ids();
    drop(provider_registry);

    let providers = crate::commands::PROVIDER_OPTIONS.iter().map(|(id, name, base_url, default_model)| {
        let models: Vec<&ModelConfig> = config.models.iter()
            .filter(|m| m.provider.to_string() == *id)
            .collect();
        let requires_key = *id != "ollama" && *id != "lmstudio";
        let has_configured = models.iter().any(|m| {
            if !m.enabled { return false; }
            // Check if model is actually registered in the provider registry
            // (means it has valid API key and was successfully initialized)
            registered_ids.contains(&m.id)
        });
        let available_models: Vec<AvailableModel> = if models.is_empty() || !has_configured {
            // No actively-configured models for this provider;
            // show the full known model list from PROVIDER_MODELS so the user can pick.
            crate::commands::PROVIDER_MODELS.iter()
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
                name: model_id.clone(),       // use model_id as the API model name
                display_name: model_id.clone(),
                provider: provider_enum.clone(),
                api_key: String::new(), // stored in keychain, not plaintext
                base_url: base_url.clone(),
                is_default: false,
                enabled: true,
                context_window: None,
                max_tokens: None,
            };
            config.add_model(model);
        }
    }

    // Remove old models from same provider that aren't in enabled_models
    // (e.g., old "default-openai" that has no configured API key)
    config.models.retain(|m| m.provider.to_string() != provider || enabled_models.contains(&m.id));

    // Set the first enabled model as the new default
    if let Some(first) = enabled_models.first() {
        if config.get_model(first).is_some() {
            config.set_default_model(first);
        }
    }
    config.save()?;

    // Remove any stale providers from the registry (old default models, etc.)
    let mut providers = state.providers.lock().await;
    for model_id in config.models.iter()
        .filter(|m| m.provider.to_string() == provider && !enabled_models.contains(&m.id))
        .map(|m| m.id.clone())
        .collect::<Vec<_>>()
    {
        providers.remove_model(&model_id);
    }
    drop(providers);

    // Store API key in OS keychain
    let requires_api_key = provider != "ollama" && provider != "lmstudio";
    if requires_api_key && !api_key.is_empty() {
        for model_id in &enabled_models {
            let _ = keychain::store_api_key(model_id, &api_key);
        }
    }

    let mut providers = state.providers.lock().await;
    if !requires_api_key || !api_key.is_empty() {
        for model_id in &enabled_models {
            if let Some(mut cfg) = config.get_model(model_id).cloned() {
                // Pass the API key directly to the provider registry,
                // bypassing keychain (which may be mock/in-memory and unreliable).
                cfg.api_key = if requires_api_key { api_key.clone() } else { String::new() };
                providers.add_model(cfg);
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

    // Update keychain first if a new API key is provided
    if let Some(key) = &api_key {
        if !key.is_empty() {
            for model in &config.models {
                if model.provider.to_string() == provider {
                    let _ = keychain::store_api_key(&model.id, key);
                }
            }
        }
    }

    for model in &mut config.models {
        if model.provider.to_string() == provider {
            if let Some(key) = &api_key {
                if !key.is_empty() {
                    model.api_key = String::new(); // stored in keychain
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
        let _ = keychain::delete_api_key(&id);
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



