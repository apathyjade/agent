export interface Conversation {
  id: string;
  title: string;
  model_id: string;
  system_prompt?: string | null;
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: string;
  conversation_id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  tool_calls?: string | null;
  tool_call_id?: string | null;
  tokens?: number | null;
  created_at: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  context_window?: number;
}

export interface ModelConfig {
  id: string;
  name: string;
  display_name: string;
  provider: string;
  api_key: string;
  base_url?: string | null;
  is_default: boolean;
  enabled: boolean;
  context_window?: number | null;
  max_tokens?: number | null;
}

export interface ProviderStatus {
  id: string;
  name: string;
  configured: boolean;
  base_url?: string;
  enabled_models: string[];
  available_models: ModelInfo[];
  requires_api_key: boolean;
}

export interface ProviderSetupParams {
  provider: string;
  apiKey: string;
  baseUrl?: string;
  enabledModels: string[];
}

export interface ToolInfo {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  enabled: boolean;
}

export interface StreamChunk {
  content: string;
  done: boolean;
  tool_calls?: Array<{
    id: string;
    name: string;
    status: string;
    result?: string;
  }>;
}

export interface SystemPrompt {
  id: string;
  name: string;
  content: string;
  is_default: boolean;
  created_at: string;
}

export interface SkillInfo {
  id: string;
  name: string;
  description: string;
  version: string;
  author?: string | null;
  icon?: string | null;
  tags?: string[] | null;
  source: 'local' | 'registry' | 'scanned';
  agent_sources?: string[] | null;
  enabled: boolean;
  installed_at: string;
  updated_at: string;
}

export interface SkillDetail extends SkillInfo {
  config_schema?: Record<string, unknown> | null;
  config?: Record<string, unknown> | null;
  source_path?: string | null;
  entry_type: string;
  entry_value: string;
}

/** Agent source identifier — which AI agent's skill directory this was found in */
export type AgentSource = 'generic' | 'claude-code' | 'opencode' | 'codex' | 'cursor' | 'workspace';

/** A skill from the skills.sh marketplace search API */
export interface MarketSkill {
  id: string;
  name: string;
  /** Human-readable install count (e.g. "1.6M installs") */
  description: string;
  /** GitHub owner/repo source (e.g. "vercel-labs/agent-skills") */
  source: string;
  /** Raw install count number */
  installs: number;
}

/** Key-value pair for workflow variables */
export interface WorkflowVar {
  key: string;
  value: string;
}

/** A discovered workflow definition */
export interface WorkflowInfo {
  name: string;
  description: string;
  step_count: number;
  file_path: string;
  trigger: string;                 // "manual" | "cron: 0 9 * * *"
  next_run_at?: string | null;
  last_run_status?: string | null;
  last_run_at?: string | null;
}

/** A workflow run record from DB */
export interface WorkflowRunRecord {
  id: string;
  workflow_name: string;
  status: string;
  step_results?: string | null;
  step_progress?: string | null;  // JSON string of StepProgress[]
  error?: string | null;
  trigger_type: string;            // NEW
  started_at: string;
  finished_at?: string | null;
}

/** Step execution progress for realtime timeline display */
export interface StepProgress {
  step_id: string;
  status: string;
  duration_ms?: number | null;
  error?: string | null;
  result_summary?: string | null;
}

/** Per-tool configuration from MCP server */
export interface McpToolInfo {
  name: string;
  description: string;
  enabled: boolean;
  confirmation: string; // "auto_allow" | "confirm_once" | "deny"
}

/** Health and usage stats for an MCP connection */
export interface ConnectionStats {
  uptime_seconds: number;
  total_calls: number;
  error_count: number;
  avg_latency_ms: number;
  last_error?: string | null;
}

/** Connection status string from backend */
export type McpConnectionStatus =
  | 'disabled'
  | 'waiting'
  | 'starting'
  | 'ready'
  | 'degraded'
  | 'stopping'
  | 'stopped'
  | 'error';

/** A single stderr log entry from an MCP server */
export interface McpLogEntry {
  timestamp: string;
  level: string;
  message: string;
}

/** MCP server connection info returned from backend */
export interface McpConnectionInfo {
  id: string;
  name: string;
  /** Connection status: "ready" | "starting" | "error" | "stopped" | etc. */
  status: McpConnectionStatus;
  /** Human-readable status detail (e.g. error message) */
  status_detail?: string | null;
  tool_count: number;
  tools: McpToolInfo[];
  stats: ConnectionStats;
  error?: string | null;
}

/** Result of a reconcile scan operation */
export interface ReconcileResult {
  /** Skill IDs that were auto-added (found on disk, missing from DB) */
  added: string[];
  /** Skill IDs that were auto-removed (DB record, but files missing) */
  removed: string[];
}

// ── Runtime Environment Types ──

/** Supported runtime types */
export type RuntimeType = 'node' | 'python' | 'docker' | 'uv' | 'go' | 'rust' | 'java' | 'deno';

/** Where a runtime is installed */
export type RuntimeSource = 'system' | 'built_in' | 'none';

/** A single installed version of a runtime */
export interface InstalledVersion {
  version: string;
  path: string;
  installed_at: string;
  is_active: boolean;
}

/** Detailed info about a detected/installed runtime */
export interface RuntimeInfo {
  runtime_type: RuntimeType;
  display_name: string;
  source: RuntimeSource;
  /** Currently active version string */
  version: string | null;
  /** All locally installed versions */
  installed_versions: InstalledVersion[];
  executable_path: string | null;
  error: string | null;
  available: boolean;
}

/** Progress of a runtime installation */
export interface InstallProgress {
  runtime_type: RuntimeType;
  stage: string;
  progress: number;
  message: string;
}

/** A version available for download */
export interface AvailableVersion {
  version: string;
  display_name: string;
  url: string;
}

/** Runtime suggestion for a CLI command */
export interface RuntimeSuggestion {
  runtime_type: RuntimeType | null;
  available: boolean;
  version: string | null;
  source: RuntimeSource | null;
  display_name: string | null;
  error?: string | null;
}

/** Version lifecycle status */
export type VersionLifecycle = 'latest' | 'lts' | 'active' | 'maintenance' | 'eol';

/** A version available for download with lifecycle info */
export interface RuntimeVersion {
  runtime_type: RuntimeType;
  version: string;
  display_name: string;
  url: string;
  lts: string | null;
  is_stable: boolean;
  release_date: string | null;
  file_size: number | null;
}

/** Upgrade suggestion */
export interface VersionUpdate {
  runtime_type: RuntimeType;
  current_version: string;
  latest_version: string;
  reason: string;
}

/** A project bound to runtime management */
export interface BoundProject {
  id: string;
  path: string;
  name: string;
  auto_sync: boolean;
  last_scan: string | null;
  requirements: ProjectRuntimeRequirement[];
  created_at: string;
  updated_at: string;
}

/** A single runtime requirement for a project */
export interface ProjectRuntimeRequirement {
  runtime_type: RuntimeType;
  version_spec: string;
  source_file: string;
  resolved_version: string | null;
}

/** Result of scanning a project directory */
export interface ProjectScanResult {
  project_path: string;
  project_name: string;
  requirements: ProjectRuntimeRequirement[];
  errors: string[];
}

/** Result of syncing project runtime versions */
export interface SyncResult {
  project_id: string;
  actions: SyncAction[];
  success: boolean;
  error: string | null;
}

/** A single sync action */
export interface SyncAction {
  runtime_type: RuntimeType;
  action: string;
  from_version: string | null;
  to_version: string;
  success: boolean;
}

/** Health check result item */
export interface HealthCheckItem {
  runtime_type: RuntimeType;
  status: 'healthy' | 'warning' | 'error';
  message: string;
  detail: string | null;
}

/** A single executable found on PATH */
export interface FoundExecutable {
  path: string;
  version: string | null;
  is_active: boolean;
}

/** PATH conflict info for a runtime type */
export interface PathConflict {
  runtime_type: RuntimeType;
  executables: FoundExecutable[];
  conflict: boolean;
}

/** Batch install request item */
export interface BatchInstallItem {
  runtime_type: string;
  version: string | null;
}

/** Disk usage for a runtime */
export interface DiskUsageItem {
  runtime_type: RuntimeType;
  display_name: string;
  size_bytes: number;
  installed_count: number;
  active_version: string | null;
}

/** Batch install result */
export interface BatchInstallResult {
  runtime_type: string;
  version: string;
  success: boolean;
  error: string | null;
}

// ── Memory System Types ──

/** A memory entry remembered by the agent */
export interface MemoryInfo {
  id: string;
  content: string;
  memory_type: 'fact' | 'preference' | 'project_context' | 'user_info' | 'conversation_summary';
  scope: string;
  source: string;
  relevance: number;
  tags: string[] | null;
  created_at: string;
  updated_at: string;
  last_accessed_at: string;
  access_count: number;
}

/** Parameters for creating a new memory */
export interface CreateMemoryParams {
  content: string;
  memory_type?: string;
  scope?: string;
  source?: string;
  relevance?: number;
  tags?: string[] | null;
}

/** Parameters for updating a memory */
export interface UpdateMemoryParams {
  content?: string;
  memory_type?: string;
  scope?: string;
  relevance?: number;
  tags?: string[] | null;
}

// ── Persona System Types ──

/** A virtual persona with its own identity, memories, and config */
export interface PersonaInfo {
  id: string;
  name: string;
  title: string;
  emoji: string;
  description: string;
  system_prompt: string;
  temperature: number;
  response_style: string;
  model_provider: string;
  model_name: string;
  is_default: boolean;
  created_at: string;
  updated_at: string;
}

/** Parameters for creating a new persona */
export interface CreatePersonaParams {
  name: string;
  title?: string;
  emoji?: string;
  description?: string;
  system_prompt: string;
  temperature?: number;
  response_style?: string;
  model_provider?: string;
  model_name?: string;
  is_default?: boolean;
}

/** Parameters for updating a persona */
export interface UpdatePersonaParams {
  name?: string;
  title?: string;
  emoji?: string;
  description?: string;
  system_prompt?: string;
  temperature?: number;
  response_style?: string;
  model_provider?: string;
  model_name?: string;
  is_default?: boolean;
}

/// Result of persona resolution
export interface ResolveResult {
  persona: PersonaInfo;
  mode: 'manual' | 'auto' | 'default';
}

/** A version manager tool for a runtime type */
export interface VersionManager {
  id: string;
  display_name: string;
  runtime_type: RuntimeType;
  installed: boolean;
  install_path?: string | null;
  version?: string | null;
  can_install: boolean;
  install_guide?: string | null;
  recommended: boolean;
  install_url?: string | null;
}
