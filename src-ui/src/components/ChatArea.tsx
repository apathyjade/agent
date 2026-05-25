import { useState, useRef, useEffect, useCallback, memo } from 'react';
import { Loader2, Sparkles, Wrench, CheckCircle, XCircle, ArrowDown, Code, X } from 'lucide-react';
import { Select } from 'antd';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Row, Col } from '@jelper/component';
import { useStore } from '../store';
import { MessageBubble } from './MessageBubble';
import { CodeBlock } from './CodeBlock';
import { ChatInput } from './ChatInput';
import type { Message } from '../types';

const StreamingMessage = memo(function StreamingMessage({ content }: { content: string }) {
  return (
    <Row $gap={16}>
      <Row.Item $fixed>
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center shadow-sm">
          <Sparkles size={16} className="text-white" />
        </div>
      </Row.Item>
      <Row.Item $scale={1}>
        <div className="rounded-2xl px-4 py-3 bg-white dark:bg-gray-800 border border-gray-100 dark:border-gray-700 shadow-sm">
          <div className="prose prose-sm max-w-none prose-p:my-2 prose-pre:my-0 prose-pre:p-0 prose-pre:border-0 prose-code:bg-gray-100 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded prose-code:text-pink-600 prose-code:font-mono prose-code:text-sm prose-ul:my-1 prose-ol:my-1 prose-li:my-0.5 prose-headings:my-2 prose-h1:text-lg prose-h2:text-base prose-h3:text-sm prose-hr:my-3 prose-blockquote:border-l-purple-400 prose-blockquote:text-gray-500 prose-blockquote:not-italic prose-a:text-purple-600">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
            <span className="inline-block w-2 h-4 ml-1 bg-purple-500 animate-pulse" />
          </div>
        </div>
      </Row.Item>
    </Row>
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
  const currentSession = useStore((s) => s.currentSession);
  const messages = useStore((s) => s.messages);
  const loading = useStore((s) => s.loading);
  const error = useStore((s) => s.error);
  const isStreaming = useStore((s) => s.isStreaming);
  const streamingContent = useStore((s) => s.streamingContent);
  const activeToolCalls = useStore((s) => s.activeToolCalls);
  const sendMessageStream = useStore((s) => s.sendMessageStream);
  const models = useStore((s) => s.models);
  const updateSessionModel = useStore((s) => s.updateSessionModel);
  const setError = useStore((s) => s.setError);
  const lastSessionMessages = useStore((s) => s.lastSessionMessages);
  const setSessionMessages = useStore((s) => s.setSessionMessages);

  const [isNearBottom, setIsNearBottom] = useState(true);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [showDebugPanel, setShowDebugPanel] = useState(false);
  
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
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

  // Listen for debug_messages event from backend
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<Array<Record<string, unknown>>>('debug_messages', (event) => {
        setSessionMessages(event.payload);
      }).then(fn => { unlisten = fn; });
    });
    return () => { unlisten?.(); };
  }, []);

  const handleChatSend = useCallback(async (content: string) => {
    setIsNearBottom(true);
    setShowScrollButton(false);
    const pid = useStore.getState().activePersonaId;
    await sendMessageStream(content, undefined, pid ?? undefined);
  }, [sendMessageStream]);

  const handleRetry = useCallback(() => {
    setError(null);
  }, [setError]);

  if (!currentSession) return null;

  return (
    <Col className="bg-white dark:bg-gray-900 transition-colors">
      <Col.Item $fixed>
        <Row $justify="space-between" $align="center" className="px-6 py-3 border-b border-gray-100 dark:border-gray-700 bg-white/80 dark:bg-gray-900/80 backdrop-blur-sm">
          <Row $align="center" $gap={8}>
            <Select
              value={currentSession.model_id}
              onChange={async (value) => {
                await updateSessionModel(currentSession.id, value);
              }}
              size="small"
              variant="borderless"
              popupMatchSelectWidth={false}
              className="text-xs"
              options={models.filter(m => m.enabled).map(m => ({
                value: m.id,
                label: m.display_name,
                provider: m.provider,
              }))}
              optionRender={(option) => (
                <div className="flex items-center justify-between gap-3">
                  <span>{option.data.label}</span>
                  <span className="text-xs text-gray-400 opacity-60">{option.data.provider}</span>
                </div>
              )}
              notFoundContent="没有可用模型"
            />
          </Row>
          <Row $align="center" $gap={12}>
            {currentSession.system_prompt && (
              <span className="px-2 py-1 bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 text-xs rounded-lg">提示词已加载</span>
            )}
            {lastSessionMessages && (
              <button
                onClick={() => setShowDebugPanel(!showDebugPanel)}
                className={`p-1.5 rounded-lg transition-all ${
                  showDebugPanel
                    ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-400'
                    : 'text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
                }`}
                title="查看本次请求的上下文"
              >
                <Code size={14} />
              </button>
            )}
            {isStreaming && (
              <Row $align="center" $gap={8} className="text-purple-600 text-sm">
                <Loader2 size={14} className="animate-spin" />
                <span>生成中...</span>
              </Row>
            )}
          </Row>
        </Row>
      </Col.Item>

      {showDebugPanel && lastSessionMessages && (
        <Col.Item $fixed>
          <div className="border-b border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50">
            <div className="max-w-3xl mx-auto px-6 py-3">
              <Row $justify="space-between" $align="center" className="mb-2">
                <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
                  请求上下文 ({lastSessionMessages.length} 条消息)
                </span>
                <button
                  onClick={() => setShowDebugPanel(false)}
                  className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
                >
                  <X size={14} />
                </button>
              </Row>
              <CodeBlock language="json" value={JSON.stringify(lastSessionMessages, null, 2)} />
            </div>
          </div>
        </Col.Item>
      )}

      <Col.Item $scale={1}>
        <div 
          ref={messagesContainerRef}
          onScroll={handleScroll}
          className="overflow-y-auto h-full relative"
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
              <Row $align="center" $gap={12} className="text-gray-400 dark:text-gray-500 py-4">
                <Loader2 size={18} className="animate-spin" />
                <span className="text-sm">思考中...</span>
              </Row>
            )}

            {error && (
              <div className="p-4 bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-800 rounded-xl" role="alert">
                <Row $align="flex-start" $gap={8} className="text-red-600 dark:text-red-400 text-sm">
                  <XCircle size={16} className="mt-0.5" />
                  <Row.Item $scale={1}>
                    <span>{error}</span>
                    <div className="mt-2">
                      <button
                        onClick={handleRetry}
                        className="text-xs text-red-500 hover:text-red-700 dark:text-red-400 dark:hover:text-red-300 font-medium"
                      >
                        重试
                      </button>
                    </div>
                  </Row.Item>
                </Row>
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
      </Col.Item>

      <Col.Item $fixed>
        <div className="border-t border-gray-100 dark:border-gray-700 bg-white dark:bg-gray-900 p-4 transition-colors">
          <div className="max-w-3xl mx-auto">
            <ChatInput
              onSend={handleChatSend}
              disabled={isStreaming}
            />
            <p className="text-xs text-gray-400 dark:text-gray-500 mt-2 text-center">AI 可能会产生不准确的信息，请验证重要信息</p>
          </div>
        </div>
      </Col.Item>
    </Col>
  );
}

function ToolCallIndicator({ toolCall }: { toolCall: { id: string; name: string; status: string; result?: string } }) {
  return (
    <Row $align="center" $gap={8} className="p-3 bg-gray-50 dark:bg-gray-800 rounded-xl text-sm border border-gray-100 dark:border-gray-700">
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
    </Row>
  );
}
