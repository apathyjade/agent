import { useState, useRef, useEffect } from 'react';
import { Send, Loader2, Sparkles, Wrench, CheckCircle, XCircle } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useStore } from '../store';
import type { Message } from '../types';

export function ChatArea() {
  const {
    currentConversation,
    messages,
    loading,
    error,
    isStreaming,
    streamingContent,
    activeToolCalls,
    sendMessageStream,
  } = useStore();

  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, streamingContent, activeToolCalls]);

  useEffect(() => {
    if (inputRef.current && !isStreaming) {
      inputRef.current.focus();
    }
  }, [currentConversation]);

  const handleSend = async () => {
    if (!input.trim() || loading || isStreaming) return;
    const content = input.trim();
    setInput('');
    await sendMessageStream(content);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  if (!currentConversation) return null;

  return (
    <div className="flex-1 flex flex-col bg-white">
      <div className="px-6 py-4 border-b border-gray-100 flex items-center justify-between bg-white/80 backdrop-blur-sm sticky top-0 z-10">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center shadow-sm">
            <Sparkles size={20} className="text-white" />
          </div>
          <div>
            <h2 className="text-base font-semibold text-gray-900">{currentConversation.title}</h2>
            <p className="text-xs text-gray-500">{currentConversation?.model_id || ''}</p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          {currentConversation.system_prompt && (
            <span className="px-2 py-1 bg-purple-50 text-purple-600 text-xs rounded-lg">提示词已加载</span>
          )}
          {isStreaming && (
            <div className="flex items-center gap-2 text-purple-600 text-sm">
              <Loader2 size={14} className="animate-spin" />
              生成中...
            </div>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="max-w-3xl mx-auto px-6 py-8 space-y-6">
          {messages.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}

          {isStreaming && activeToolCalls.length > 0 && (
            <div className="space-y-2">
              {activeToolCalls.map((tc) => (
                <ToolCallIndicator key={tc.id} toolCall={tc} />
              ))}
            </div>
          )}

          {isStreaming && streamingContent && (
            <div className="flex gap-4">
              <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center flex-shrink-0">
                <Sparkles size={16} className="text-white" />
              </div>
              <div className="flex-1">
                <div className="prose prose-gray max-w-none">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{streamingContent}</ReactMarkdown>
                  <span className="inline-block w-2 h-4 ml-1 bg-purple-500 animate-pulse" />
                </div>
              </div>
            </div>
          )}

          {loading && !isStreaming && (
            <div className="flex items-center gap-3 text-gray-400 py-4">
              <Loader2 size={18} className="animate-spin" />
              <span className="text-sm">思考中...</span>
            </div>
          )}

          {error && (
            <div className="p-4 bg-red-50 border border-red-100 rounded-xl">
              <div className="flex items-center gap-2 text-red-600 text-sm">
                <XCircle size={16} />
                <span>{error}</span>
              </div>
              <button
                onClick={() => handleSend()}
                className="mt-2 text-xs text-red-500 hover:text-red-700 font-medium"
              >
                重试
              </button>
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>
      </div>

      <div className="border-t border-gray-100 bg-white p-4">
        <div className="max-w-3xl mx-auto">
          <div className="relative bg-gray-50 rounded-2xl border border-gray-200 focus-within:border-purple-400 focus-within:ring-2 focus-within:ring-purple-100 transition-all">
            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入消息... (Shift+Enter 换行)"
              className="w-full bg-transparent px-4 py-3 pr-14 resize-none focus:outline-none min-h-[56px] max-h-[200px] text-sm"
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
          <p className="text-xs text-gray-400 mt-2 text-center">AI 可能会产生不准确的信息，请验证重要信息</p>
        </div>
      </div>
    </div>
  );
}

function MessageBubble({ message }: { message: Message }) {
  const isUser = message.role === 'user';

  return (
    <div className={`flex gap-4 ${isUser ? 'flex-row-reverse' : ''}`}>
      {!isUser && (
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center flex-shrink-0 shadow-sm">
          <Sparkles size={16} className="text-white" />
        </div>
      )}
      {isUser && (
        <div className="w-8 h-8 rounded-lg bg-gray-200 flex items-center justify-center flex-shrink-0">
          <svg className="w-4 h-4 text-gray-500" fill="currentColor" viewBox="0 0 24 24">
            <path d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z" />
          </svg>
        </div>
      )}
      <div
        className={`max-w-[80%] rounded-2xl px-4 py-3 ${
          isUser
            ? 'bg-gradient-to-r from-purple-600 to-indigo-600 text-white'
            : 'bg-gray-50 text-gray-900'
        }`}
      >
        {isUser ? (
          <p className="whitespace-pre-wrap text-sm">{message.content}</p>
        ) : (
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            className="prose prose-sm max-w-none prose-p:my-2 prose-pre:my-2"
          >
            {message.content}
          </ReactMarkdown>
        )}
        {message.tokens && (
          <p className={`text-xs mt-2 ${isUser ? 'text-purple-200' : 'text-gray-400'}`}>
            {message.tokens} tokens
          </p>
        )}
      </div>
    </div>
  );
}

function ToolCallIndicator({ toolCall }: { toolCall: { id: string; name: string; status: string; result?: string } }) {
  return (
    <div className="flex items-center gap-2 p-3 bg-gray-50 rounded-xl text-sm border border-gray-100">
      <Wrench size={14} className="text-purple-500" />
      <span className="text-gray-700 capitalize">{toolCall.name.replace('_', ' ')}</span>
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
