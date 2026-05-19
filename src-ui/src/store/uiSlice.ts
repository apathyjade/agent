import type { StateCreator } from 'zustand';

export interface Toast {
  id: string;
  type: 'success' | 'error' | 'warning' | 'info';
  message: string;
}

export interface UISlice {
  loading: boolean;
  error: string | null;
  streamingContent: string;
  isStreaming: boolean;
  activeToolCalls: Array<{ id: string; name: string; status: string; result?: string }>;
  darkMode: boolean;
  toasts: Toast[];

  setError: (error: string | null) => void;
  toggleDarkMode: () => void;
  setDarkMode: (dark: boolean) => void;
  addToast: (type: Toast['type'], message: string) => void;
  removeToast: (id: string) => void;
  setActiveToolCalls: (calls: UISlice['activeToolCalls']) => void;
  setStreamingContent: (content: string) => void;
  appendStreamingContent: (content: string) => void;
  setLoading: (loading: boolean) => void;
}

export const createUISlice: StateCreator<UISlice, [], [], UISlice> = (set, get) => ({
  loading: false,
  error: null,
  streamingContent: '',
  isStreaming: false,
  activeToolCalls: [],
  darkMode: localStorage.getItem('darkMode') === 'true',
  toasts: [],

  setError: (error) => set({ error }),

  toggleDarkMode: () => {
    set((state) => {
      const next = !state.darkMode;
      localStorage.setItem('darkMode', String(next));
      if (next) {
        document.documentElement.classList.add('dark');
      } else {
        document.documentElement.classList.remove('dark');
      }
      return { darkMode: next };
    });
  },

  setDarkMode: (dark) => {
    localStorage.setItem('darkMode', String(dark));
    if (dark) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
    set({ darkMode: dark });
  },

  addToast: (type, message) => {
    const id = Date.now().toString() + Math.random().toString(36).slice(2);
    set((state) => ({
      toasts: [...state.toasts, { id, type, message }],
    }));
    setTimeout(() => {
      get().removeToast(id);
    }, 4000);
  },

  removeToast: (id) => {
    set((state) => ({
      toasts: state.toasts.filter((t) => t.id !== id),
    }));
  },

  setActiveToolCalls: (calls) => set({ activeToolCalls: calls }),
  setStreamingContent: (content) => set({ streamingContent: content }),
  appendStreamingContent: (content) =>
    set((state) => ({ streamingContent: state.streamingContent + content })),
  setLoading: (loading) => set({ loading }),
});
