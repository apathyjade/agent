import { useState, useRef, useEffect, useCallback, memo } from 'react';
import { Send, Loader2, Sparkles, Wrench, CheckCircle, XCircle, ChevronDown, ArrowDown } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useStore } from '../store';
import { MessageBubble } from './MessageBubble';
import type { Message } from '../types';

const StreamingMessage = memo(function StreamingMessage({ content }: { content: string }) {
  return (
    <div className="flex gap-4">
      <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center flex-shrink-0 shadow-sm">
        <Sparkles size={16} className="text-white" />
      </div>
      <div className="flex-1 min-w-0">
        <div className="rounded-2xl px-4 py-3 bg-white dark:bg-gray-800 border border-gray-100 dark:border-gray-700 shadow-sm">
          <div className="prose prose-sm max-w-none prose-p:my-2 prose-pre:my-0 prose-pre:p-0 prose-pre:border-0 prose-code:bg-gray-100 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded prose-code:text-pink-600 prose-code:font-mono prose-code:text-sm prose-ul:my-1 prose-ol:my-1 prose-li:my-0.5 prose-headings:my-2 prose-h1:text-lg prose-h2:text-base prose-h3:text-sm prose-hr:my-3 prose-blockquote:border-l-purple-400 prose-blockquote:text-gray-500 prose-blockquote:not-italic prose-a:text-purple-600">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
            <span className="inline-block w-2 h-4 ml-1 bg-purple-500 animate-pulse" />
          </div>
        </div>
      </div>
    </div>
  );
});

const MessageList = memo(function MessageList({ messages }: { messages: Message[] }) {
  return (
    <>
      {messages.map((msg) => (
        <MessageBubble key={msg.id} message={msg} />
      ))}
    </>
  );
});

export function ChatArea() {
  const currentConversation = useStore((s) => s.currentConversation);
  const messages = useStore((s) => s.messages);
  const loading = useStore((s) => s.loading);
  const error = useStore((s) => s.error);
  const isStreaming = useStore((s) => s.isStreaming);
  const streamingContent = useStore((s) => s.streamingContent);
  const activeToolCalls = useStore((s) => s.activeToolCalls);
  const sendMessageStream = useStore((s) => s.sendMessageStream);
  const models = useStore((s) => s.models);
  const updateConversationModel = useStore((s) => s.updateConversationModel);
  const setError = useStore((s) => s.setError);

  const [input, setInput] = useState('');
  const [showModelPicker, setShowModelPicker] = useState(false);
  const [isNearBottom, setIsNearBottom] = useState(true);
  const [showScrollButton, setShowScrollButton] = useState(false);
  
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const autoScrollTimeoutRef = useRef<number | null>(null);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = 'smooth') => {
    if (messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior });
    }
  }, []);

  const handleScroll = useCallback(() => {
    const container = messagesContainerRef.current;
    if (!container) return;
    
    const { scrollTop, scrollHeight, clientHeight } = container;
    const distanceFromBottom = scrollHeight - scrollTop - clientHeight;
    const wasNearBottom = distanceFromBottom < 200;
    
    setIsNearBottom(wasNearBottom);
    setShowScrollButton(!wasNearBottom && (isStreaming || messages.length > 5));
  }, [isStreaming, messages.length]);

  useEffect(() => {
    if (isNearBottom && (messages.length > 0 || streamingContent)) {
      if (autoScrollTimeoutRef.current) {
        clearTimeout(autoScrollTimeoutRef.current);
      }
      autoScrollTimeoutRef.current = window.setTimeout(() => {
        scrollToBottom('auto');
      }, 50);
    }
    return () => {
      if (autoScrollTimeoutRef.current) {
        clearTimeout(autoScrollTimeoutRef.current);
      }
    };
  }, [messages.length, streamingContent, activeToolCalls.length, isNearBottom, scrollToBottom]);

  useEffect(() => {
    if (inputRef.current && !isStreaming) {
      inputRef.current.focus();
    }
  }, [currentConversation, isStreaming]);

  useEffect(() => {
    if (!showModelPicker) return;
    const handleClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('.model-picker')) setShowModelPicker(false);
    };
    document.addEventListener('click', handleClick);
    return () => document.removeEventListener('click', handleClick);
  }, [showModelPicker]);

  const handleSend = useCallback(async () => {
    if (!input.trim() || loading || isStreaming) return;
    const content = input.trim();
    setInput('');
    setIsNearBottom(true);
    setShowScrollButton(false);
    await sendMessageStream(content);
  }, [input, loading, isStreaming, sendMessageStream]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [handleSend]);

  const handleRetry = useCallback(() => {
    setError(null);
    handleSend();
  }, [setError, handleSend]);

  if (!currentConversation) return null;

  return (
    <div className="flex-1 flex flex-col bg-white dark:bg-gray-900 min-h-0 transition-colors">
      <div className="px-6 py-3 border-b border-gray-100 dark:border-gray-700 flex items-center justify-between bg-white/80 dark:bg-gray-900/80 backdrop-blur-sm sticky top-0 z-10">
        <div className="flex items-center gap-2">
          <div className="relative model-picker">
              <button
                onClick={(e) => { e.stopPropagation(); setShowModelPicker(!showModelPicker); }}
                className="flex items-center gap-1 text-xs text-gray-500 dark:text-gray-400 hover:text-purple-600 dark:hover:text-purple-400 transition-colors"
              >
                {models.find(m => m.id === currentConversation.model_id)?.display_name || currentConversation.model_id}
                <ChevronDown size={12} />
              </button>
              {showModelPicker && (
                <div className="absolute left-0 top-full mt-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg shadow-lg z-20 py-1 min-w-[200px] max-h-48 overflow-y-auto">
                  {models.filter(m => m.enabled).map((model) => (
                    <button
                      key={model.id}
                      onClick={async () => {
                        await updateConversationModel(currentConversation.id, model.id);
                        setShowModelPicker(false);
                      }}
                      className={`w-full text-left px-3 py-2 text-sm hover:bg-purple-50 dark:hover:bg-purple-900/30 transition-colors ${
                        currentConversation.model_id === model.id
                          ? 'text-purple-600 dark:text-purple-400 bg-purple-50 dark:bg-purple-900/30 font-medium'
                          : 'text-gray-700 dark:text-gray-300'
                      }`}
                    >
                      <div className="flex items-center justify-between">
                        <span>{model.display_name}</span>
                        <span className="text-xs text-gray-400 dark:text-gray-500">{model.provider}</span>
                      </div>
                    </button>
                  ))}
                  {models.filter(m => m.enabled).length === 0 && (
                    <div className="px-3 py-2 text-sm text-gray-400 dark:text-gray-500">没有可用模型</div>
                  )}
                </div>
              )}
            </div>
          </div>
          <div className="flex items-center gap-3">
          {currentConversation.system_prompt && (
            <span className="px-2 py-1 bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 text-xs rounded-lg">提示词已加载</span>
          )}
          {isStreaming && (
            <div className="flex items-center gap-2 text-purple-600 text-sm">
              <Loader2 size={14} className="animate-spin" />
              生成中...
            </div>
          )}
        </div>
      </div>

      <div 
        ref={messagesContainerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto min-h-0 relative"
      >
        <div className="max-w-3xl mx-auto px-6 py-8 space-y-6">
          <MessageList messages={messages} />

          {isStreaming && activeToolCalls.length > 0 && (
            <div className="space-y-2">
              {activeToolCalls.map((tc) => (
                <ToolCallIndicator key={tc.id} toolCall={tc} />
              ))}
            </div>
          )}

          {isStreaming && streamingContent && (
            <StreamingMessage content={streamingContent} />
          )}

          {loading && !isStreaming && (
            <div className="flex items-center gap-3 text-gray-400 dark:text-gray-500 py-4">
              <Loader2 size={18} className="animate-spin" />
              <span className="text-sm">思考中...</span>
            </div>
          )}

          {error && (
            <div className="p-4 bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-800 rounded-xl" role="alert">
              <div className="flex items-start gap-2 text-red-600 dark:text-red-400 text-sm">
                <XCircle size={16} className="flex-shrink-0 mt-0.5" />
                <div className="flex-1">
                  <span>{error}</span>
                  <div className="mt-2 flex gap-2">
                    <button
                      onClick={handleRetry}
                      className="text-xs text-red-500 hover:text-red-700 dark:text-red-400 dark:hover:text-red-300 font-medium"
                    >
                      重试
                    </button>
                  </div>
                </div>
              </div>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>

        {showScrollButton && (
          <button
            onClick={() => {
              scrollToBottom('smooth');
              setIsNearBottom(true);
              setShowScrollButton(false);
            }}
            className="absolute bottom-4 left-1/2 -translate-x-1/2 p-2 rounded-full bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 shadow-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-all animate-in fade-in"
          >
            <ArrowDown size={18} className="text-gray-600 dark:text-gray-400" />
          </button>
        )}
      </div>

      <div className="border-t border-gray-100 dark:border-gray-700 bg-white dark:bg-gray-900 p-4 transition-colors">
        <div className="max-w-3xl mx-auto">
          <div className="relative bg-gray-50 dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700 focus-within:border-purple-400 dark:focus-within:border-purple-500 focus-within:ring-2 focus-within:ring-purple-100 dark:focus-within:ring-purple-900/50 transition-all">
            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入消息... (Shift+Enter 换行)"
              className="w-full bg-transparent px-4 py-3 pr-14 resize-none focus:outline-none min-h-[56px] max-h-[200px] text-sm dark:text-gray-100 dark:placeholder-gray-400"
              rows={1}
              disabled={isStreaming}
            />
            <button
              onClick={handleSend}
              disabled={!input.trim() || loading || isStreaming}
              className="absolute right-2 bottom-2 p-2 rounded-xl bg-gradient-to-r from-purple-600 to-indigo-600 text-white disabled:opacity-30 disabled:cursor-not-allowed hover:shadow-md transition-all"
            >
              {isStreaming ? <Loader2 size={18} className="animate-spin" /> : <Send size={18} />}
            </button>
          </div>
          <p className="text-xs text-gray-400 dark:text-gray-500 mt-2 text-center">AI 可能会产生不准确的信息，请验证重要信息</p>
        </div>
      </div>
    </div>
  );
}

function ToolCallIndicator({ toolCall }: { toolCall: { id: string; name: string; status: string; result?: string } }) {
  return (
    <div className="flex items-center gap-2 p-3 bg-gray-50 dark:bg-gray-800 rounded-xl text-sm border border-gray-100 dark:border-gray-700">
      <Wrench size={14} className="text-purple-500" />
      <span className="text-gray-700 dark:text-gray-300 capitalize">{toolCall.name.replace('_', ' ')}</span>
      {toolCall.status === 'calling' && (
        <Loader2 size={14} className="animate-spin text-purple-500" />
      )}
      {toolCall.status === 'completed' && (
        <CheckCircle size={14} className="text-green-500" />
      )}
      {toolCall.result && (
        <span className="text-xs text-gray-400 truncate max-w-[200px]">
          {toolCall.result.substring(0, 50)}...
        </span>
      )}
    </div>
  );
}
