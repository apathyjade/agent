import { create } from 'zustand';
import type { StreamChunk, Message, PersonaInfo, SessionSummary } from '../types';
import * as api from '../api/tauri';

import { type UISlice, createUISlice } from './uiSlice';
import { type SessionSlice, createSessionSlice } from './sessionSlice';
import { type ModelSlice, createModelSlice } from './modelSlice';
import { type SkillSlice, createSkillSlice } from './skillSlice';
import { type McpSlice, createMcpSlice } from './mcpSlice';
import { type MemorySlice, createMemorySlice } from './memorySlice';
import { type PersonaSlice, createPersonaSlice } from './personaSlice';
import { type RuntimeSlice, createRuntimeSlice } from './runtimeSlice';
import { type WorkflowSlice, createWorkflowSlice } from './workflowSlice';
import { type LifecycleSlice, createLifecycleSlice } from './lifecycleSlice';

export type { Toast } from './uiSlice';

// Cross-slice method for streaming (spans messages + UI state)
export interface StreamSlice {
  sendMessageStream: (content: string, toolsEnabled?: boolean, activePersonaId?: string) => Promise<void>;
}

export type AppState = UISlice & SessionSlice & ModelSlice & SkillSlice & McpSlice & MemorySlice & PersonaSlice & RuntimeSlice & WorkflowSlice & LifecycleSlice & StreamSlice & {
  summaries: SessionSummary[];
  fetchSummaries: (sessionId: string) => Promise<void>;
  activePersonaId: string | null;
  activePersonaInfo: PersonaInfo | null;
  setActivePersona: (info: PersonaInfo | null) => void;
  pendingProjectId: string | null;
  setPendingProjectId: (id: string | null) => void;
};

export const useStore = create<AppState>()((set, get, store) => ({
  ...createUISlice(set, get, store),
  ...createSessionSlice(set, get, store),
  ...createModelSlice(set, get, store),
  ...createSkillSlice(set, get, store),
  ...createMcpSlice(set, get, store),
  ...createMemorySlice(set, get, store),
  ...createPersonaSlice(set, get, store),
  ...createRuntimeSlice(set, get, store),
  ...createWorkflowSlice(set, get, store),
  ...createLifecycleSlice(set, get, store),

  summaries: [],

  fetchSummaries: async (sessionId: string) => {
    try {
      const summaries = await api.getSessionSummaries(sessionId);
      set({ summaries });
    } catch (err) {
      console.error('Failed to fetch summaries:', err);
    }
  },

  activePersonaId: null,
  activePersonaInfo: null,
  setActivePersona: (info: PersonaInfo | null) => {
    set({
      activePersonaId: info?.id ?? null,
      activePersonaInfo: info,
    });
  },

  pendingProjectId: null,
  setPendingProjectId: (id: string | null) => {
    set({ pendingProjectId: id });
  },

  // sendMessageStream spans both conversation and UI state
  sendMessageStream: async (content: string, toolsEnabled?: boolean, activePersonaId?: string) => {
    const { currentSession } = get();
    if (!currentSession) return;

    const userMessage: Message = {
      id: Date.now().toString(),
      session_id: currentSession.id,
      role: 'user',
      content,
      created_at: new Date().toISOString(),
    };

    set({
      messages: [...get().messages, userMessage],
      loading: true,
      error: null,
      isStreaming: true,
      streamingContent: '',
      activeToolCalls: [],
    });

    try {
      await api.sendMessageStream(
        currentSession.id,
        content,
        (chunk: StreamChunk) => {
          if (chunk.content) {
            set((state) => ({
              streamingContent: state.streamingContent + chunk.content,
            }));
          }
          if (chunk.tool_calls) {
            set({ activeToolCalls: chunk.tool_calls });

            // Persist completed/failed tool calls as visible messages in the flow
            const { currentSession } = get();
            const finishedToolMsg = chunk.tool_calls
              .filter(tc => tc.status === 'completed' || tc.status === 'failed')
              .map(tc => ({
                id: `tool-${tc.id}`,
                session_id: currentSession?.id || '',
                role: 'tool' as const,
                content: JSON.stringify({
                  name: tc.name,
                  result: tc.result || '',
                  status: tc.status,
                }),
                created_at: new Date().toISOString(),
              }));
            if (finishedToolMsg.length > 0) {
              set((state) => ({
                messages: [...state.messages, ...finishedToolMsg],
              }));
            }
          }
          if (chunk.phase !== undefined) {
            set({ currentPhase: chunk.phase ?? null });
          }
          if (chunk.done) {
            const s = get();
            if (s.streamingContent) {
              const assistantMsg: Message = {
                id: Date.now().toString(),
                session_id: currentSession.id,
                role: 'assistant',
                content: s.streamingContent,
                created_at: new Date().toISOString(),
              };
              set((state) => ({
                messages: [...state.messages, assistantMsg],
              }));
            }
            set({
              loading: false,
              isStreaming: false,
              streamingContent: '',
              activeToolCalls: [],
              currentPhase: null,
            });
            // Refresh summaries after stream completes
            get().fetchSummaries(currentSession.id);
          }
        },
        toolsEnabled,
        activePersonaId,
      );
    } catch (err) {
      set({ error: String(err), loading: false, isStreaming: false });
    }
  },
}));
