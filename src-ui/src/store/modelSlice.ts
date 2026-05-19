import type { StateCreator } from 'zustand';
import type { ModelConfig, ProviderStatus } from '../types';
import * as api from '../api/tauri';

export interface ModelSlice {
  providers: ProviderStatus[];
  models: ModelConfig[];
  defaultModel: string | null;

  fetchModels: () => Promise<void>;
  fetchProviders: () => Promise<void>;
  setupProvider: (params: Parameters<typeof api.setupProvider>[0]) => Promise<void>;
  updateProviderConfig: (params: Parameters<typeof api.updateProviderConfig>[0]) => Promise<void>;
  removeProvider: (provider: string) => Promise<void>;
  setDefaultModel: (model: string) => Promise<void>;
}

export const createModelSlice: StateCreator<ModelSlice, [], [], ModelSlice> = (set, get) => ({
  providers: [],
  models: [],
  defaultModel: null,

  fetchModels: async () => {
    try {
      const models = await api.getModels();
      const defaultModel = models.find(m => m.is_default)?.id ?? models.find(m => m.enabled)?.id ?? null;
      set({ models, defaultModel });
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  fetchProviders: async () => {
    try {
      const providers = await api.listProviders();
      set({ providers });
    } catch (err) {
      // provider commands not available in current backend; ignore
    }
  },

  setupProvider: async (params) => {
    await api.setupProvider(params);
    // Refresh providers and models after setup to ensure UI is up-to-date.
    // These may fail independently (e.g., provider list not available in some backends).
    await get().fetchProviders();
    await get().fetchModels();
  },

  updateProviderConfig: async (params) => {
    try {
      await api.updateProviderConfig(params);
      await get().fetchProviders();
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  removeProvider: async (provider) => {
    try {
      await api.removeProvider(provider);
      await get().fetchProviders();
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },

  setDefaultModel: async (model) => {
    try {
      await api.setDefaultModel(model);
      set((state: any) => ({
        defaultModel: model,
        models: state.models.map((m: ModelConfig) => ({
          ...m,
          is_default: m.id === model,
        })),
      }));
    } catch (err) {
      set({ error: String(err) } as any);
    }
  },
});
