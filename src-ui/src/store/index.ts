import { create } from 'zustand';
import type { Conversation, Message, ToolInfo, StreamChunk, SystemPrompt, ProviderStatus } from '../types';
import * as api from '../api/tauri';

interface ToolCallState {
  id: string;
  name: string;
  status: string;
  result?: string;
}

interface AppState {
  conversations: Conversation[];
  currentConversation: Conversation | null;
  messages: Message[];
  providers: ProviderStatus[];
  defaultModel: string | null;
  tools: ToolInfo[];
  systemPrompts: SystemPrompt[];
  loading: boolean;
  error: string | null;
  streamingContent: string;
  isStreaming: boolean;
  activeToolCalls: ToolCallState[];

  fetchConversations: () => Promise<void>;
  fetchProviders: () => Promise<void>;
  setupProvider: (params: Parameters<typeof api.setupProvider>[0]) => Promise<void>;
  updateProviderConfig: (params: Parameters<typeof api.updateProviderConfig>[0]) => Promise<void>;
  removeProvider: (provider: string) => Promise<void>;
  setDefaultModel: (model: string) => Promise<void>;
  createConversation: (title: string, modelId: string, systemPrompt?: string) => Promise<void>;
  selectConversation: (id: string) => Promise<void>;
  deleteConversation: (id: string) => Promise<void>;
  updateConversationTitle: (id: string, title: string) => Promise<void>;
  clearConversation: () => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  sendMessageStream: (content: string) => Promise<void>;
  fetchTools: () => Promise<void>;
  toggleTool: (name: string, enabled: boolean) => Promise<void>;
  fetchSystemPrompts: () => Promise<void>;
  createSystemPrompt: (name: string, content: string, isDefault: boolean) => Promise<void>;
  deleteSystemPrompt: (id: string) => Promise<void>;
  setDefaultSystemPrompt: (id: string) => Promise<void>;
  setError: (error: string | null) => void;
}

export const useStore = create<AppState>((set, get) => ({
  conversations: [],
  currentConversation: null,
  messages: [],
  providers: [],
  defaultModel: null,
  tools: [],
  systemPrompts: [],
  loading: false,
  error: null,
  streamingContent: '',
  isStreaming: false,
  activeToolCalls: [],

  fetchConversations: async () => {
    set({ loading: true, error: null });
    try {
      const conversations = await api.listConversations();
      set({ conversations, loading: false });
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  fetchProviders: async () => {
    try {
      const providers = await api.listProviders();
      const defaultModel = await api.getDefaultModel();
      set({ providers, defaultModel });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setupProvider: async (params) => {
    try {
      await api.setupProvider(params);
      await get().fetchProviders();
    } catch (err) {
      set({ error: String(err) });
    }
  },

  updateProviderConfig: async (params) => {
    try {
      await api.updateProviderConfig(params);
      await get().fetchProviders();
    } catch (err) {
      set({ error: String(err) });
    }
  },

  removeProvider: async (provider) => {
    try {
      await api.removeProvider(provider);
      await get().fetchProviders();
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setDefaultModel: async (model) => {
    try {
      await api.setDefaultModel(model);
      set({ defaultModel: model });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  createConversation: async (title, modelId, systemPrompt) => {
    set({ loading: true, error: null });
    try {
      const conv = await api.createConversation(title, modelId, systemPrompt);
      set((state) => ({
        conversations: [conv, ...state.conversations],
        currentConversation: conv,
        messages: [],
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  selectConversation: async (id) => {
    set({ loading: true, error: null });
    try {
      const conv = await api.getConversation(id);
      if (conv) {
        const messages = await api.getMessages(id);
        set({ currentConversation: conv, messages, loading: false });
      }
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  deleteConversation: async (id) => {
    set({ loading: true, error: null });
    try {
      await api.deleteConversation(id);
      set((state) => ({
        conversations: state.conversations.filter((c) => c.id !== id),
        currentConversation: state.currentConversation?.id === id ? null : state.currentConversation,
        messages: state.currentConversation?.id === id ? [] : state.messages,
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  updateConversationTitle: async (id, title) => {
    try {
      await api.updateConversationTitle(id, title);
      set((state) => ({
        conversations: state.conversations.map((c) =>
          c.id === id ? { ...c, title } : c
        ),
        currentConversation: state.currentConversation?.id === id
          ? { ...state.currentConversation, title }
          : state.currentConversation,
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  clearConversation: async () => {
    const { currentConversation } = get();
    if (!currentConversation) return;
    try {
      await api.clearConversation(currentConversation.id);
      set({ messages: [] });
    } catch (err) {
      set({ error: String(err) });
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

    set((state) => ({
      messages: [...state.messages, userMessage],
      loading: true,
      error: null,
    }));

    try {
      const assistantMsg = await api.sendMessage(currentConversation.id, content);
      set((state) => ({
        messages: [...state.messages, assistantMsg],
        loading: false,
      }));
    } catch (err) {
      set({ error: String(err), loading: false });
    }
  },

  sendMessageStream: async (content) => {
    const { currentConversation } = get();
    if (!currentConversation) return;

    const userMessage: Message = {
      id: Date.now().toString(),
      conversation_id: currentConversation.id,
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
        currentConversation.id,
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
            const { streamingContent } = get();
            const assistantMsg: Message = {
              id: Date.now().toString(),
              conversation_id: currentConversation.id,
              role: 'assistant',
              content: streamingContent,
              created_at: new Date().toISOString(),
            };
            set((state) => ({
              messages: [...state.messages, assistantMsg],
              loading: false,
              isStreaming: false,
              streamingContent: '',
              activeToolCalls: [],
            }));
          }
        }
      );
    } catch (err) {
      set({ error: String(err), loading: false, isStreaming: false });
    }
  },

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
      set((state) => ({
        tools: state.tools.map((t) =>
          t.name === name ? { ...t, enabled } : t
        ),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

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
      set((state) => ({
        systemPrompts: [prompt, ...state.systemPrompts],
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  deleteSystemPrompt: async (id) => {
    try {
      await api.deleteSystemPrompt(id);
      set((state) => ({
        systemPrompts: state.systemPrompts.filter((p) => p.id !== id),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setDefaultSystemPrompt: async (id) => {
    try {
      await api.setDefaultSystemPrompt(id);
      set((state) => ({
        systemPrompts: state.systemPrompts.map((p) => ({
          ...p,
          is_default: p.id === id,
        })),
      }));
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setError: (error) => set({ error }),
}));
