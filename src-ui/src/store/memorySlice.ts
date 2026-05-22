import type { StateCreator } from 'zustand';
import type { MemoryInfo, CreateMemoryParams, UpdateMemoryParams } from '../types';
import * as api from '../api/tauri';

export interface MemorySlice {
  memories: MemoryInfo[];
  memoryLoading: boolean;
  memoryError: string | null;
  memorySearchQuery: string;
  memoryFilterType: string;

  fetchMemories: () => Promise<void>;
  createMemory: (params: CreateMemoryParams) => Promise<void>;
  updateMemory: (id: string, params: UpdateMemoryParams) => Promise<void>;
  deleteMemory: (id: string) => Promise<void>;
  searchMemories: (query: string, memoryType?: string, scope?: string) => Promise<void>;
  setMemorySearchQuery: (query: string) => void;
  setMemoryFilterType: (type: string) => void;
  clearMemoryError: () => void;
}

export const createMemorySlice: StateCreator<MemorySlice, [], [], MemorySlice> = (set, get) => ({
  memories: [],
  memoryLoading: false,
  memoryError: null,
  memorySearchQuery: '',
  memoryFilterType: '',

  fetchMemories: async () => {
    set({ memoryLoading: true, memoryError: null });
    try {
      const memories = await api.listMemories();
      set({ memories, memoryLoading: false });
    } catch (err) {
      set({ memoryError: String(err), memoryLoading: false });
    }
  },

  createMemory: async (params) => {
    set({ memoryLoading: true, memoryError: null });
    try {
      await api.createMemory(params);
      await get().fetchMemories();
    } catch (err) {
      set({ memoryError: String(err), memoryLoading: false });
      throw err;
    }
  },

  updateMemory: async (id, params) => {
    set({ memoryLoading: true, memoryError: null });
    try {
      await api.updateMemory(id, params);
      await get().fetchMemories();
    } catch (err) {
      set({ memoryError: String(err), memoryLoading: false });
      throw err;
    }
  },

  deleteMemory: async (id) => {
    set({ memoryLoading: true, memoryError: null });
    try {
      await api.deleteMemory(id);
      set((state) => ({
        memories: state.memories.filter((m) => m.id !== id),
        memoryLoading: false,
      }));
    } catch (err) {
      set({ memoryError: String(err), memoryLoading: false });
      throw err;
    }
  },

  searchMemories: async (query, memoryType, scope) => {
    set({ memoryLoading: true, memoryError: null, memorySearchQuery: query });
    try {
      const memories = await api.searchMemories(query, memoryType, scope);
      set({ memories, memoryLoading: false });
    } catch (err) {
      set({ memoryError: String(err), memoryLoading: false });
    }
  },

  setMemorySearchQuery: (query) => set({ memorySearchQuery: query }),
  setMemoryFilterType: (type) => set({ memoryFilterType: type }),
  clearMemoryError: () => set({ memoryError: null }),
});
