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
