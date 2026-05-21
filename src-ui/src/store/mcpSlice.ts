import type { StateCreator } from 'zustand';
import type { McpConnectionInfo, ConnectionStats } from '../types';
import * as api from '../api/tauri';

export interface McpSlice {
  mcpConnections: McpConnectionInfo[];
  mcpLoading: boolean;
  mcpError: string | null;
  addMcpDialogOpen: boolean;
  newMcpName: string;
  newMcpCommand: string;
  newMcpArgs: string;

  // Log viewer state
  logViewerServerId: string | null;
  logViewerOpen: boolean;
  logEntries: string[];
  logLoading: boolean;

  fetchMcpConnections: () => Promise<void>;
  addMcpServer: () => Promise<void>;
  removeMcpServer: (id: string) => Promise<void>;
  connectMcpServer: (id: string) => Promise<void>;
  disconnectMcpServer: (id: string) => Promise<void>;
  restartMcpServer: (id: string) => Promise<void>;
  updateToolConfig: (connectionId: string, toolName: string, enabled: boolean, confirmation: string) => Promise<void>;
  fetchConnectionStats: (id: string) => Promise<ConnectionStats | null>;
  fetchMcpLogs: (id: string) => Promise<void>;
  updateStartupPolicy: (id: string, options: { launchOnStartup?: boolean; launchOnDemand?: boolean; priority?: number; maxRetries?: number; healthCheckIntervalMs?: number }) => Promise<void>;
  openLogViewer: (serverId: string) => Promise<void>;
  closeLogViewer: () => void;
  setNewMcpName: (name: string) => void;
  setNewMcpCommand: (cmd: string) => void;
  setNewMcpArgs: (args: string) => void;
  setAddMcpDialogOpen: (open: boolean) => void;
  clearMcpError: () => void;
}

export const createMcpSlice: StateCreator<McpSlice, [], [], McpSlice> = (set, get) => ({
  mcpConnections: [],
  mcpLoading: false,
  mcpError: null,
  addMcpDialogOpen: false,
  newMcpName: '',
  newMcpCommand: 'npx',
  newMcpArgs: '-y @anthropic/mcp-server-filesystem',

  logViewerServerId: null,
  logViewerOpen: false,
  logEntries: [],
  logLoading: false,

  fetchMcpConnections: async () => {
    set({ mcpLoading: true, mcpError: null });
    try {
      const connections = await api.listMcpConnections();
      set({ mcpConnections: connections, mcpLoading: false });
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  addMcpServer: async () => {
    const { newMcpName, newMcpCommand, newMcpArgs } = get();
    if (!newMcpName.trim() || !newMcpCommand.trim()) return;

    set({ mcpLoading: true, mcpError: null });
    try {
      const args = newMcpArgs
        .split(' ')
        .map(a => a.trim())
        .filter(a => a.length > 0);
      await api.addMcpServer(newMcpName.trim(), newMcpCommand.trim(), args);
      await get().fetchMcpConnections();
      set({
        mcpLoading: false,
        addMcpDialogOpen: false,
        newMcpName: '',
        newMcpCommand: 'npx',
        newMcpArgs: '-y @anthropic/mcp-server-filesystem',
      });
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  removeMcpServer: async (id) => {
    set({ mcpLoading: true, mcpError: null });
    try {
      await api.removeMcpServer(id);
      await get().fetchMcpConnections();
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  connectMcpServer: async (id) => {
    set({ mcpLoading: true, mcpError: null });
    try {
      await api.connectMcpServer(id);
      await get().fetchMcpConnections();
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  disconnectMcpServer: async (id) => {
    set({ mcpLoading: true, mcpError: null });
    try {
      await api.disconnectMcpServer(id);
      await get().fetchMcpConnections();
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  restartMcpServer: async (id) => {
    set({ mcpLoading: true, mcpError: null });
    try {
      await api.restartMcpServer(id);
      await get().fetchMcpConnections();
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  updateToolConfig: async (connectionId, toolName, enabled, confirmation) => {
    set({ mcpLoading: true, mcpError: null });
    try {
      await api.updateMcpToolConfig(connectionId, toolName, enabled, confirmation);
      await get().fetchMcpConnections();
      set({ mcpLoading: false });
    } catch (err) {
      set({ mcpError: String(err), mcpLoading: false });
    }
  },

  fetchConnectionStats: async (id) => {
    try {
      return await api.getMcpConnectionStats(id);
    } catch {
      return null;
    }
  },

  fetchMcpLogs: async (id) => {
    set({ logLoading: true });
    try {
      const entries = await api.getMcpServerLogs(id);
      set({ logEntries: entries, logLoading: false });
    } catch {
      set({ logLoading: false });
    }
  },

  updateStartupPolicy: async (id, options) => {
    try {
      await api.updateMcpStartupPolicy(id, options);
      await get().fetchMcpConnections();
    } catch (err) {
      set({ mcpError: String(err) });
    }
  },

  openLogViewer: async (serverId) => {
    set({ logViewerServerId: serverId, logViewerOpen: true, logEntries: [] });
    await get().fetchMcpLogs(serverId);
  },

  closeLogViewer: () => {
    set({ logViewerServerId: null, logViewerOpen: false, logEntries: [] });
  },

  setNewMcpName: (name) => set({ newMcpName: name }),
  setNewMcpCommand: (cmd) => set({ newMcpCommand: cmd }),
  setNewMcpArgs: (args) => set({ newMcpArgs: args }),
  setAddMcpDialogOpen: (open) => set({ addMcpDialogOpen: open }),
  clearMcpError: () => set({ mcpError: null }),
});
