import type { StateCreator } from 'zustand';
import type { LifecycleConfig } from '../types';
import * as api from '../api/tauri';

export interface LifecycleSlice {
  lifecycleConfig: LifecycleConfig;
  fetchLifecycleConfig: () => Promise<void>;
  updateLifecycleConfig: (config: LifecycleConfig) => Promise<void>;
  archiveSession: (id: string) => Promise<void>;
  unarchiveSession: (id: string) => Promise<void>;
}

export const createLifecycleSlice: StateCreator<any, [], [], LifecycleSlice> = (set, _get) => ({
  lifecycleConfig: {
    auto_title_enabled: true,
    title_model: null,
    auto_summarize_enabled: true,
    summarize_chunk_size: 20,
    summarize_model: null,
    auto_archive_enabled: true,
    archive_after_days: 30,
  },

  fetchLifecycleConfig: async () => {
    try {
      const config = await api.getLifecycleConfig();
      set({ lifecycleConfig: config });
    } catch (err) {
      console.error('Failed to load lifecycle config:', err);
    }
  },

  updateLifecycleConfig: async (config) => {
    try {
      await api.updateLifecycleConfig(config);
      set({ lifecycleConfig: config });
    } catch (err) {
      console.error('Failed to update lifecycle config:', err);
    }
  },

  archiveSession: async (id) => {
    try {
      await api.archiveSession(id);
      // Update local state: mark as archived instead of removing
      set((state: any) => ({
        sessions: state.sessions.map((s: any) =>
          s.id === id ? { ...s, archived: true } : s
        ),
      }));
    } catch (err) {
      console.error('Failed to archive session:', err);
    }
  },

  unarchiveSession: async (id) => {
    try {
      await api.unarchiveSession(id);
      // Update local state: mark as not archived
      set((state: any) => ({
        sessions: state.sessions.map((s: any) =>
          s.id === id ? { ...s, archived: false } : s
        ),
      }));
    } catch (err) {
      console.error('Failed to unarchive session:', err);
    }
  },
});
