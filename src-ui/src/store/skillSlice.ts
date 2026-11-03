import type { StateCreator } from 'zustand';
import type {
  AgentOption,
  CliStatus,
  InstallSkillParams,
  InstalledSkill,
  LegacyImportResult,
  LegacySkill,
  MarketSkill,
  SkillScope,
  UpdateCheck,
} from '../types';
import * as api from '../api/tauri';

export interface SkillSlice {
  cliStatus: CliStatus | null;
  agents: AgentOption[];
  scope: SkillScope;
  projectPath: string | null;
  installed: InstalledSkill[];
  legacySkills: LegacySkill[];
  marketSkills: MarketSkill[];
  skillContent: string | null;
  skillLoading: boolean;
  /** Short name of the operation currently running, or null. */
  busy: string | null;
  skillError: string | null;
  updates: UpdateCheck | null;

  fetchEnv: () => Promise<void>;
  fetchAgents: () => Promise<void>;
  fetchInstalled: () => Promise<void>;
  fetchContent: (name: string, scope: SkillScope) => Promise<void>;
  installSkill: (params: InstallSkillParams) => Promise<void>;
  removeSkill: (name: string) => Promise<void>;
  checkUpdates: () => Promise<void>;
  updateAll: () => Promise<void>;
  fetchLegacy: () => Promise<void>;
  importLegacy: (paths: string[], agents: string[], copy: boolean) => Promise<LegacyImportResult>;
  fetchMarketTopSkills: () => Promise<void>;
  searchMarketSkills: (query: string) => Promise<void>;
  setScope: (scope: SkillScope) => Promise<void>;
  setProjectPath: (path: string | null) => Promise<void>;
  clearSkillContent: () => void;
  clearSkillError: () => void;
}

export const createSkillSlice: StateCreator<SkillSlice, [], [], SkillSlice> = (set, get) => ({
  cliStatus: null,
  agents: [],
  scope: 'global',
  projectPath: null,
  installed: [],
  legacySkills: [],
  marketSkills: [],
  skillContent: null,
  skillLoading: false,
  busy: null,
  skillError: null,
  updates: null,

  fetchEnv: async () => {
    set({ skillError: null });
    try {
      const cliStatus = await api.getSkillsEnv();
      set({ cliStatus });
    } catch (err) {
      set({ skillError: String(err) });
    }
  },

  fetchAgents: async () => {
    set({ skillError: null });
    try {
      const agents = await api.listSkillAgents();
      set({ agents });
    } catch (err) {
      set({ skillError: String(err) });
    }
  },

  fetchInstalled: async () => {
    set({ skillLoading: true, skillError: null });
    try {
      const { scope, projectPath } = get();
      const installed = await api.listInstalledSkills(scope, projectPath);
      set({ installed });
    } catch (err) {
      set({ installed: [], skillError: String(err) });
    } finally {
      set({ skillLoading: false });
    }
  },

  fetchContent: async (name, scope) => {
    set({ busy: 'content', skillError: null, skillContent: null });
    try {
      const skillContent = await api.getSkillContent(name, scope, get().projectPath);
      set({ skillContent });
    } catch (err) {
      set({ skillError: String(err) });
    } finally {
      set({ busy: null });
    }
  },

  installSkill: async (params) => {
    set({ busy: 'install', skillError: null });
    try {
      await api.installSkills({ ...params, projectPath: params.projectPath ?? get().projectPath });
      await get().fetchInstalled();
    } catch (err) {
      set({ skillError: String(err) });
      throw err;
    } finally {
      set({ busy: null });
    }
  },

  removeSkill: async (name) => {
    set({ busy: 'remove', skillError: null });
    try {
      const { scope, projectPath } = get();
      await api.removeSkill(name, scope, projectPath);
      await get().fetchInstalled();
    } catch (err) {
      set({ skillError: String(err) });
      throw err;
    } finally {
      set({ busy: null });
    }
  },

  checkUpdates: async () => {
    set({ busy: 'update', skillError: null });
    try {
      const { scope, projectPath } = get();
      const updates = await api.checkSkillUpdates(scope, projectPath);
      set({ updates });
    } catch (err) {
      set({ skillError: String(err) });
    } finally {
      set({ busy: null });
    }
  },

  updateAll: async () => {
    set({ busy: 'update', skillError: null });
    try {
      const { scope, projectPath } = get();
      await api.updateSkills(scope, projectPath);
      await get().fetchInstalled();
      await get().checkUpdates();
    } catch (err) {
      set({ skillError: String(err) });
    } finally {
      set({ busy: null });
    }
  },

  fetchLegacy: async () => {
    set({ busy: 'legacy', skillError: null });
    try {
      const legacySkills = await api.detectLegacySkills();
      set({ legacySkills });
    } catch (err) {
      set({ skillError: String(err) });
    } finally {
      set({ busy: null });
    }
  },

  importLegacy: async (paths, agents, copy) => {
    set({ busy: 'legacy', skillError: null });
    try {
      const result = await api.importLegacySkills(paths, agents, copy);
      await get().fetchInstalled();
      await get().fetchLegacy();
      return result;
    } catch (err) {
      set({ skillError: String(err) });
      throw err;
    } finally {
      set({ busy: null });
    }
  },

  fetchMarketTopSkills: async () => {
    set({ busy: 'market', skillError: null });
    try {
      const marketSkills = await api.listMarketTopSkills(30);
      set({ marketSkills });
    } catch (err) {
      set({ skillError: String(err) });
    } finally {
      set({ busy: null });
    }
  },

  searchMarketSkills: async (query) => {
    set({ busy: 'market', skillError: null });
    try {
      const marketSkills = await api.searchMarketSkills(query, 30);
      set({ marketSkills });
    } catch (err) {
      set({ skillError: String(err) });
    } finally {
      set({ busy: null });
    }
  },

  setScope: async (scope) => {
    set({ scope });
    await get().fetchInstalled();
  },

  setProjectPath: async (path) => {
    set({ projectPath: path });
    await get().fetchInstalled();
  },

  clearSkillContent: () => set({ skillContent: null }),
  clearSkillError: () => set({ skillError: null }),
});
