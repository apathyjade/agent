import type { StateCreator } from 'zustand';
import type { SkillInfo, SkillDetail, MarketSkill, ReconcileResult } from '../types';
import * as api from '../api/tauri';

export interface SkillSlice {
  skills: SkillInfo[];
  selectedSkillDetail: SkillDetail | null;
  skillLoading: boolean;
  skillError: string | null;
  installDialogOpen: boolean;
  reconciling: boolean;
  marketSkills: MarketSkill[];
  marketLoading: boolean;
  marketSearching: boolean;

  fetchSkills: () => Promise<void>;
  toggleSkill: (id: string, enabled: boolean) => Promise<void>;
  fetchSkillDetail: (id: string) => Promise<void>;
  installSkill: (path: string) => Promise<void>;
  uninstallSkill: (id: string) => Promise<void>;
  configureSkill: (id: string, config: Record<string, unknown>) => Promise<void>;
  clearSkillDetail: () => void;
  setInstallDialogOpen: (open: boolean) => void;
  clearSkillError: () => void;
  reconcileSkills: () => Promise<ReconcileResult>;
  fetchMarketTopSkills: () => Promise<void>;
  searchMarketSkills: (query: string) => Promise<void>;
  installMarketSkill: (source: string) => Promise<void>;
}

export const createSkillSlice: StateCreator<SkillSlice, [], [], SkillSlice> = (set, get) => ({
  skills: [],
  selectedSkillDetail: null,
  skillLoading: false,
  skillError: null,
  installDialogOpen: false,
  reconciling: false,
  marketSkills: [],
  marketLoading: false,
  marketSearching: false,

  fetchSkills: async () => {
    try {
      const skills = await api.listSkills();
      set({ skills });
    } catch (err) {
      set({ skillError: String(err) });
    }
  },

  toggleSkill: async (id, enabled) => {
    try {
      await api.toggleSkill(id, enabled);
      set((state: any) => ({
        skills: state.skills.map((s: SkillInfo) =>
          s.id === id ? { ...s, enabled } : s
        ),
      }));
    } catch (err) {
      set({ skillError: String(err) });
    }
  },

  fetchSkillDetail: async (id) => {
    set({ skillLoading: true, skillError: null });
    try {
      const detail = await api.getSkillDetail(id);
      set({ selectedSkillDetail: detail, skillLoading: false });
    } catch (err) {
      set({ skillError: String(err), skillLoading: false });
    }
  },

  installSkill: async (path) => {
    set({ skillLoading: true, skillError: null });
    try {
      await api.installSkillFromPath(path);
      await get().fetchSkills();
      set({ skillLoading: false, installDialogOpen: false });
    } catch (err) {
      set({ skillError: String(err), skillLoading: false });
      throw err;
    }
  },

  uninstallSkill: async (id) => {
    set({ skillLoading: true, skillError: null });
    try {
      await api.uninstallSkill(id);
      set({ selectedSkillDetail: null, skillLoading: false });
      await get().fetchSkills();
    } catch (err) {
      set({ skillError: String(err), skillLoading: false });
    }
  },

  configureSkill: async (id, config) => {
    set({ skillLoading: true, skillError: null });
    try {
      await api.configureSkill(id, config);
      set({ skillLoading: false });
    } catch (err) {
      set({ skillError: String(err), skillLoading: false });
    }
  },

  reconcileSkills: async () => {
    set({ reconciling: true, skillError: null });
    try {
      const result = await api.reconcileSkills();
      await get().fetchSkills();
      set({ reconciling: false });
      return result;
    } catch (err) {
      set({ skillError: String(err), reconciling: false });
      throw err;
    }
  },

  fetchMarketTopSkills: async () => {
    set({ marketLoading: true, skillError: null });
    try {
      const skills = await api.listMarketTopSkills(30);
      set({ marketSkills: skills, marketLoading: false });
    } catch (err) {
      set({ skillError: String(err), marketLoading: false });
    }
  },

  searchMarketSkills: async (query) => {
    set({ marketSearching: true, skillError: null });
    try {
      const skills = await api.searchMarketSkills(query, 30);
      set({ marketSkills: skills, marketSearching: false });
    } catch (err) {
      set({ skillError: String(err), marketSearching: false });
    }
  },

  installMarketSkill: async (source) => {
    set({ skillLoading: true, skillError: null });
    try {
      await api.installMarketSkill(source);
      // Reconcile to pick up the newly installed skill from disk into DB
      await get().reconcileSkills();
      // Refresh local skills list
      await get().fetchSkills();
      set({ skillLoading: false });
    } catch (err) {
      set({ skillError: String(err), skillLoading: false });
      throw err;
    }
  },

  clearSkillDetail: () => set({ selectedSkillDetail: null }),
  setInstallDialogOpen: (open) => set({ installDialogOpen: open }),
  clearSkillError: () => set({ skillError: null }),
});
