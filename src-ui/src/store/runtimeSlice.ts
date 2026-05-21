import type { StateCreator } from 'zustand';
import type { RuntimeInfo, RuntimeType, InstallProgress, AvailableVersion } from '../types';
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
  availableVersions: AvailableVersion[];
  availableVersionsLoading: boolean;

  /** Install directory path */
  installDir: string;

  fetchRuntimes: () => Promise<void>;
  refreshRuntime: (rt: RuntimeType) => Promise<void>;
  installRuntime: (rt: RuntimeType, version?: string) => Promise<void>;
  clearInstallProgress: () => void;
  clearRuntimeError: () => void;

  /** Version management */
  fetchAvailableVersions: (rt: RuntimeType) => Promise<void>;
  switchVersion: (rt: RuntimeType, version: string) => Promise<void>;
  uninstallVersion: (rt: RuntimeType, version: string) => Promise<void>;

  /** Install directory */
  fetchInstallDir: () => Promise<void>;
  setInstallDir: (dir: string) => Promise<void>;
}

export const createRuntimeSlice: StateCreator<RuntimeSlice, [], [], RuntimeSlice> = (set, _get) => ({
  runtimes: [],
  runtimeLoading: false,
  runtimeError: null,
  installProgress: null,
  installingRuntime: null,
  availableVersions: [],
  availableVersionsLoading: false,
  installDir: '',

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
      set({ availableVersions: versions, availableVersionsLoading: false });
    } catch {
      set({ availableVersionsLoading: false });
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
    } catch {
      // ignore
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
});
