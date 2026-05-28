import { invoke } from '@tauri-apps/api/core';
import type {
  SkillInfo, SkillDetail, MarketSkill, ReconcileResult,
  McpConnectionInfo, ConnectionStats,
  WorkflowInfo, WorkflowRunRecord,
  RuntimeInfo, RuntimeSuggestion, RuntimeVersion, InstalledVersion,
  BoundProject, ProjectScanResult, SyncResult,
  VersionUpdate, PathConflict, BatchInstallItem, BatchInstallResult,
  DiskUsageItem, VersionManager,
  MemoryInfo, CreateMemoryParams, UpdateMemoryParams,
  PersonaInfo, CreatePersonaParams, UpdatePersonaParams, ResolveResult,
  Project, Session,
} from '../types';

// ── Skill Commands ──

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

export async function searchMarketSkills(query: string, limit?: number): Promise<MarketSkill[]> {
  return invoke('search_market_skills', { query, limit });
}

export async function installMarketSkill(source: string): Promise<string> {
  return invoke('install_market_skill', { source });
}

// ── Workflow Commands ──

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

// ── MCP Commands ──

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

// ── Runtime Commands ──

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

export async function setRuntimeDefault(runtimeType: string, version: string): Promise<void> {
  return invoke('set_runtime_default', { runtimeType, version });
}

export async function getRuntimeDefault(runtimeType: string): Promise<string | null> {
  return invoke('get_runtime_default', { runtimeType });
}

export async function resolveVersion(runtimeType: string, versionSpec: string): Promise<string> {
  return invoke('resolve_version', { runtimeType, versionSpec });
}

export async function checkRuntimeUpdates(): Promise<VersionUpdate[]> {
  return invoke('check_runtime_updates');
}

export async function detectPathConflicts(): Promise<PathConflict[]> {
  return invoke('detect_path_conflicts');
}

export async function batchInstallRuntimes(installs: BatchInstallItem[]): Promise<BatchInstallResult[]> {
  return invoke('batch_install_runtimes', { installs });
}

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

// ── Project Commands ──

export async function listProjects(): Promise<Project[]> {
  return invoke('list_projects');
}

export async function createProject(name: string, path: string): Promise<Project> {
  return invoke('create_project', { name, path });
}

export async function deleteProject(id: string): Promise<void> {
  return invoke('delete_project', { id });
}

export async function getProjectSessions(projectId: string): Promise<Session[]> {
  return invoke('get_project_sessions', { projectId });
}
