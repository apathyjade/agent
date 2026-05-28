import { invoke } from '@tauri-apps/api/core';
import type { LifecycleConfig, ProviderStatus, ProviderSetupParams, ModelConfig, ToolInfo, SystemPrompt } from '../types';

export async function setWindowPosition(x: number, y: number): Promise<void> {
  return invoke('set_window_position', { x, y });
}

// ── Provider Commands ──

export async function listProviders(): Promise<ProviderStatus[]> {
  return invoke('list_providers_cmd');
}

export async function getProviderModels(provider: string): Promise<Array<{ id: string; name: string; context_window?: number }>> {
  return invoke('get_provider_models', { provider });
}

export async function setupProvider(params: ProviderSetupParams): Promise<void> {
  return invoke('setup_provider', {
    provider: params.provider,
    apiKey: params.apiKey,
    baseUrl: params.baseUrl,
    enabledModels: params.enabledModels,
  });
}

export async function updateProviderConfig(params: {
  provider: string;
  apiKey?: string;
  baseUrl?: string;
  enabledModels?: string[];
}): Promise<void> {
  return invoke('update_provider_config', {
    provider: params.provider,
    apiKey: params.apiKey,
    baseUrl: params.baseUrl,
    enabledModels: params.enabledModels,
  });
}

export async function removeProvider(provider: string): Promise<void> {
  return invoke('remove_provider', { provider });
}

// ── Model Commands ──

export async function getModels(): Promise<ModelConfig[]> {
  return invoke('get_models');
}

export async function getDefaultModel(): Promise<ModelConfig | null> {
  return invoke('get_default_model');
}

export async function setDefaultModel(model: string): Promise<void> {
  return invoke('set_default_model', { id: model });
}

export async function getAvailableModels(): Promise<Record<string, Array<{ id: string; name: string; context_window?: number }>>> {
  return invoke('get_available_models');
}

// ── Settings Commands ──

export async function updateSettings(key: string, value: string): Promise<void> {
  return invoke('update_settings', { key, value });
}

export async function getSettings(): Promise<Record<string, string>> {
  return invoke('get_settings');
}

// ── Tool Commands ──

export async function listTools(): Promise<ToolInfo[]> {
  return invoke('list_tools');
}

export async function toggleTool(name: string, enabled: boolean): Promise<void> {
  return invoke('toggle_tool', { name, enabled });
}

// ── System Prompt Commands ──

export async function createSystemPrompt(name: string, content: string, isDefault: boolean): Promise<SystemPrompt> {
  return invoke('create_system_prompt', { name, content, isDefault });
}

export async function listSystemPrompts(): Promise<SystemPrompt[]> {
  return invoke('list_system_prompts');
}

export async function deleteSystemPrompt(id: string): Promise<void> {
  return invoke('delete_system_prompt', { id });
}

export async function setDefaultSystemPrompt(id: string): Promise<void> {
  return invoke('set_default_system_prompt', { id });
}

export async function getDefaultSystemPrompt(): Promise<SystemPrompt | null> {
  return invoke('get_default_system_prompt');
}

// ── Lifecycle Commands ──

export async function getLifecycleConfig(): Promise<LifecycleConfig> {
  return invoke('get_lifecycle_config');
}

export async function updateLifecycleConfig(config: LifecycleConfig): Promise<void> {
  return invoke('update_lifecycle_config', { config });
}
