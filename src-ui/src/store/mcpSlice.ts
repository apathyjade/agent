import type { StateCreator } from 'zustand';
import type { McpConnectionInfo, ConnectionStats, RuntimeSuggestion } from '../types';
import * as api from '../api/tauri';

export interface McpSlice {
  mcpConnections: McpConnectionInfo[];
  mcpLoading: boolean;
  mcpError: string | null;
  addMcpDialogOpen: boolean;
  newMcpName: string;
  newMcpCommand: string;
  newMcpArgs: string;
  /** Runtime suggestion for the current MCP command (for add dialog) */
  mcpRuntimeSuggestion: RuntimeSuggestion | null;
  mcpRuntimeChecking: boolean;

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
  /** Check runtime availability for the current MCP command */
  checkMcpCommandRuntime: (command: string) => Promise<void>;
}

export const createMcpSlice: StateCreator<McpSlice, [], [], McpSlice> = (set, get) => ({
  mcpConnections: [],
  mcpLoading: false,
  mcpError: null,
  addMcpDialogOpen: false,
  newMcpName: '',
  newMcpCommand: 'npx',
  newMcpArgs: '-y @anthropic/mcp-server-filesystem',
  mcpRuntimeSuggestion: null,
  mcpRuntimeChecking: false,

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
      // Infer runtime type from command
      const rtSuggestion = await api.suggestRuntimeForCommand(newMcpCommand.trim());
      const rt = rtSuggestion.runtime_type ?? undefined;
      await api.addMcpServer(newMcpName.trim(), newMcpCommand.trim(), args, rt);
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
  setNewMcpCommand: (cmd) => {
    set({ newMcpCommand: cmd });
    // Debounced runtime check could be added here, but for now trigger on command change
    get().checkMcpCommandRuntime(cmd);
  },
  setNewMcpArgs: (args) => set({ newMcpArgs: args }),
  setAddMcpDialogOpen: (open) => {
    set({ addMcpDialogOpen: open });
    if (!open) {
      // Reset suggestion when dialog closes
      set({ mcpRuntimeSuggestion: null });
    } else {
      // Check current command when opening
      const { newMcpCommand } = get();
      if (newMcpCommand.trim()) {
        get().checkMcpCommandRuntime(newMcpCommand);
      }
    }
  },
  clearMcpError: () => set({ mcpError: null }),

  checkMcpCommandRuntime: async (command) => {
    if (!command.trim()) {
      set({ mcpRuntimeSuggestion: null, mcpRuntimeChecking: false });
      return;
    }
    set({ mcpRuntimeChecking: true });
    try {
      const suggestion = await api.suggestRuntimeForCommand(command);
      set({ mcpRuntimeSuggestion: suggestion, mcpRuntimeChecking: false });
    } catch {
      set({ mcpRuntimeSuggestion: null, mcpRuntimeChecking: false });
    }
  },
});
