import type { StateCreator } from 'zustand';
import type { Conversation, Message } from '../types';
import * as api from '../api/tauri';

export interface ConversationSlice {
  conversations: Conversation[];
  currentConversation: Conversation | null;
  messages: Message[];

  fetchConversations: () => Promise<void>;
  createConversation: (title: string, modelId: string, systemPrompt?: string) => Promise<void>;
  selectConversation: (id: string) => Promise<void>;
  deleteConversation: (id: string) => Promise<void>;
  updateConversationTitle: (id: string, title: string) => Promise<void>;
  updateConversationModel: (id: string, modelId: string) => Promise<void>;
  clearConversation: () => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
}

export const createConversationSlice: StateCreator<ConversationSlice, [], [], ConversationSlice> = (set, get) => ({
  conversations: [],
  currentConversation: null,
  messages: [],

  fetchConversations: async () => {
    (get() as any).setLoading(true);
    try {
      const conversations = await api.listConversations();
      set({ conversations, loading: false } as any);
    } catch (err) {
      set({ error: String(err), loading: false } as any);
    }
  },

  createConversation: async (title, modelId, systemPrompt) => {
    (get() as any).setLoading(true);
    try {
      const conv = await api.createConversation(title, modelId, systemPrompt);
      set((state: any) => ({
        conversations: [conv, ...state.conversations],
        currentConversation: conv,
        messages: [],
        loading: false,
        error: null,
      }));
    } catch (err) {
      set({ error: String(err), loading: false } as any);
    }
  },

  selectConversation: async (id) => {
    (get() as any).setLoading(true);
    try {
      const conv = await api.getConversation(id);
      if (conv) {
        const messages = await api.getMessages(id);
        set({ currentConversation: conv, messages, loading: false } as any);
      }
    } catch (err) {
      set({ error: String(err), loading: false } as any);
    }
  },

  deleteConversation: async (id) => {
    (get() as any).setLoading(true);
    try {
      await api.deleteConversation(id);
      set((state: any) => ({
        conversations: state.conversations.filter((c: Conversation) => c.id !== id),
        currentConversation: state.currentConversation?.id === id ? null : state.currentConversation,
        messages: state.currentConversation?.id === id ? [] : state.messages,
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false } as any);
    }
  },

  updateConversationTitle: async (id, title) => {
    try {
      await api.updateConversationTitle(id, title);
      set((state: any) => ({
        conversations: state.conversations.map((c: Conversation) =>
          c.id === id ? { ...c, title } : c
        ),
        currentConversation: state.currentConversation?.id === id
          ? { ...state.currentConversation, title }
          : state.currentConversation,
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  updateConversationModel: async (id, modelId) => {
    try {
      await api.updateConversationModel(id, modelId);
      set((state: any) => ({
        conversations: state.conversations.map((c: Conversation) =>
          c.id === id ? { ...c, model_id: modelId } : c
        ),
        currentConversation: state.currentConversation?.id === id
          ? { ...state.currentConversation, model_id: modelId }
          : state.currentConversation,
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  clearConversation: async () => {
    const { currentConversation } = get();
    if (!currentConversation) return;
    try {
      await api.clearConversation(currentConversation.id);
      set({ messages: [] });
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  sendMessage: async (content) => {
    const { currentConversation } = get();
    if (!currentConversation) return;

    const userMessage: Message = {
      id: Date.now().toString(),
      conversation_id: currentConversation.id,
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
      const assistantMsg = await api.sendMessage(currentConversation.id, content);
      set((state: any) => ({
        messages: [...state.messages, assistantMsg],
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false } as any);
    }
  },
});
