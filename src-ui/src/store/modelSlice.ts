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

export const createModelSlice: StateCreator<any, [], [], ModelSlice> = (set, get) => ({
  providers: [],
  models: [],
  defaultModel: null,

  fetchModels: async () => {
    try {
      const models = await api.getModels();
      const defaultModel = models.find(m => m.is_default)?.id ?? models.find(m => m.enabled)?.id ?? null;
      set({ models, defaultModel });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  fetchProviders: async () => {
    try {
      const providers = await api.listProviders();
      set({ providers });
    } catch (err) {
      set({ error: String(err) });
    }
  },

  setupProvider: async (params) => {
    try {
      await api.setupProvider(params);
      await get().fetchProviders();
      await get().fetchModels();
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

  updateProviderConfig: async (params) => {
    try {
      await api.updateProviderConfig(params);
      await get().fetchProviders();
      await get().fetchModels();
    } catch (err) {
      set({ error: String(err) });
    }
  },

  removeProvider: async (provider) => {
    try {
      await api.removeProvider(provider);
      await get().fetchProviders();
      await get().fetchModels();
    } catch (err) {
      set({ error: String(err) });
    }
  },
});
