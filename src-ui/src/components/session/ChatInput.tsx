import { useState, useRef, useEffect, useCallback } from 'react';
import { Send, Loader2, X, Sparkles, FolderOpen } from 'lucide-react';
import { Dropdown, Select } from 'antd';
import { useStore } from '../../store';
import { getSettings } from '../../api/config';
import type { PersonaInfo } from '../../types';

// OS-specific default workspace path (same logic as SettingsPage)
function getDefaultWorkspacePath(): string {
  const ua = navigator.platform.toLowerCase();
  if (ua.includes('win')) return '%USERPROFILE%\\Code';
  if (ua.includes('mac')) return '~/Code';
  return '~/Code';
}

interface ChatInputProps {
  onSend: (content: string) => void | Promise<void>;
  disabled?: boolean;
  placeholder?: string;
}

export function ChatInput({ onSend, disabled, placeholder }: ChatInputProps) {
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const [input, setInput] = useState('');
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionQuery, setMentionQuery] = useState('');
  const personas = useStore((s) => s.personas);
  const fetchPersonas = useStore((s) => s.fetchPersonas);
  const activePersonaInfo = useStore((s) => s.activePersonaInfo);
  const setActivePersona = useStore((s) => s.setActivePersona);

  // Model & workspace state
  const currentSession = useStore((s) => s.currentSession);
  const models = useStore((s) => s.models);
  const defaultModel = useStore((s) => s.defaultModel);
  const updateSessionModel = useStore((s) => s.updateSessionModel);
  const updateSessionWorkspace = useStore((s) => s.updateSessionWorkspace);
  const [selectedModel, setSelectedModel] = useState(defaultModel || '');
  const [systemWorkspace, setSystemWorkspace] = useState<string>(
    // localStorage cache → OS default (instant, no async wait)
    (() => { try { return localStorage.getItem('agent_default_workspace') || getDefaultWorkspacePath(); } catch { return getDefaultWorkspacePath(); } })()
  );

  useEffect(() => {
    if (personas.length === 0) fetchPersonas();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Sync model selector with session when it changes
  useEffect(() => {
    if (currentSession?.model_id) {
      setSelectedModel(currentSession.model_id);
    }
  }, [currentSession?.model_id]);

  // Load system default workspace
  useEffect(() => {
    (async () => {
      try {
        const settings = await getSettings();
        const ws = settings['default_workspace'] || getDefaultWorkspacePath();
        setSystemWorkspace(ws);
        localStorage.setItem('agent_default_workspace', ws);
      } catch {
        const fallback = getDefaultWorkspacePath();
        setSystemWorkspace(fallback);
        localStorage.setItem('agent_default_workspace', fallback);
      }
    })();
  }, []);

  const handleSend = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || disabled) return;
    setInput('');
    setMentionOpen(false);
    await onSend(trimmed);
    inputRef.current?.focus();
  }, [input, disabled, onSend]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [handleSend]);

  const filteredPersonas = personas.filter((p) =>
    !mentionQuery || p.name.toLowerCase().includes(mentionQuery.toLowerCase()),
  );

  const handleInputChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = e.target.value;
    setInput(value);

    const atIndex = value.lastIndexOf('@');
    if (atIndex >= 0) {
      const query = value.slice(atIndex + 1);
      if (!query.includes(' ') && !query.includes('\n')) {
        setMentionQuery(query);
        setMentionOpen(true);
        if (personas.length === 0) fetchPersonas();
        return;
      }
    }
    setMentionOpen(false);
  }, [personas.length, fetchPersonas]);

  const handleSelectPersona = useCallback((persona: PersonaInfo) => {
    const atIndex = input.lastIndexOf('@');
    if (atIndex >= 0) setInput(input.slice(0, atIndex));
    setActivePersona(persona);
    setMentionOpen(false);
    inputRef.current?.focus();
  }, [input, setActivePersona]);

  const handleModelChange = useCallback(async (value: string) => {
    setSelectedModel(value);
    if (currentSession) {
      await updateSessionModel(currentSession.id, value);
    }
  }, [currentSession, updateSessionModel]);

  const handlePickWorkspace = useCallback(async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: '选择工作空间目录' });
      if (selected && currentSession) {
        await updateSessionWorkspace(currentSession.id, selected as string);
      }
    } catch {
      // Not running in Tauri
    }
  }, [currentSession, updateSessionWorkspace]);

  // Resolve workspace display: session config → system default
  const getWorkspacePath = (): string => {
    if (!currentSession?.config) return systemWorkspace;
    try {
      const cfg = JSON.parse(currentSession.config);
      return cfg.workspace_path || systemWorkspace;
    } catch {
      return systemWorkspace;
    }
  };

  const effectiveWorkspace = getWorkspacePath();

  const enabledModels = models.filter(m => m.enabled);

  return (
    <div>
      {activePersonaInfo && (
        <div className="mb-2 flex items-center gap-2">
          <span className="text-xs text-gray-400 dark:text-gray-500">persona:</span>
          <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-purple-50 dark:bg-purple-900/30 border border-purple-200 dark:border-purple-800 text-sm">
            <span>{activePersonaInfo.emoji}</span>
            <span className="font-medium text-purple-700 dark:text-purple-300">{activePersonaInfo.name}</span>
            <button
              onClick={() => setActivePersona(null)}
              className="ml-1 p-0.5 rounded hover:bg-purple-200 dark:hover:bg-purple-800 transition-colors"
            >
              <X size={12} className="text-purple-400" />
            </button>
          </span>
        </div>
      )}

      <Dropdown
        open={mentionOpen && filteredPersonas.length > 0}
        menu={{
          items: filteredPersonas.map((p) => ({
            key: p.id,
            label: (
              <div className="flex items-center gap-3 py-0.5">
                <span className="text-lg">{p.emoji}</span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-sm">{p.name}</span>
                    {p.is_default && (
                      <span className="text-[10px] px-1.5 py-0.5 rounded bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-400 leading-normal">默认</span>
                    )}
                  </div>
                  <div className="text-xs text-gray-400 truncate">{p.title}</div>
                </div>
              </div>
            ),
            onClick: () => handleSelectPersona(p),
          })),
          style: { maxHeight: 240, overflowY: 'auto' },
        }}
        placement="topLeft"
        trigger={[]}
      >
        <div className="flex items-start gap-3 bg-gray-50 dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 focus-within:border-purple-400 dark:focus-within:border-purple-500 focus-within:ring-2 focus-within:ring-purple-100 dark:focus-within:ring-purple-900/50 transition-all px-4 py-3">
          <span className="text-gray-400 dark:text-gray-500 font-mono text-sm leading-6 flex-shrink-0 select-none">&gt;</span>
          <textarea
            ref={inputRef}
            value={input}
            onChange={handleInputChange}
            onKeyDown={handleKeyDown}
            placeholder={placeholder ?? '输入消息... (@ 唤起虚拟人, Enter 发送)'}
            className="flex-1 bg-transparent resize-none focus:outline-none min-h-[24px] max-h-[160px] text-sm text-gray-700 dark:text-gray-100 placeholder-gray-400 dark:placeholder-gray-500 leading-6"
            rows={1}
            disabled={disabled}
          />
          <button
            onClick={handleSend}
            disabled={!input.trim() || disabled}
            className="flex-shrink-0 p-1.5 rounded-xl bg-gradient-to-r from-purple-600 to-indigo-600 text-white disabled:opacity-30 disabled:cursor-not-allowed hover:shadow-md transition-all mt-0.5"
          >
            {disabled ? <Loader2 size={16} className="animate-spin" /> : <Send size={16} />}
          </button>
        </div>
      </Dropdown>

      {/* Bottom toolbar — model + workspace */}
      <div className="flex items-center justify-start gap-2.5 pt-2 mt-2 border-t border-gray-100 dark:border-gray-700/50">
        {/* Model selector pill */}
        <div className="inline-flex items-center gap-1.5 px-2.5 py-1 bg-gray-50 dark:bg-gray-800/60 border border-gray-150 dark:border-gray-700/50 rounded-full">
          <Sparkles size={11} className="text-purple-500 flex-shrink-0" />
          <Select
            value={selectedModel || undefined}
            onChange={handleModelChange}
            size="small"
            variant="borderless"
            popupMatchSelectWidth={false}
            className="text-[11px] min-w-[80px]"
            dropdownStyle={{ minWidth: 200 }}
            options={enabledModels.map((m) => ({
              value: m.id,
              label: m.display_name,
              provider: m.provider,
            }))}
            optionRender={(option) => (
              <div className="flex items-center justify-between gap-4 py-0.5">
                <span className="text-xs">{option.data.label}</span>
                <span className="text-[10px] text-gray-400">{option.data.provider}</span>
              </div>
            )}
            notFoundContent="没有可用模型"
          />
        </div>

        {/* Workspace selector pill */}
        {effectiveWorkspace ? (
          <button onClick={handlePickWorkspace} className="inline-flex items-center gap-1 px-2.5 py-1 bg-amber-50/70 dark:bg-amber-900/20 border border-amber-200/70 dark:border-amber-800/50 rounded-full group hover:bg-amber-100 dark:hover:bg-amber-900/30 transition-colors cursor-pointer">
            <FolderOpen size={11} className="text-amber-500 flex-shrink-0" />
            <span className="text-[11px] text-amber-700 dark:text-amber-300">{effectiveWorkspace}</span>
            <svg className="w-2.5 h-2.5 text-amber-400 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" /></svg>
          </button>
        ) : (
          <button onClick={handlePickWorkspace} className="inline-flex items-center gap-1 px-2.5 py-1 text-[11px] text-gray-400 hover:text-amber-600 dark:hover:text-amber-400 hover:bg-amber-50/50 dark:hover:bg-amber-900/20 border border-transparent hover:border-amber-200/70 dark:hover:border-amber-800/50 rounded-full transition-all">
            <FolderOpen size={11} />
            <span>工作空间</span>
          </button>
        )}
      </div>
    </div>
  );
}
