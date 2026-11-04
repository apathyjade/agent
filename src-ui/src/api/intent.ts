import { invoke } from '@tauri-apps/api/core';

// ── Intent Types ──

/** A domain of intents. Wire shape mirrors the backend `IntentDomainInfo`. */
export interface IntentDomainInfo {
  id: string;
  name: string;
  description: string;
  is_active: boolean;
  source: string;
  version: number;
  created_at: string;
  updated_at: string;
}

/** An intent. Wire shape mirrors the backend `IntentInfo`; arrays are real. */
export interface IntentInfo {
  id: string;
  domain_id: string;
  name: string;
  description: string;
  examples: string[];
  workflow: string;
  allowed_tools: string[];
  default_agent: string;
  scope: string;
  enabled: boolean;
  version: number;
  confidence_threshold: number;
  fallback_intent: string;
  path: string;
  system_prompt_appendix: string;
  model_id: string;
  max_iterations: number | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

/** Session-level intent domain state returned by the backend. */
export interface SessionIntentState {
  domain_id: string | null;
  domain_name: string | null;
  mode: 'auto' | 'pinned';
}

/** Payload of the `intent_resolved` Tauri event. */
export interface IntentResolvedEvent {
  session_id: string;
  domain_id: string;
  domain_name: string;
  intent_id: string;
  intent_name: string;
  confidence: number;
  source: 'llm' | 'scorer' | 'fallback';
  widened: boolean;
}

/**
 * Editable intent fields. Every field is optional: absent on create means the
 * schema default, absent on update means "keep the stored value". To clear a
 * string send `""`, to clear a list send `[]`, to clear `max_iterations` send
 * `null`.
 */
export type IntentInput = Partial<
  Omit<IntentInfo, 'id' | 'domain_id' | 'version' | 'created_at' | 'updated_at'>
>;

// ── Intent Domain Commands ──

export async function listIntentDomains(): Promise<IntentDomainInfo[]> {
  return invoke('list_intent_domains');
}

export async function getActiveIntentDomain(): Promise<IntentDomainInfo | null> {
  return invoke('get_active_intent_domain');
}

export async function createIntentDomain(
  name: string,
  description: string,
): Promise<IntentDomainInfo> {
  return invoke('create_intent_domain', { name, description });
}

export async function updateIntentDomain(
  id: string,
  name: string,
  description: string,
): Promise<IntentDomainInfo> {
  return invoke('update_intent_domain', { id, name, description });
}

export async function deleteIntentDomain(id: string): Promise<void> {
  return invoke('delete_intent_domain', { id });
}

export async function setActiveIntentDomain(id: string): Promise<void> {
  return invoke('set_active_intent_domain', { id });
}

// ── Intent Commands ──

export async function listIntents(domainId: string): Promise<IntentInfo[]> {
  return invoke('list_intents', { domainId });
}

export async function getIntent(id: string): Promise<IntentInfo> {
  return invoke('get_intent', { id });
}

export async function createIntent(
  domainId: string,
  intent: IntentInput,
): Promise<IntentInfo> {
  return invoke('create_intent', { domainId, intent });
}

export async function updateIntent(id: string, intent: IntentInput): Promise<IntentInfo> {
  return invoke('update_intent', { id, intent });
}

export async function deleteIntent(id: string): Promise<void> {
  return invoke('delete_intent', { id });
}

export async function setIntentEnabled(id: string, enabled: boolean): Promise<void> {
  return invoke('set_intent_enabled', { id, enabled });
}

// ── Intent Import / Export Commands ──

export async function importIntentDomain(
  json: string,
  name: string,
): Promise<IntentDomainInfo> {
  return invoke('import_intent_domain', { json, name });
}

export async function exportIntentDomain(domainId: string): Promise<string> {
  return invoke('export_intent_domain', { domainId });
}

// ── Session Intent Domain Commands ──

export async function getSessionIntentState(sessionId: string): Promise<SessionIntentState> {
  return invoke('get_session_intent_state', { sessionId });
}

export async function setSessionIntentDomain(
  sessionId: string,
  domainId: string | null,
): Promise<SessionIntentState> {
  return invoke('set_session_intent_domain', { sessionId, domainId });
}
