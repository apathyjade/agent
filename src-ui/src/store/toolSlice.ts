import type { StateCreator } from 'zustand';
import type { ToolInfo } from '../types';
import * as api from '../api/tauri';

export interface ToolSlice {
  tools: ToolInfo[];

  fetchTools: () => Promise<void>;
  toggleTool: (name: string, enabled: boolean) => Promise<void>;
}

export const createToolSlice: StateCreator<ToolSlice, [], [], ToolSlice> = (set) => ({
  tools: [],

  fetchTools: async () => {
    try {
      const tools = await api.listTools();
      set({ tools });
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  toggleTool: async (name, enabled) => {
    try {
      await api.toggleTool(name, enabled);
      set((state: any) => ({
        tools: state.tools.map((t: ToolInfo) =>
          t.name === name ? { ...t, enabled } : t
        ),
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },
});
