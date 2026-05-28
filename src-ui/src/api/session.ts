import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Session, SessionSummary, Message, StreamChunk, PlanDetail } from '../types';

export async function createSession(
  title: string,
  modelId: string,
  systemPrompt?: string,
  personaId?: string,
): Promise<Session> {
  return invoke('create_session', { title, modelId, systemPrompt, personaId });
}

export async function listSessions(includeArchived?: boolean): Promise<Session[]> {
  return invoke('list_sessions', { includeArchived });
}

export async function getSession(id: string): Promise<Session | null> {
  return invoke('get_session', { id });
}

export async function deleteSession(id: string): Promise<void> {
  return invoke('delete_session', { id });
}

export async function updateSessionTitle(id: string, title: string): Promise<void> {
  return invoke('update_session_title', { id, title });
}

export async function updateSessionModel(id: string, modelId: string): Promise<void> {
  return invoke('update_session_model', { id, modelId });
}

export async function clearSession(sessionId: string): Promise<void> {
  return invoke('clear_session', { sessionId });
}

export async function updateSessionConfig(id: string, config: string): Promise<void> {
  return invoke('update_session_config', { id, config });
}

export async function sendMessage(sessionId: string, content: string, toolsEnabled?: boolean, activePersonaId?: string): Promise<Message> {
  return invoke('send_message', { sessionId, content, toolsEnabled, activePersonaId: activePersonaId ?? null });
}

export async function sendMessageStream(
  sessionId: string,
  content: string,
  onChunk: (chunk: StreamChunk) => void,
  toolsEnabled?: boolean,
  activePersonaId?: string,
): Promise<string> {
  const unlisten = await listen<StreamChunk>('stream_chunk', (event) => {
    onChunk(event.payload);
  });

  try {
    const result = await invoke<string>('send_message_stream', {
      sessionId,
      content,
      toolsEnabled,
      activePersonaId: activePersonaId ?? null,
    });
    return result;
  } finally {
    unlisten();
  }
}

export async function getMessages(sessionId: string): Promise<Message[]> {
  return invoke('get_messages', { sessionId });
}

export async function getRequestContext(sessionId: string): Promise<string | null> {
  return invoke('get_request_context', { sessionId });
}

export async function getSessionSummaries(sessionId: string): Promise<SessionSummary[]> {
  return invoke('get_session_summaries', { sessionId });
}

export async function forceGenerateSummary(sessionId: string): Promise<void> {
  return invoke('force_generate_summary', { sessionId });
}

export async function archiveSession(id: string): Promise<void> {
  return invoke('archive_session', { id });
}

export async function unarchiveSession(id: string): Promise<void> {
  return invoke('unarchive_session', { id });
}

// ── Execution Commands ──

export async function executePlan(sessionId: string, planJson: string): Promise<void> {
  return invoke('execute_plan', { sessionId, planJson });
}

export async function pauseExecution(sessionId: string): Promise<void> {
  return invoke('pause_execution', { sessionId });
}

export async function resumeExecution(sessionId: string): Promise<void> {
  return invoke('resume_execution', { sessionId });
}

export async function cancelExecution(sessionId: string): Promise<void> {
  return invoke('cancel_execution', { sessionId });
}

export async function getExecutionStatus(sessionId: string): Promise<string | null> {
  return invoke('get_execution_status', { sessionId });
}

export async function getPlanDetail(planId: string): Promise<PlanDetail | null> {
  return invoke('get_plan_detail', { planId });
}
