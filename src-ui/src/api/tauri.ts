import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export async function setWindowPosition(x: number, y: number): Promise<void> {
  return invoke('set_window_position', { x, y });
}


import type { Conversation, Message, ToolInfo, StreamChunk, SystemPrompt, ProviderStatus, ProviderSetupParams, ModelConfig, SkillInfo, SkillDetail, MarketSkill, ReconcileResult } from '../types';

export async function createConversation(
  title: string,
  modelId: string,
  systemPrompt?: string
): Promise<Conversation> {
  return invoke('create_conversation', { title, modelId, systemPrompt });
}

export async function listConversations(): Promise<Conversation[]> {
  return invoke('list_conversations');
}

export async function getConversation(id: string): Promise<Conversation | null> {
  return invoke('get_conversation', { id });
}

export async function deleteConversation(id: string): Promise<void> {
  return invoke('delete_conversation', { id });
}

export async function updateConversationTitle(id: string, title: string): Promise<void> {
  return invoke('update_conversation_title', { id, title });
}

export async function updateConversationModel(id: string, modelId: string): Promise<void> {
  return invoke('update_conversation_model', { id, modelId });
}

export async function clearConversation(conversationId: string): Promise<void> {
  return invoke('clear_conversation', { conversationId });
}

export async function sendMessage(conversationId: string, content: string, toolsEnabled?: boolean): Promise<Message> {
  return invoke('send_message', { conversationId, content, toolsEnabled });
}

export async function sendMessageStream(
  conversationId: string,
  content: string,
  onChunk: (chunk: StreamChunk) => void,
  toolsEnabled?: boolean,
): Promise<string> {
  const unlisten = await listen<StreamChunk>('stream_chunk', (event) => {
    onChunk(event.payload);
  });

  try {
    const result = await invoke<string>('send_message_stream', {
      conversationId,
      content,
      toolsEnabled,
    });
    return result;
  } finally {
    unlisten();
  }
}

export async function getMessages(conversationId: string): Promise<Message[]> {
  return invoke('get_messages', { conversationId });
}

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

export async function updateSettings(key: string, value: string): Promise<void> {
  return invoke('update_settings', { key, value });
}

export async function getSettings(): Promise<Record<string, string>> {
  return invoke('get_settings');
}

export async function listTools(): Promise<ToolInfo[]> {
  return invoke('list_tools');
}

export async function toggleTool(name: string, enabled: boolean): Promise<void> {
  return invoke('toggle_tool', { name, enabled });
}

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

export async function listSkills(): Promise<SkillInfo[]> {
  return invoke('list_skills');
}

export async function getSkillDetail(id: string): Promise<SkillDetail> {
  return invoke('get_skill_detail', { id });
}

export async function installSkillFromPath(path: string): Promise<SkillInfo> {
  return invoke('install_skill_from_path', { path });
}

export async function uninstallSkill(id: string): Promise<void> {
  return invoke('uninstall_skill', { id });
}

export async function toggleSkill(id: string, enabled: boolean): Promise<void> {
  return invoke('toggle_skill', { id, enabled });
}

export async function configureSkill(id: string, config: Record<string, unknown>): Promise<void> {
  return invoke('configure_skill', { id, config });
}

export async function reconcileSkills(): Promise<ReconcileResult> {
  return invoke('reconcile_skills');
}

export async function listMarketTopSkills(limit?: number): Promise<MarketSkill[]> {
  return invoke('list_market_top_skills', { limit });
}

export async function searchMarketSkills(
  query: string,
  limit?: number,
): Promise<MarketSkill[]> {
  return invoke('search_market_skills', { query, limit });
}

export async function installMarketSkill(source: string): Promise<string> {
  return invoke('install_market_skill', { source });
}
