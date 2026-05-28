import { useState, useRef, useEffect, useCallback } from 'react';
import { Send, Loader2, X } from 'lucide-react';
import { Dropdown } from 'antd';
import { useStore } from '../../store';
import type { PersonaInfo } from '../../types';

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

  useEffect(() => {
    if (personas.length === 0) fetchPersonas();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

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
    </div>
  );
}
