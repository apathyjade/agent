import { useState, useRef, useEffect, useCallback } from 'react';
import { Send, Loader2, X } from 'lucide-react';
import { Dropdown } from 'antd';
import { useStore } from '../store';
import type { PersonaInfo } from '../types';

interface ChatInputProps {
  /** Called when the user sends a message. The input is cleared after onSend resolves. */
  onSend: (content: string) => void | Promise<void>;
  /** Disables the input (e.g. during streaming) */
  disabled?: boolean;
  /** Placeholder text for the textarea */
  placeholder?: string;
}

export function ChatInput({ onSend, disabled, placeholder }: ChatInputProps) {
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // ── Input state ──
  const [input, setInput] = useState('');

  // ── @mention persona state ──
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionQuery, setMentionQuery] = useState('');
  const personas = useStore((s) => s.personas);
  const fetchPersonas = useStore((s) => s.fetchPersonas);
  const activePersonaInfo = useStore((s) => s.activePersonaInfo);
  const setActivePersona = useStore((s) => s.setActivePersona);

  // Pre-fetch personas on mount so @mention is ready
  useEffect(() => {
    if (personas.length === 0) {
      fetchPersonas();
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Send handler ──
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

  // ── @mention handling ──
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
        if (personas.length === 0) {
          fetchPersonas();
        }
        return;
      }
    }
    setMentionOpen(false);
  }, [personas.length, fetchPersonas]);

  const handleSelectPersona = useCallback((persona: PersonaInfo) => {
    const atIndex = input.lastIndexOf('@');
    if (atIndex >= 0) {
      setInput(input.slice(0, atIndex));
    }
    setActivePersona(persona);
    setMentionOpen(false);
    inputRef.current?.focus();
  }, [input, setActivePersona]);

  return (
    <div>
      {/* Active persona badge */}
      {activePersonaInfo && (
        <div className="mb-2 flex items-center gap-2 px-2">
          <span className="text-xs font-medium text-gray-500 dark:text-gray-400">当前虚拟人：</span>
          <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-purple-50 dark:bg-purple-900/30 border border-purple-200 dark:border-purple-800 text-sm">
            <span>{activePersonaInfo.emoji}</span>
            <span className="font-medium text-purple-700 dark:text-purple-300">{activePersonaInfo.name}</span>
            <span className="text-xs text-purple-500 dark:text-purple-400">{activePersonaInfo.title}</span>
            <button
              onClick={() => setActivePersona(null)}
              className="ml-1 p-0.5 rounded hover:bg-purple-200 dark:hover:bg-purple-800 transition-colors"
              title="取消选择"
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
        <div className="relative bg-gray-50 dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700 focus-within:border-purple-400 dark:focus-within:border-purple-500 focus-within:ring-2 focus-within:ring-purple-100 dark:focus-within:ring-purple-900/50 transition-all">
          <textarea
            ref={inputRef}
            value={input}
            onChange={handleInputChange}
            onKeyDown={handleKeyDown}
            placeholder={placeholder ?? '输入消息... (@ 唤起虚拟人, Shift+Enter 换行)'}
            className="w-full bg-transparent px-4 py-3 pr-14 resize-none focus:outline-none min-h-[80px] max-h-[240px] text-sm dark:text-gray-100 dark:placeholder-gray-400"
            rows={2}
            disabled={disabled}
          />
          <button
            onClick={handleSend}
            disabled={!input.trim() || disabled}
            className="absolute right-2 bottom-2 p-2 rounded-xl bg-gradient-to-r from-purple-600 to-indigo-600 text-white disabled:opacity-30 disabled:cursor-not-allowed hover:shadow-md transition-all"
          >
            {disabled ? <Loader2 size={18} className="animate-spin" /> : <Send size={18} />}
          </button>
        </div>
      </Dropdown>
    </div>
  );
}
