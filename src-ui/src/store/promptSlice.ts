import type { StateCreator } from 'zustand';
import type { SystemPrompt } from '../types';
import * as api from '../api/tauri';

export interface PromptSlice {
  systemPrompts: SystemPrompt[];

  fetchSystemPrompts: () => Promise<void>;
  createSystemPrompt: (name: string, content: string, isDefault: boolean) => Promise<void>;
  deleteSystemPrompt: (id: string) => Promise<void>;
  setDefaultSystemPrompt: (id: string) => Promise<void>;
}

export const createPromptSlice: StateCreator<PromptSlice, [], [], PromptSlice> = (set) => ({
  systemPrompts: [],

  fetchSystemPrompts: async () => {
    try {
      const prompts = await api.listSystemPrompts();
      set({ systemPrompts: prompts });
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  createSystemPrompt: async (name, content, isDefault) => {
    try {
      const prompt = await api.createSystemPrompt(name, content, isDefault);
      set((state: any) => ({
        systemPrompts: [prompt, ...state.systemPrompts],
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  deleteSystemPrompt: async (id) => {
    try {
      await api.deleteSystemPrompt(id);
      set((state: any) => ({
        systemPrompts: state.systemPrompts.filter((p: SystemPrompt) => p.id !== id),
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  setDefaultSystemPrompt: async (id) => {
    try {
      await api.setDefaultSystemPrompt(id);
      set((state: any) => ({
        systemPrompts: state.systemPrompts.map((p: SystemPrompt) => ({
          ...p,
          is_default: p.id === id,
        })),
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },
});
