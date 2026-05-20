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

/** Result of a reconcile scan operation */
export interface ReconcileResult {
  /** Skill IDs that were auto-added (found on disk, missing from DB) */
  added: string[];
  /** Skill IDs that were auto-removed (DB record, but files missing) */
  removed: string[];
}
