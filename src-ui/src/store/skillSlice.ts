import type { StateCreator } from 'zustand';
import type { SkillInfo, SkillDetail, DiscoveredSkill } from '../types';
import * as api from '../api/tauri';

export interface SkillSlice {
  skills: SkillInfo[];
  selectedSkillDetail: SkillDetail | null;
  skillLoading: boolean;
  skillError: string | null;
  installDialogOpen: boolean;
  discoveredSkills: DiscoveredSkill[];
  discoveredLoading: boolean;

  fetchSkills: () => Promise<void>;
  toggleSkill: (id: string, enabled: boolean) => Promise<void>;
  fetchSkillDetail: (id: string) => Promise<void>;
  installSkill: (path: string) => Promise<void>;
  uninstallSkill: (id: string) => Promise<void>;
  configureSkill: (id: string, config: Record<string, unknown>) => Promise<void>;
  clearSkillDetail: () => void;
  setInstallDialogOpen: (open: boolean) => void;
  clearSkillError: () => void;
  scanDiscoveredSkills: () => Promise<void>;
  importDiscoveredSkill: (discovered: DiscoveredSkill) => Promise<void>;
}

export const createSkillSlice: StateCreator<SkillSlice, [], [], SkillSlice> = (set, get) => ({
  skills: [],
  selectedSkillDetail: null,
  skillLoading: false,
  skillError: null,
  installDialogOpen: false,
  discoveredSkills: [],
  discoveredLoading: false,

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

  scanDiscoveredSkills: async () => {
    set({ discoveredLoading: true, skillError: null });
    try {
      const skills = await api.scanLocalSkills();
      set({ discoveredSkills: skills, discoveredLoading: false });
    } catch (err) {
      set({ skillError: String(err), discoveredLoading: false });
    }
  },

  importDiscoveredSkill: async (discovered) => {
    set({ skillLoading: true, skillError: null });
    try {
      await api.importScannedSkill(discovered.id, discovered.path, discovered.agent_sources);
      // Re-scan discovered to update already_imported flags
      await get().scanDiscoveredSkills();
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
