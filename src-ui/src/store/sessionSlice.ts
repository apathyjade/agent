import type { StateCreator } from 'zustand';
import type { Session, Message, SessionMode, ExecStatus, ExecutionPlan, PlanProgressEvent, ExecutionLogEntry, ToolInfo, SystemPrompt } from '../types';
import * as api from '../api/tauri';

export interface SessionSlice {
  sessions: Session[];
  currentSession: Session | null;
  messages: Message[];

  // Execution state
  sessionMode: SessionMode;
  executionStatus: ExecStatus;
  activePlan: ExecutionPlan | null;
  planProgress: PlanProgressEvent | null;
  executionLogs: ExecutionLogEntry[];

  fetchSessions: () => Promise<void>;
  createSession: (title: string, modelId: string, systemPrompt?: string, personaId?: string) => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  updateSessionTitle: (id: string, title: string) => Promise<void>;
  updateSessionModel: (id: string, modelId: string) => Promise<void>;
  clearSession: () => Promise<void>;
  newChat: () => void;
  sendMessage: (content: string) => Promise<void>;
  updateSessionConfig: (id: string, config: Record<string, unknown>) => Promise<void>;

  // Execution actions
  setSessionMode: (mode: SessionMode) => void;
  setExecutionStatus: (status: ExecStatus) => void;
  setActivePlan: (plan: ExecutionPlan | null) => void;
  setPlanProgress: (progress: PlanProgressEvent | null) => void;
  addExecutionLog: (entry: ExecutionLogEntry) => void;
  clearExecutionLogs: () => void;
  executePlan: (sessionId: string, plan: ExecutionPlan) => Promise<void>;
  pauseExecution: (sessionId: string) => Promise<void>;
  resumeExecution: (sessionId: string) => Promise<void>;
  cancelExecution: (sessionId: string) => Promise<void>;

  // ── Tools state (merged from toolSlice) ──
  tools: ToolInfo[];
  fetchTools: () => Promise<void>;
  toggleTool: (name: string, enabled: boolean) => Promise<void>;

  // ── System prompts state (merged from promptSlice) ──
  systemPrompts: SystemPrompt[];
  fetchSystemPrompts: () => Promise<void>;
  createSystemPrompt: (name: string, content: string, isDefault: boolean) => Promise<void>;
  deleteSystemPrompt: (id: string) => Promise<void>;
  setDefaultSystemPrompt: (id: string) => Promise<void>;
}

export const createSessionSlice: StateCreator<any, [], [], SessionSlice> = (set, get) => ({
  sessions: [],
  currentSession: null,
  messages: [],
  tools: [],
  systemPrompts: [],
  sessionMode: 'chat',
  executionStatus: { type: 'idle' },
  activePlan: null,
  planProgress: null,
  executionLogs: [],

  fetchSessions: async () => {
    get().setLoading(true);
    try {
      const sessions = await api.listSessions(true);
      set({ sessions, loading: false });
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  createSession: async (title, modelId, systemPrompt, personaId) => {
    get().setLoading(true);
    try {
      const conv = await api.createSession(title, modelId, systemPrompt, personaId);
      set((state: any) => ({
        sessions: [conv, ...state.sessions],
        currentSession: conv,
        messages: [],
        loading: false,
        error: null,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  selectSession: async (id) => {
    get().setLoading(true);
    try {
      const conv = await api.getSession(id);
      if (conv) {
        const messages = await api.getMessages(id);
        // Load persisted request context for this session
        const ctxJson = await api.getRequestContext(id);
        if (ctxJson) {
          try { get().setSessionMessages(JSON.parse(ctxJson)); } catch (err) { console.error('Failed to parse session messages:', err); }
        }
        set({ currentSession: conv, messages, loading: false });
      }
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  deleteSession: async (id) => {
    get().setLoading(true);
    try {
      await api.deleteSession(id);
      set((state: any) => ({
        sessions: state.sessions.filter((c: Session) => c.id !== id),
        currentSession: state.currentSession?.id === id ? null : state.currentSession,
        messages: state.currentSession?.id === id ? [] : state.messages,
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  updateSessionTitle: async (id, title) => {
    try {
      await api.updateSessionTitle(id, title);
      set((state: any) => ({
        sessions: state.sessions.map((c: Session) =>
          c.id === id ? { ...c, title } : c
        ),
        currentSession: state.currentSession?.id === id
          ? { ...state.currentSession, title }
          : state.currentSession,
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  updateSessionModel: async (id, modelId) => {
    try {
      await api.updateSessionModel(id, modelId);
      set((state: any) => ({
        sessions: state.sessions.map((c: Session) =>
          c.id === id ? { ...c, model_id: modelId } : c
        ),
        currentSession: state.currentSession?.id === id
          ? { ...state.currentSession, model_id: modelId }
          : state.currentSession,
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  clearSession: async () => {
    const { currentSession } = get();
    if (!currentSession) return;
    try {
      await api.clearSession(currentSession.id);
      set({ messages: [] });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  newChat: () => {
    set({ currentSession: null, messages: [] });
  },

  sendMessage: async (content) => {
    const { currentSession } = get();
    if (!currentSession) return;

    const userMessage: Message = {
      id: Date.now().toString(),
      session_id: currentSession.id,
      role: 'user',
      content,
      created_at: new Date().toISOString(),
    };

    set((state: any) => ({
      messages: [...state.messages, userMessage],
      loading: true,
      error: null,
    }));

    try {
      const assistantMsg = await api.sendMessage(currentSession.id, content);
      set((state: any) => ({
        messages: [...state.messages, assistantMsg],
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  updateSessionConfig: async (id, config) => {
    try {
      const configStr = JSON.stringify(config);
      await api.updateSessionConfig(id, configStr);
      set((state: any) => ({
        sessions: state.sessions.map((s: any) =>
          s.id === id ? { ...s, config: configStr } : s
        ),
        currentSession: state.currentSession?.id === id
          ? { ...state.currentSession, config: configStr }
          : state.currentSession,
      }));
    } catch (err) {
      console.error('Failed to update session config:', err);
    }
  },

  // ── Execution Actions ──

  setSessionMode: (mode) => set({ sessionMode: mode }),

  setExecutionStatus: (status) => set({ executionStatus: status }),

  setActivePlan: (plan) => set({ activePlan: plan }),

  setPlanProgress: (progress) => set({ planProgress: progress }),

  addExecutionLog: (entry) => {
    // Add execution log as a visible message in the chat
    const sessionId = get().currentSession?.id;
    if (!sessionId) return;
    const logMsg: Message = {
      id: `exec-log-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      session_id: sessionId,
      role: 'tool',
      content: `[${entry.step}] ${entry.message}`,
      created_at: entry.timestamp,
    };
    set((state: any) => ({
      executionLogs: [...state.executionLogs, entry].slice(-200),
      messages: [...state.messages, logMsg],
    }));
  },

  clearExecutionLogs: () => set({ executionLogs: [] }),

  executePlan: async (sessionId, plan) => {
    set({
      sessionMode: 'autonomous',
      executionStatus: { type: 'running', step_index: 0, started_at: new Date().toISOString() },
      activePlan: plan,
    });
    try {
      await api.executePlan(sessionId, JSON.stringify(plan));
    } catch (err) {
      set({ executionStatus: { type: 'failed', step_index: 0, error: String(err) } });
    }
  },

  pauseExecution: async (sessionId) => {
    try {
      await api.pauseExecution(sessionId);
      set((state: any) => ({
        executionStatus: { type: 'paused', step_index: state.planProgress?.step_index ?? 0, reason: 'user_paused' },
      }));
    } catch (err) {
      console.error('Failed to pause:', err);
    }
  },

  resumeExecution: async (sessionId) => {
    try {
      await api.resumeExecution(sessionId);
    } catch (err) {
      console.error('Failed to resume:', err);
    }
  },

  cancelExecution: async (sessionId) => {
    try {
      await api.cancelExecution(sessionId);
      set({ activePlan: null, executionStatus: { type: 'idle' }, planProgress: null });
    } catch (err) {
      console.error('Failed to cancel:', err);
    }
  },

  // ── Tools (merged from toolSlice) ──

  fetchTools: async () => {
    try {
      const tools = await api.listTools();
      set({ tools });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  toggleTool: async (name, enabled) => {
    try {
      await api.toggleTool(name, enabled);
      set((state: any) => ({
        tools: state.tools.map((t: any) =>
          t.name === name ? { ...t, enabled } : t
        ),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  // ── System prompts (merged from promptSlice) ──

  fetchSystemPrompts: async () => {
    try {
      const prompts = await api.listSystemPrompts();
      set({ systemPrompts: prompts });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  createSystemPrompt: async (name, content, isDefault) => {
    try {
      const prompt = await api.createSystemPrompt(name, content, isDefault);
      set((state: any) => ({
        systemPrompts: [prompt, ...state.systemPrompts],
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  deleteSystemPrompt: async (id) => {
    try {
      await api.deleteSystemPrompt(id);
      set((state: any) => ({
        systemPrompts: state.systemPrompts.filter((p: any) => p.id !== id),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setDefaultSystemPrompt: async (id) => {
    try {
      await api.setDefaultSystemPrompt(id);
      set((state: any) => ({
        systemPrompts: state.systemPrompts.map((p: any) => ({
          ...p,
          is_default: p.id === id,
        })),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },
});
