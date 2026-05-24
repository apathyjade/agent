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

export const createPromptSlice: StateCreator<any, [], [], PromptSlice> = (set) => ({
  systemPrompts: [],

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
