import { create } from 'zustand';
import type { StreamChunk, Message, PersonaInfo } from '../types';
import * as api from '../api/tauri';

import { type UISlice, createUISlice } from './uiSlice';
import { type SessionSlice, createSessionSlice } from './sessionSlice';
import { type ModelSlice, createModelSlice } from './modelSlice';
import { type ToolSlice, createToolSlice } from './toolSlice';
import { type PromptSlice, createPromptSlice } from './promptSlice';
import { type SkillSlice, createSkillSlice } from './skillSlice';
import { type McpSlice, createMcpSlice } from './mcpSlice';
import { type MemorySlice, createMemorySlice } from './memorySlice';
import { type PersonaSlice, createPersonaSlice } from './personaSlice';
import { type RuntimeSlice, createRuntimeSlice } from './runtimeSlice';
import { type WorkflowSlice, createWorkflowSlice } from './workflowSlice';

export type { Toast } from './uiSlice';

// Cross-slice method for streaming (spans messages + UI state)
export interface StreamSlice {
  sendMessageStream: (content: string, toolsEnabled?: boolean, activePersonaId?: string) => Promise<void>;
}

export type AppState = UISlice & SessionSlice & ModelSlice & ToolSlice & PromptSlice & SkillSlice & McpSlice & MemorySlice & PersonaSlice & RuntimeSlice & WorkflowSlice & StreamSlice & {
  activePersonaId: string | null;
  activePersonaInfo: PersonaInfo | null;
  setActivePersona: (info: PersonaInfo | null) => void;
};

export const useStore = create<AppState>()((set, get, store) => ({
  ...createUISlice(set, get, store),
  ...createSessionSlice(set, get, store),
  ...createModelSlice(set, get, store),
  ...createToolSlice(set, get, store),
  ...createPromptSlice(set, get, store),
  ...createSkillSlice(set, get, store),
  ...createMcpSlice(set, get, store),
  ...createMemorySlice(set, get, store),
  ...createPersonaSlice(set, get, store),
  ...createRuntimeSlice(set, get, store),
  ...createWorkflowSlice(set, get, store),

  activePersonaId: null,
  activePersonaInfo: null,
  setActivePersona: (info: PersonaInfo | null) => {
    set({
      activePersonaId: info?.id ?? null,
      activePersonaInfo: info,
    });
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
          }
          if (chunk.done) {
            const s = get();
            const assistantMsg: Message = {
              id: Date.now().toString(),
              session_id: currentSession.id,
              role: 'assistant',
              content: s.streamingContent,
              created_at: new Date().toISOString(),
            };
            set((state) => ({
              messages: [...state.messages, assistantMsg],
              loading: false,
              isStreaming: false,
              streamingContent: '',
            }));
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
