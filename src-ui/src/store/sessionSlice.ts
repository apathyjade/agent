import type { StateCreator } from 'zustand';
import type { Session, Message } from '../types';
import * as api from '../api/tauri';

export interface SessionSlice {
  sessions: Session[];
  currentSession: Session | null;
  messages: Message[];

  fetchSessions: () => Promise<void>;
  createSession: (title: string, modelId: string, systemPrompt?: string) => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  updateSessionTitle: (id: string, title: string) => Promise<void>;
  updateSessionModel: (id: string, modelId: string) => Promise<void>;
  clearSession: () => Promise<void>;
  newChat: () => void;
  sendMessage: (content: string) => Promise<void>;
}

export const createSessionSlice: StateCreator<any, [], [], SessionSlice> = (set, get) => ({
  sessions: [],
  currentSession: null,
  messages: [],

  fetchSessions: async () => {
    get().setLoading(true);
    try {
      const sessions = await api.listSessions();
      set({ sessions, loading: false });
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  createSession: async (title, modelId, systemPrompt) => {
    get().setLoading(true);
    try {
      const conv = await api.createSession(title, modelId, systemPrompt);
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
});
