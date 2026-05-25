import { useState, useRef, useEffect, useCallback, memo } from 'react';
import {
  Loader2,
  Sparkles,
  CheckCircle,
  XCircle,
  ArrowDown,
  FileText,
} from 'lucide-react';
import { Select } from 'antd';
import { useStore } from '../store';
import { MessageBubble } from './MessageBubble';
import { ChatInput } from './ChatInput';
import type { Message } from '../types';

const StreamingMessage = memo(function StreamingMessage({
  content,
}: {
  content: string;
}) {
  return (
    <div className="group">
      <hr className="tui-divider" />
      <div className="tui-header">
        <Sparkles size={12} className="text-purple-500" />
        <span className="text-purple-500 font-semibold">Agent</span>
        <span className="tui-cursor">streaming</span>
      </div>
      <div className="tui-content">
        {content}
        <span className="tui-cursor" />
      </div>
    </div>
  );
});

const SummaryDivider = memo(function SummaryDivider({
  summary,
  messageCount,
}: {
  summary: string;
  messageCount: number;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div className="group">
      <hr className="tui-divider" />
      <div
        className="tui-header cursor-pointer select-none"
        onClick={() => setExpanded(!expanded)}
      >
        <FileText size={12} className="text-amber-500" />
        <span className="text-amber-600 dark:text-amber-400 font-medium">
          Summary
        </span>
        <span className="text-gray-400">{messageCount} messages compressed</span>
        <span className="ml-auto text-gray-400">
          {expanded ? 'hide' : 'show'}
        </span>
      </div>
      {expanded && (
        <div className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed px-1 py-2">
          {summary}
        </div>
      )}
    </div>
  );
});

const MessageList = memo(function MessageList({
  messages,
}: {
  messages: Message[];
}) {
  return (
    <>
      {messages.map((msg) => (
        <MessageBubble key={msg.id} message={msg} />
      ))}
    </>
  );
});

export function ChatArea() {
  const currentSession = useStore((s) => s.currentSession);
  const messages = useStore((s) => s.messages);
  const loading = useStore((s) => s.loading);
  const error = useStore((s) => s.error);
  const isStreaming = useStore((s) => s.isStreaming);
  const streamingContent = useStore((s) => s.streamingContent);
  const activeToolCalls = useStore((s) => s.activeToolCalls);
  const currentPhase = useStore((s) => s.currentPhase);
  const sendMessageStream = useStore((s) => s.sendMessageStream);
  const models = useStore((s) => s.models);
  const personas = useStore((s) => s.personas);
  const updateSessionModel = useStore((s) => s.updateSessionModel);
  const setError = useStore((s) => s.setError);
  const summaries = useStore((s) => s.summaries);
  const fetchSummaries = useStore((s) => s.fetchSummaries);

  const [isNearBottom, setIsNearBottom] = useState(true);
  const [showScrollButton, setShowScrollButton] = useState(false);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const autoScrollTimeoutRef = useRef<number | null>(null);

  const scrollToBottom = useCallback(
    (behavior: ScrollBehavior = 'smooth') => {
      if (messagesEndRef.current) {
        messagesEndRef.current.scrollIntoView({ behavior });
      }
    },
    [],
  );

  const handleScroll = useCallback(() => {
    const container = messagesContainerRef.current;
    if (!container) return;
    const { scrollTop, scrollHeight, clientHeight } = container;
    const distanceFromBottom = scrollHeight - scrollTop - clientHeight;
    const wasNearBottom = distanceFromBottom < 200;
    setIsNearBottom(wasNearBottom);
    setShowScrollButton(
      !wasNearBottom && (isStreaming || messages.length > 5),
    );
  }, [isStreaming, messages.length]);

  useEffect(() => {
    if (isNearBottom && (messages.length > 0 || streamingContent)) {
      if (autoScrollTimeoutRef.current)
        clearTimeout(autoScrollTimeoutRef.current);
      autoScrollTimeoutRef.current = window.setTimeout(
        () => scrollToBottom('auto'),
        50,
      );
    }
    return () => {
      if (autoScrollTimeoutRef.current)
        clearTimeout(autoScrollTimeoutRef.current);
    };
  }, [
    messages.length,
    streamingContent,
    activeToolCalls.length,
    isNearBottom,
    scrollToBottom,
  ]);

  useEffect(() => {
    if (currentSession?.id) fetchSummaries(currentSession.id);
  }, [currentSession?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleChatSend = useCallback(
    async (content: string) => {
      setIsNearBottom(true);
      setShowScrollButton(false);
      const pid = useStore.getState().activePersonaId;
      await sendMessageStream(content, undefined, pid ?? undefined);
    },
    [sendMessageStream],
  );

  const handleRetry = useCallback(() => setError(null), [setError]);

  if (!currentSession) return null;

  return (
    <div className="h-full flex flex-col bg-white dark:bg-gray-900 transition-colors">
      {/* Top status bar */}
      <div className="flex items-center justify-between px-6 py-3 border-b border-gray-100 dark:border-gray-700 bg-white/80 dark:bg-gray-900/80 backdrop-blur-sm flex-shrink-0">
        <div className="flex items-center gap-3">
          <Select
            value={currentSession.model_id}
            onChange={async (value) => {
              await updateSessionModel(currentSession.id, value);
            }}
            size="small"
            variant="borderless"
            popupMatchSelectWidth={false}
            className="text-xs"
            options={models
              .filter((m) => m.enabled)
              .map((m) => ({
                value: m.id,
                label: m.display_name,
                provider: m.provider,
              }))}
            optionRender={(option) => (
              <div className="flex items-center justify-between gap-3">
                <span>{option.data.label}</span>
                <span className="text-xs text-gray-400 opacity-60">
                  {option.data.provider}
                </span>
              </div>
            )}
            notFoundContent="没有可用模型"
          />
          {currentSession.system_prompt && (
            <span className="px-2 py-1 bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 text-xs rounded-lg">
              提示词已加载
            </span>
          )}
          {(() => {
            const sessionPersona = currentSession.persona_id
              ? personas.find(p => p.id === currentSession.persona_id)
              : null;
            return sessionPersona ? (
              <span className="inline-flex items-center gap-1 px-2 py-1 bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 text-xs rounded-lg">
                <span>{sessionPersona.emoji}</span>
                <span>{sessionPersona.name}</span>
              </span>
            ) : null;
          })()}
          {(() => {
            const cfg = currentSession.config ? (() => { try { return JSON.parse(currentSession.config); } catch { return null; } })() : null;
            const tools = cfg?.enabled_tools;
            return tools && tools.length > 0 ? (
              <span className="text-xs text-gray-400 dark:text-gray-500" title={tools.join(', ')}>
                {tools.length} tools
              </span>
            ) : null;
          })()}
        </div>
        <div className="flex items-center gap-4 text-xs text-gray-400 dark:text-gray-500">
          <span>{messages.length} 条消息</span>
          {isStreaming && (
            <span className="flex items-center gap-1 text-purple-600">
              <Loader2 size={12} className="animate-spin" />
              生成中...
            </span>
          )}
        </div>
      </div>

      {/* Message stream */}
      <div
        ref={messagesContainerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto relative"
      >
        <div className="px-6 py-6 max-w-4xl mx-auto">
          {messages.length === 0 && !isStreaming && (
            <div className="flex flex-col items-center justify-center py-24 text-center select-none">
              <Sparkles size={28} className="text-gray-300 dark:text-gray-600 mb-4" />
              <p className="text-sm text-gray-400 dark:text-gray-500 mb-1">
                暂无消息
              </p>
              <p className="text-xs text-gray-300 dark:text-gray-600">
                输入消息开始对话
              </p>
            </div>
          )}

          <MessageList messages={messages} />

          {/* Summary dividers */}
          {summaries.length > 0 && messages.length > 0 && (
            <div className="space-y-0">
              {summaries.map((s) => (
                <SummaryDivider
                  key={s.id}
                  summary={s.summary}
                  messageCount={Math.max(
                    3,
                    Math.round(s.original_token_count / 30),
                  )}
                />
              ))}
            </div>
          )}

          {/* Phase indicator */}
          {isStreaming && currentPhase && activeToolCalls.length === 0 && !streamingContent && (
            <div className="flex items-center gap-2 px-6 py-2">
              <Loader2 size={12} className="animate-spin text-purple-500" />
              <span className="text-xs text-purple-500 dark:text-purple-400 capitalize">
                {currentPhase === 'classifying' && '正在分析意图...'}
                {currentPhase === 'building_context' && '正在构建上下文...'}
                {currentPhase === 'thinking' && '正在思考...'}
                {currentPhase === 'executing_tool' && '正在执行工具...'}
                {currentPhase === 'processing_result' && '正在处理结果...'}
                {currentPhase === 'completed' && '完成'}
                {!['classifying','building_context','thinking','executing_tool','processing_result','completed'].includes(currentPhase) && currentPhase}
              </span>
            </div>
          )}

          {/* Process steps — tool calls & phase info shown inline */}
          {isStreaming && (
            <div className="space-y-0">
              {/* Pre-stream phase hint */}
              {currentPhase && activeToolCalls.length === 0 && !streamingContent && (
                <div className="group">
                  <hr className="tui-divider" />
                  <div className="tui-header">
                    {currentPhase === 'classifying' && <><Loader2 size={12} className="animate-spin text-purple-500" /><span className="text-purple-600 dark:text-purple-400">正在分析意图...</span></>}
                    {currentPhase === 'building_context' && <><Loader2 size={12} className="animate-spin text-blue-500" /><span className="text-blue-600 dark:text-blue-400">正在构建上下文...</span></>}
                    {currentPhase === 'thinking' && <><Loader2 size={12} className="animate-spin text-amber-500" /><span className="text-amber-600 dark:text-amber-400">思考中...</span></>}
                  </div>
                </div>
              )}

              {/* Tool call & result cards */}
              {activeToolCalls.map((tc) => (
                <div key={tc.id} className="group">
                  <hr className="tui-divider" />
                  <div className="tui-header">
                    {tc.status === 'calling' && <><Loader2 size={12} className="animate-spin text-orange-500" /><span className="text-orange-600 dark:text-orange-400 font-medium">正在执行: {tc.name.replace(/_/g, ' ')}</span></>}
                    {tc.status === 'completed' && <><CheckCircle size={12} className="text-green-500" /><span className="text-green-600 dark:text-green-400 font-medium">已完成: {tc.name.replace(/_/g, ' ')}</span></>}
                    {tc.status === 'failed' && <><XCircle size={12} className="text-red-500" /><span className="text-red-600 dark:text-red-400 font-medium">执行失败: {tc.name.replace(/_/g, ' ')}</span></>}
                  </div>
                  {(tc.status === 'completed' || tc.status === 'failed') && tc.result && (
                    <div className={`mx-1 my-1.5 p-2 rounded-lg text-xs font-mono whitespace-pre-wrap max-h-48 overflow-y-auto ${
                      tc.status === 'failed'
                        ? 'bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 text-red-700 dark:text-red-300'
                        : 'bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 text-gray-600 dark:text-gray-400'
                    }`}>
                      {tc.status === 'failed'
                        ? tc.result.replace(/^Tool execution error: /, '')
                        : (() => {
                            try {
                              const json = JSON.parse(tc.result);
                              return JSON.stringify(json, null, 2);
                            } catch {
                              return tc.result.length > 500
                                ? tc.result.substring(0, 500) + '...'
                                : tc.result;
                            }
                          })()
                      }
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* Streaming content */}
          {isStreaming && streamingContent && (
            <StreamingMessage content={streamingContent} />
          )}

          {/* Loading */}
          {loading && !isStreaming && (
            <div className="group">
              <hr className="tui-divider" />
              <div className="tui-header">
                <Loader2 size={10} className="animate-spin text-purple-500" />
                <span className="text-gray-400">思考中...</span>
              </div>
            </div>
          )}

          {/* Error */}
          {error && (
            <div className="group">
              <hr className="tui-divider" />
              <div className="tui-header">
                <XCircle size={12} className="text-red-500" />
                <span className="text-red-500 font-medium">Error</span>
              </div>
              <div className="text-sm text-red-600 dark:text-red-400 px-1 py-2">
                {error}
                <button
                  onClick={handleRetry}
                  className="ml-2 text-purple-600 dark:text-purple-400 hover:underline text-xs"
                >
                  重试
                </button>
              </div>
            </div>
          )}

          <div ref={messagesEndRef} />
        </div>

        {/* Scroll button */}
        {showScrollButton && (
          <button
            onClick={() => {
              scrollToBottom('smooth');
              setIsNearBottom(true);
              setShowScrollButton(false);
            }}
            className="absolute bottom-4 left-1/2 -translate-x-1/2 p-2 rounded-full bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 shadow-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-all z-10"
          >
            <ArrowDown size={18} className="text-gray-600 dark:text-gray-400" />
          </button>
        )}
      </div>

      {/* Input area */}
      <div className="border-t border-gray-100 dark:border-gray-700 bg-white dark:bg-gray-900 px-6 py-4 flex-shrink-0 transition-colors">
        <div className="max-w-4xl mx-auto">
          <ChatInput onSend={handleChatSend} disabled={isStreaming} />
          <p className="text-xs text-gray-400 dark:text-gray-500 mt-2 text-center">
            AI 可能会产生不准确的信息，请验证重要信息
          </p>
        </div>
      </div>
    </div>
  );
}
