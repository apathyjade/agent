import type { StateCreator } from 'zustand';
import type { RuntimeInfo, RuntimeType, RuntimeVersion, InstallProgress, BoundProject, ProjectScanResult, HealthCheckItem, VersionUpdate, PathConflict, BatchInstallItem, BatchInstallResult, VersionManager, DiskUsageItem } from '../types';
import * as api from '../api/tauri';
import { listen } from '@tauri-apps/api/event';

export interface RuntimeSlice {
  runtimes: RuntimeInfo[];
  runtimeLoading: boolean;
  runtimeError: string | null;

  /** Current installation progress (null if not installing) */
  installProgress: InstallProgress | null;
  installingRuntime: RuntimeType | null;

  /** Available versions for the runtime being installed */
  availableVersions: RuntimeVersion[];
  availableVersionsLoading: boolean;

  /** Cached versions keyed by runtime_type */
  versionCache: Record<string, RuntimeVersion[]>;

  /** Install directory path */
  installDir: string;

  fetchRuntimes: () => Promise<void>;
  refreshRuntime: (rt: RuntimeType) => Promise<void>;
  installRuntime: (rt: RuntimeType, version?: string) => Promise<void>;
  clearInstallProgress: () => void;
  clearRuntimeError: () => void;

  /** Version management */
  fetchAvailableVersions: (rt: RuntimeType) => Promise<void>;
  refreshVersionCache: (rt: RuntimeType) => Promise<RuntimeVersion[]>;
  switchVersion: (rt: RuntimeType, version: string) => Promise<void>;
  uninstallVersion: (rt: RuntimeType, version: string) => Promise<void>;

  /** Install directory */
  fetchInstallDir: () => Promise<void>;
  setInstallDir: (dir: string) => Promise<void>;

  // ── Project Binding ──

  projectBindings: BoundProject[];
  projectBindingLoading: boolean;

  fetchProjectBindings: () => Promise<void>;
  addProjectBinding: (path: string) => Promise<void>;
  removeProjectBinding: (id: string) => Promise<void>;
  syncProjectBinding: (id: string) => Promise<void>;
  scanProjectBinding: (path: string) => Promise<ProjectScanResult | null>;

  // ── Health & Updates ──

  healthItems: HealthCheckItem[];
  versionUpdates: VersionUpdate[];

  fetchHealthStatus: () => Promise<void>;
  checkUpdates: () => Promise<void>;

  // ── PATH Conflicts & Batch Install ──

  pathConflicts: PathConflict[];
  batchInstalling: boolean;
  batchInstallResults: BatchInstallResult[];

  fetchPathConflicts: () => Promise<void>;
  batchInstallAll: (installs: { runtimeType: string; version: string | null }[]) => Promise<void>;

  // ── Version Manager Integration ──

  managers: Record<string, VersionManager[]>;
  activeManagers: Record<string, string>;
  managersLoading: boolean;
  fetchManagers: (rt: string) => Promise<void>;
  fetchAllManagers: () => Promise<void>;
  setManager: (rt: string, managerId: string) => Promise<void>;
  installManagerTool: (managerId: string, downloadUrl: string) => Promise<string>;

  // ── Disk Usage ──

  diskUsage: DiskUsageItem[];
  diskUsageLoading: boolean;

  fetchDiskUsage: () => Promise<void>;
}

export const createRuntimeSlice: StateCreator<RuntimeSlice, [], [], RuntimeSlice> = (set, _get) => ({
  runtimes: [],
  runtimeLoading: false,
  runtimeError: null,
  installProgress: null,
  installingRuntime: null,
  availableVersions: [],
  availableVersionsLoading: false,
  versionCache: {},
  installDir: '',
  projectBindings: [],
  projectBindingLoading: false,
  healthItems: [],
  versionUpdates: [],
  pathConflicts: [],
  batchInstalling: false,
  batchInstallResults: [],
  managers: {},
  activeManagers: {},
  managersLoading: false,
  diskUsage: [],
  diskUsageLoading: false,

  fetchRuntimes: async () => {
    set({ runtimeLoading: true, runtimeError: null });
    try {
      const runtimes = await api.listRuntimes();
      set({ runtimes, runtimeLoading: false });
    } catch (err) {
      set({ runtimeError: String(err), runtimeLoading: false });
    }
  },

  refreshRuntime: async (rt) => {
    try {
      const info = await api.refreshRuntime(rt);
      set((state) => ({
        runtimes: state.runtimes.map((r) => r.runtime_type === rt ? info : r),
      }));
    } catch (err) {
      set({ runtimeError: String(err) });
    }
  },

  installRuntime: async (rt, version) => {
    set({ installingRuntime: rt, installProgress: null, runtimeError: null });

    const unlisten = await listen<InstallProgress>('install_progress', (event) => {
      set({ installProgress: event.payload });
    });

    try {
      await api.installRuntime(rt, version);
      // Refresh full runtime list after install
      const runtimes = await api.listRuntimes();
      set({
        installingRuntime: null,
        installProgress: null,
        runtimes,
      });
    } catch (err) {
      set({ installingRuntime: null, runtimeError: String(err) });
    } finally {
      unlisten();
    }
  },

  clearInstallProgress: () => {
    set({ installProgress: null, installingRuntime: null });
  },

  clearRuntimeError: () => set({ runtimeError: null }),

  // ── Version Management ──

  fetchAvailableVersions: async (rt) => {
    set({ availableVersionsLoading: true });
    try {
      const versions = await api.listAvailableVersions(rt);
      set((state) => ({
        availableVersions: versions,
        availableVersionsLoading: false,
        versionCache: { ...state.versionCache, [rt]: versions },
      }));
    } catch (err) {
      console.error('runtimeSlice error:', err);
      set({ availableVersionsLoading: false });
    }
  },

  refreshVersionCache: async (rt) => {
    set({ availableVersionsLoading: true });
    try {
      const versions = await api.refreshVersionCache(rt);
      set((state) => ({
        availableVersions: versions,
        availableVersionsLoading: false,
        versionCache: { ...state.versionCache, [rt]: versions },
      }));
      return versions;
    } catch (err) {
      set({ availableVersionsLoading: false });
      throw err;
    }
  },

  switchVersion: async (rt, version) => {
    set({ runtimeLoading: true, runtimeError: null });
    try {
      const info = await api.switchRuntimeVersion(rt, version);
      set((state) => ({
        runtimeLoading: false,
        runtimes: state.runtimes.map((r) => r.runtime_type === rt ? info : r),
      }));
    } catch (err) {
      set({ runtimeError: String(err), runtimeLoading: false });
    }
  },

  uninstallVersion: async (rt, version) => {
    set({ runtimeLoading: true, runtimeError: null });
    try {
      const info = await api.uninstallRuntimeVersion(rt, version);
      set((state) => ({
        runtimeLoading: false,
        runtimes: state.runtimes.map((r) => r.runtime_type === rt ? info : r),
      }));
    } catch (err) {
      set({ runtimeError: String(err), runtimeLoading: false });
    }
  },

  // ── Install Directory ──

  fetchInstallDir: async () => {
    try {
      const dir = await api.getRuntimeInstallDir();
      set({ installDir: dir });
    } catch (err) {
      console.error(err);
    }
  },

  setInstallDir: async (dir) => {
    set({ runtimeLoading: true, runtimeError: null });
    try {
      const result = await api.setRuntimeInstallDir(dir);
      set({ installDir: result, runtimeLoading: false });
      // Re-detect runtimes at new location
      const runtimes = await api.listRuntimes();
      set({ runtimes });
    } catch (err) {
      set({ runtimeError: String(err), runtimeLoading: false });
    }
  },

  // ── Project Binding ──

  fetchProjectBindings: async () => {
    set({ projectBindingLoading: true });
    try {
      const projects = await api.listBoundProjects();
      set({ projectBindings: projects, projectBindingLoading: false });
    } catch (err) {
      console.error('runtimeSlice error:', err);
      set({ projectBindingLoading: false });
    }
  },

  addProjectBinding: async (path) => {
    try {
      const project = await api.addBoundProject(path);
      set((state) => ({
        projectBindings: [...state.projectBindings, project],
      }));
    } catch (err) {
      set({ runtimeError: String(err) });
    }
  },

  removeProjectBinding: async (id) => {
    try {
      await api.removeBoundProject(id);
      set((state) => ({
        projectBindings: state.projectBindings.filter((p) => p.id !== id),
      }));
    } catch (err) {
      set({ runtimeError: String(err) });
    }
  },

  syncProjectBinding: async (id) => {
    try {
      await api.syncProject(id);
      // Refresh runtimes after sync
      const runtimes = await api.listRuntimes();
      set({ runtimes });
    } catch (err) {
      set({ runtimeError: String(err) });
    }
  },

  scanProjectBinding: async (path) => {
    try {
      const result = await api.scanProject(path);
      return result;
    } catch (err) {
      console.error(err);
      return null;
    }
  },

  // ── Health & Updates ──

  fetchHealthStatus: async () => {
    try {
      const updates = await api.checkRuntimeUpdates();
      // Build health items from updates
      const items: HealthCheckItem[] = [];
      // For now, basic health info — will be enhanced
      set({ versionUpdates: updates, healthItems: items });
    } catch (err) {
      console.error(err);
    }
  },

  checkUpdates: async () => {
    try {
      const updates = await api.checkRuntimeUpdates();
      set({ versionUpdates: updates });
    } catch {
      // ignore
    }
  },

  // ── PATH Conflicts & Batch Install ──

  fetchPathConflicts: async () => {
    try {
      const conflicts = await api.detectPathConflicts();
      set({ pathConflicts: conflicts });
    } catch (err) {
      console.error(err);
    }
  },

  batchInstallAll: async (installs) => {
    set({ batchInstalling: true, batchInstallResults: [] });
    try {
      const items: BatchInstallItem[] = installs.map(i => ({
        runtime_type: i.runtimeType,
        version: i.version,
      }));
      const results = await api.batchInstallRuntimes(items);
      set({ batchInstallResults: results, batchInstalling: false });
      // Refresh runtimes after batch install
      const runtimes = await api.listRuntimes();
      set({ runtimes });
    } catch (err) {
      set({ batchInstalling: false, runtimeError: String(err) });
    }
  },

  // ── Version Manager Integration ──

  fetchManagers: async (rt) => {
    set({ managersLoading: true });
    try {
      const managers = await api.getVersionManagers(rt);
      set((state) => ({
        managersLoading: false,
        managers: { ...state.managers, [rt]: managers },
      }));
    } catch (err) {
      console.error('runtimeSlice error:', err);
      set({ managersLoading: false });
    }
  },

  fetchAllManagers: async () => {
    set({ managersLoading: true });
    try {
      const allRts = ['node', 'python', 'go', 'rust', 'java', 'docker', 'deno', 'bun', 'ruby', 'uv'];
      const results: Record<string, VersionManager[]> = {};
      for (const rt of allRts) {
        results[rt] = await api.getVersionManagers(rt);
      }
      set({ managers: results, managersLoading: false });
    } catch (err) {
      console.error('runtimeSlice error:', err);
      set({ managersLoading: false });
    }
  },

  setManager: async (rt, managerId) => {
    try {
      await api.setActiveManager(rt, managerId);
      set((state) => ({
        activeManagers: { ...state.activeManagers, [rt]: managerId },
      }));
    } catch (err) {
      set({ runtimeError: String(err) });
    }
  },

  installManagerTool: async (managerId, downloadUrl) => {
    try {
      const result = await api.installManagerTool(managerId, downloadUrl);
      return result;
    } catch (err) {
      throw err;
    }
  },

  // ── Disk Usage ──

  fetchDiskUsage: async () => {
    set({ diskUsageLoading: true });
    try {
      const usage = await api.getRuntimesDiskUsage();
      set({ diskUsage: usage, diskUsageLoading: false });
    } catch (err) {
      console.error('runtimeSlice error:', err);
      set({ diskUsageLoading: false });
    }
  },
});
