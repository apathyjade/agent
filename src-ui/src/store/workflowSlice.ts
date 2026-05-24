import type { StateCreator } from 'zustand';
import type { WorkflowInfo, WorkflowRunRecord } from '../types';
import * as api from '../api/tauri';

export interface WorkflowSlice {
  workflows: WorkflowInfo[];
  workflowRuns: WorkflowRunRecord[];
  workflowLoading: boolean;
  workflowResult: string | null;
  workflowError: string | null;
  workflowVars: Record<string, string>;
  workflowSecretKeys: string[];
  generateDialogOpen: boolean;
  generateDescription: string;

  fetchWorkflows: () => Promise<void>;
  runWorkflow: (name: string) => Promise<void>;
  fetchWorkflowRuns: () => Promise<void>;
  clearWorkflowResult: () => void;
  pauseWorkflowSchedule: (name: string) => Promise<void>;
  resumeWorkflowSchedule: (name: string) => Promise<void>;
  fetchWorkflowVars: () => Promise<void>;
  fetchWorkflowSecretKeys: () => Promise<void>;
  setWorkflowVar: (key: string, value: string) => Promise<void>;
  deleteWorkflowVar: (key: string) => Promise<void>;
  setWorkflowSecret: (key: string, value: string) => Promise<void>;
  deleteWorkflowSecret: (key: string) => Promise<void>;
  generateWorkflow: (description: string) => Promise<void>;
  setGenerateDialogOpen: (open: boolean) => void;
  setGenerateDescription: (desc: string) => void;
}

export const createWorkflowSlice: StateCreator<WorkflowSlice, [], [], WorkflowSlice> = (set, get) => ({
  workflows: [],
  workflowRuns: [],
  workflowLoading: false,
  workflowResult: null,
  workflowError: null,
  workflowVars: {},
  workflowSecretKeys: [],
  generateDialogOpen: false,
  generateDescription: '',

  fetchWorkflows: async () => {
    set({ workflowLoading: true, workflowError: null });
    try {
      const workflows = await api.listWorkflows();
      set({ workflows, workflowLoading: false });
    } catch (err) {
      set({ workflowError: String(err), workflowLoading: false });
    }
  },

  runWorkflow: async (name) => {
    set({ workflowLoading: true, workflowError: null, workflowResult: null });
    try {
      const result = await api.runWorkflow(name);
      set({ workflowResult: result, workflowLoading: false });
      await get().fetchWorkflowRuns();
    } catch (err) {
      set({ workflowError: String(err), workflowLoading: false });
    }
  },

  fetchWorkflowRuns: async () => {
    try {
      const runs = await api.listWorkflowRuns();
      set({ workflowRuns: runs });
    } catch (err) {
      // silently fail
    }
  },

  clearWorkflowResult: () => set({ workflowResult: null, workflowError: null }),

  pauseWorkflowSchedule: async (name) => {
    try {
      await api.pauseWorkflowSchedule(name);
      await get().fetchWorkflows();
    } catch (err) {
      // silently fail
    }
  },

  resumeWorkflowSchedule: async (name) => {
    try {
      await api.resumeWorkflowSchedule(name);
      await get().fetchWorkflows();
    } catch (err) {
      // silently fail
    }
  },

  fetchWorkflowVars: async () => {
    try {
      const vars = await api.listWorkflowVars();
      set({ workflowVars: vars });
    } catch (err) { console.error('workflowSlice error:', err); }
  },
  fetchWorkflowSecretKeys: async () => {
    try {
      const keys = await api.listWorkflowSecrets();
      set({ workflowSecretKeys: keys });
    } catch (err) { console.error('workflowSlice error:', err); }
  },
  setWorkflowVar: async (key, value) => {
    await api.setWorkflowVar(key, value);
    await get().fetchWorkflowVars();
  },
  deleteWorkflowVar: async (key) => {
    await api.deleteWorkflowVar(key);
    await get().fetchWorkflowVars();
  },
  setWorkflowSecret: async (key, value) => {
    await api.setWorkflowSecret(key, value);
    await get().fetchWorkflowSecretKeys();
  },
  deleteWorkflowSecret: async (key) => {
    await api.deleteWorkflowSecret(key);
    await get().fetchWorkflowSecretKeys();
  },
  generateWorkflow: async (description) => {
    set({ workflowLoading: true, workflowError: null, workflowResult: null });
    try {
      const result = await api.generateWorkflow(description);
      set({ workflowResult: result, workflowLoading: false, generateDialogOpen: false });
      await get().fetchWorkflows();
    } catch (err) {
      set({ workflowError: String(err), workflowLoading: false });
    }
  },
  setGenerateDialogOpen: (open) => set({ generateDialogOpen: open }),
  setGenerateDescription: (desc) => set({ generateDescription: desc }),
});
