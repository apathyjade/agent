import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Session, SessionSummary, Message, StreamChunk, PlanDetail } from '../types';

export async function createSession(
  title: string,
  modelId: string,
  systemPrompt?: string,
  personaId?: string,
  projectId?: string,
): Promise<Session> {
  return invoke('create_session', { title, modelId, systemPrompt, personaId, projectId });
}

export async function listSessions(includeArchived?: boolean, projectId?: string): Promise<Session[]> {
  return invoke('list_sessions', { includeArchived, projectId });
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
  planId?: string,
  turnId?: string,
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
      planId: planId ?? null,
      turnId: turnId ?? null,
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

export async function executePlan(sessionId: string, planJson: string, turnId?: string): Promise<void> {
  return invoke('execute_plan', { sessionId, planJson, turnId: turnId ?? null });
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

// ── Turn log ──

export type TurnLogStage =
  | 'question'
  | 'understand'
  | 'plan'
  | 'confirm'
  | 'route'
  | 'prepare'
  | 'execute'
  | 'review'
  | 'answer'
  | 'notice';

export type TurnLogLevel = 'info' | 'warn' | 'error';

export interface TurnLogEntry {
  seq: number;
  at: string;
  stage: TurnLogStage;
  level: TurnLogLevel;
  title: string;
  summary: string;
  elapsed_ms: number | null;
  detail?: unknown;
}

export interface TurnLogSummary {
  turn_id: string;
  session_id: string;
  status: 'completed' | 'failed' | 'cancelled';
  path: 'fast' | 'deep' | 'planned' | 'autonomous' | 'unknown';
  question: string;
  model_id: string | null;
  persona_name: string | null;
  started_at: string;
  total_ms: number | null;
  entry_count: number;
}

export type TurnLogRecord = TurnLogSummary & {
  finished_at: string | null;
  entries: TurnLogEntry[];
};

/** 实时推送的 session_log 事件负载（detail 为空）。 */
export interface SessionLogEvent {
  session_id: string;
  turn_id: string;
  entry: TurnLogEntry;
}

/** 回合结束时推送的 session_log_done 事件负载。 */
export interface SessionLogDoneEvent {
  session_id: string;
  turn_id: string;
  status: TurnLogRecord['status'];
}

export async function getSessionTurnLogs(sessionId: string): Promise<TurnLogSummary[]> {
  return invoke('get_session_turn_logs', { sessionId });
}

export async function getSessionTurnLog(turnId: string): Promise<TurnLogRecord | null> {
  return invoke('get_session_turn_log', { turnId });
}
