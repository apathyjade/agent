import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export async function setWindowPosition(x: number, y: number): Promise<void> {
  return invoke('set_window_position', { x, y });
}


import type { Conversation, Message, ToolInfo, StreamChunk, SystemPrompt, ProviderStatus, ProviderSetupParams, ModelConfig, SkillInfo, SkillDetail, MarketSkill, ReconcileResult, McpConnectionInfo, ConnectionStats, WorkflowInfo, WorkflowRunRecord, RuntimeInfo, RuntimeSuggestion, RuntimeVersion, InstalledVersion, BoundProject, ProjectScanResult, SyncResult, VersionUpdate, PathConflict, BatchInstallItem, BatchInstallResult, DiskUsageItem, VersionManager, MemoryInfo, CreateMemoryParams, UpdateMemoryParams, PersonaInfo, CreatePersonaParams, UpdatePersonaParams, ResolveResult } from '../types';

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

export async function getRequestContext(conversationId: string): Promise<string | null> {
  return invoke('get_request_context', { conversationId });
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

// ── Pipeline / Workflow Commands ──

export async function listWorkflows(): Promise<WorkflowInfo[]> {
  return invoke('list_workflows');
}

export async function runWorkflow(name: string): Promise<string> {
  return invoke('run_workflow', { name });
}

export async function listWorkflowRuns(): Promise<WorkflowRunRecord[]> {
  return invoke('list_workflow_runs');
}

export async function setWorkflowVar(key: string, value: string): Promise<void> {
  return invoke('set_workflow_var', { key, value });
}

export async function deleteWorkflowVar(key: string): Promise<void> {
  return invoke('delete_workflow_var', { key });
}

export async function listWorkflowVars(): Promise<Record<string, string>> {
  return invoke('list_workflow_vars');
}

export async function setWorkflowSecret(key: string, value: string): Promise<void> {
  return invoke('set_workflow_secret', { key, value });
}

export async function deleteWorkflowSecret(key: string): Promise<void> {
  return invoke('delete_workflow_secret', { key });
}

export async function listWorkflowSecrets(): Promise<string[]> {
  return invoke('list_workflow_secrets');
}

export async function generateWorkflow(description: string): Promise<string> {
  return invoke('generate_workflow', { description });
}

export async function pauseWorkflowSchedule(name: string): Promise<void> {
  return invoke('pause_workflow_schedule', { name });
}

export async function resumeWorkflowSchedule(name: string): Promise<void> {
  return invoke('resume_workflow_schedule', { name });
}

export async function getWorkflowRunDetail(id: string): Promise<WorkflowRunRecord> {
  return invoke('get_workflow_run_detail', { id });
}

export async function installMarketSkill(source: string): Promise<string> {
  return invoke('install_market_skill', { source });
}

// ── MCP Server Commands ──

export async function listMcpConnections(): Promise<McpConnectionInfo[]> {
  return invoke('list_mcp_connections');
}

export async function addMcpServer(name: string, command: string, args: string[], runtime?: string): Promise<McpConnectionInfo> {
  return invoke('add_mcp_server', { name, command, args, runtime: runtime ?? '' });
}

export async function removeMcpServer(id: string): Promise<void> {
  return invoke('remove_mcp_server', { id });
}

export async function connectMcpServer(id: string): Promise<void> {
  return invoke('connect_mcp_server', { id });
}

export async function disconnectMcpServer(id: string): Promise<void> {
  return invoke('disconnect_mcp_server', { id });
}

export async function updateMcpToolConfig(
  connectionId: string,
  toolName: string,
  enabled: boolean,
  confirmation: string,
): Promise<void> {
  return invoke('update_mcp_tool_config', { connectionId, toolName, enabled, confirmation });
}

export async function getMcpConnectionStats(id: string): Promise<ConnectionStats> {
  return invoke('get_mcp_connection_stats', { id });
}

export async function restartMcpServer(id: string): Promise<void> {
  return invoke('restart_mcp_server', { id });
}

export async function getMcpServerLogs(id: string): Promise<string[]> {
  return invoke('get_mcp_server_logs', { id });
}

export async function updateMcpStartupPolicy(
  id: string,
  options: {
    launchOnStartup?: boolean;
    launchOnDemand?: boolean;
    priority?: number;
    maxRetries?: number;
    healthCheckIntervalMs?: number;
  },
): Promise<void> {
  return invoke('update_mcp_startup_policy', { id, ...options });
}

// ── Runtime Environment Commands ──

export async function listRuntimes(): Promise<RuntimeInfo[]> {
  return invoke('list_runtimes');
}

export async function getCachedRuntimes(): Promise<RuntimeInfo[]> {
  return invoke('get_cached_runtimes');
}

export async function validateRuntime(runtimeType: string): Promise<string> {
  return invoke('validate_runtime', { runtimeType });
}

export async function installRuntime(runtimeType: string, version?: string): Promise<RuntimeInfo> {
  return invoke('install_runtime', { runtimeType, version: version ?? null });
}

export async function refreshRuntime(runtimeType: string): Promise<RuntimeInfo> {
  return invoke('refresh_runtime', { runtimeType });
}

export async function suggestRuntimeForCommand(command: string): Promise<RuntimeSuggestion> {
  return invoke('suggest_runtime_for_command', { command });
}

// ── Version Management ──

export async function listAvailableVersions(runtimeType: string): Promise<RuntimeVersion[]> {
  return invoke('list_available_versions', { runtimeType });
}

export async function listInstalledVersions(runtimeType: string): Promise<InstalledVersion[]> {
  return invoke('list_installed_versions', { runtimeType });
}

export async function switchRuntimeVersion(runtimeType: string, version: string): Promise<RuntimeInfo> {
  return invoke('switch_runtime_version', { runtimeType, version });
}

export async function uninstallRuntimeVersion(runtimeType: string, version: string): Promise<RuntimeInfo> {
  return invoke('uninstall_runtime_version', { runtimeType, version });
}

// ── Install Directory ──

export async function getRuntimeInstallDir(): Promise<string> {
  return invoke('get_runtime_install_dir');
}

export async function openVersionDirectory(runtimeType: string, version: string): Promise<string> {
  return invoke('open_version_directory', { runtimeType, version });
}

export async function setRuntimeInstallDir(dir: string): Promise<string> {
  return invoke('set_runtime_install_dir', { dir });
}

export async function refreshVersionCache(runtimeType: string): Promise<RuntimeVersion[]> {
  return invoke('refresh_version_cache', { runtimeType });
}

// ── Project Binding Commands ──

export async function scanProject(path: string): Promise<ProjectScanResult> {
  return invoke('scan_project', { path });
}

export async function addBoundProject(path: string): Promise<BoundProject> {
  return invoke('add_bound_project', { path });
}

export async function listBoundProjects(): Promise<BoundProject[]> {
  return invoke('list_bound_projects');
}

export async function removeBoundProject(id: string): Promise<void> {
  return invoke('remove_bound_project', { id });
}

export async function syncProject(id: string): Promise<SyncResult> {
  return invoke('sync_project', { id });
}

// ── Alias & Version Commands ──

export async function setRuntimeDefault(runtimeType: string, version: string): Promise<void> {
  return invoke('set_runtime_default', { runtimeType, version });
}

export async function getRuntimeDefault(runtimeType: string): Promise<string | null> {
  return invoke('get_runtime_default', { runtimeType });
}

export async function resolveVersion(runtimeType: string, versionSpec: string): Promise<string> {
  return invoke('resolve_version', { runtimeType, versionSpec });
}

// ── Upgrade Check ──

export async function checkRuntimeUpdates(): Promise<VersionUpdate[]> {
  return invoke('check_runtime_updates');
}

// ── PATH Conflict Detection ──

export async function detectPathConflicts(): Promise<PathConflict[]> {
  return invoke('detect_path_conflicts');
}

// ── Batch Install ──

export async function batchInstallRuntimes(installs: BatchInstallItem[]): Promise<BatchInstallResult[]> {
  return invoke('batch_install_runtimes', { installs });
}

// ── Version Manager Commands ──

export async function getVersionManagers(runtimeType: string): Promise<VersionManager[]> {
  return invoke('get_version_managers', { runtimeType });
}

export async function setActiveManager(runtimeType: string, managerId: string): Promise<void> {
  return invoke('set_active_manager', { runtimeType, managerId });
}

export async function getActiveManager(runtimeType: string): Promise<string | null> {
  return invoke('get_active_manager', { runtimeType });
}

export async function installManagerTool(managerId: string, downloadUrl: string): Promise<string> {
  return invoke('install_manager_tool', { managerId, downloadUrl });
}

// ── Disk Usage ──

export async function getRuntimesDiskUsage(): Promise<DiskUsageItem[]> {
  return invoke('get_runtime_disk_usage');
}

// ── Persona Commands ──

export async function createPersona(params: CreatePersonaParams): Promise<PersonaInfo> {
  return invoke('create_persona', { params });
}

export async function listPersonas(): Promise<PersonaInfo[]> {
  return invoke('list_personas');
}

export async function getPersona(id: string): Promise<PersonaInfo> {
  return invoke('get_persona', { id });
}

export async function updatePersona(id: string, params: UpdatePersonaParams): Promise<PersonaInfo> {
  return invoke('update_persona', { id, params });
}

export async function deletePersona(id: string): Promise<void> {
  return invoke('delete_persona', { id });
}

export async function resolvePersona(
  message: string,
  projectPath?: string,
  activePersonaId?: string,
): Promise<ResolveResult> {
  return invoke('resolve_persona', {
    message,
    projectPath: projectPath ?? null,
    activePersonaId: activePersonaId ?? null,
  });
}

export async function linkMemoryToPersona(personaId: string, memoryId: string): Promise<void> {
  return invoke('link_memory_to_persona', { personaId, memoryId });
}

export async function unlinkMemoryFromPersona(personaId: string, memoryId: string): Promise<void> {
  return invoke('unlink_memory_from_persona', { personaId, memoryId });
}

export async function getPersonaMemories(personaId: string): Promise<MemoryInfo[]> {
  return invoke('get_persona_memories', { personaId });
}

export async function bindPersonaProject(personaId: string, projectPath: string, autoSelect?: boolean): Promise<void> {
  return invoke('bind_persona_project', { personaId, projectPath, autoSelect: autoSelect ?? null });
}

export async function unbindPersonaProject(personaId: string, projectPath: string): Promise<void> {
  return invoke('unbind_persona_project', { personaId, projectPath });
}

// ── Memory Commands ──

export async function createMemory(params: CreateMemoryParams): Promise<MemoryInfo> {
  return invoke('create_memory', { params });
}

export async function listMemories(): Promise<MemoryInfo[]> {
  return invoke('list_memories');
}

export async function getMemory(id: string): Promise<MemoryInfo> {
  return invoke('get_memory', { id });
}

export async function searchMemories(
  query: string,
  memoryType?: string,
  scope?: string,
): Promise<MemoryInfo[]> {
  return invoke('search_memories', { query, memoryType: memoryType ?? null, scope: scope ?? null });
}

export async function updateMemory(id: string, params: UpdateMemoryParams): Promise<MemoryInfo> {
  return invoke('update_memory', { id, params });
}

export async function deleteMemory(id: string): Promise<void> {
  return invoke('delete_memory', { id });
}
