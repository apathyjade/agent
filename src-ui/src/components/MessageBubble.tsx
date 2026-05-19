import { useState, memo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Sparkles, Wrench, ChevronDown, ChevronRight, User, Info } from 'lucide-react';
import { CodeBlock } from './CodeBlock';
import type { Message } from '../types';

interface MessageBubbleProps {
  message: Message;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  switch (message.role) {
    case 'user':
      return <UserMessage message={message} />;
    case 'assistant':
      return <AssistantMessage message={message} />;
    case 'system':
      return <SystemMessage message={message} />;
    case 'tool':
      return <ToolMessage message={message} />;
    default:
      return <AssistantMessage message={message} />;
  }
}

const UserMessage = memo(function UserMessage({ message }: MessageBubbleProps) {
  return (
    <div className="flex gap-4 flex-row-reverse group animate-in fade-in slide-in-from-right-4 duration-300">
      <div className="w-8 h-8 rounded-lg bg-gray-200 flex items-center justify-center flex-shrink-0 shadow-sm group-hover:shadow-md transition-shadow">
        <User size={16} className="text-gray-500" />
      </div>
      <div className="max-w-[80%] rounded-2xl px-4 py-3 bg-gradient-to-r from-purple-600 to-indigo-600 text-white shadow-sm hover:shadow-md transition-shadow">
        <p className="whitespace-pre-wrap text-sm leading-relaxed">{message.content}</p>
        {message.tokens && (
          <p className="text-xs mt-2 text-purple-200/80">{message.tokens} tokens</p>
        )}
      </div>
    </div>
  );
});

const AssistantMessage = memo(function AssistantMessage({ message }: MessageBubbleProps) {
  return (
    <div className="flex gap-4 group animate-in fade-in slide-in-from-left-4 duration-300">
      <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center flex-shrink-0 shadow-sm group-hover:shadow-md transition-shadow">
        <Sparkles size={16} className="text-white" />
      </div>
      <div className="flex-1 min-w-0">
        <div className="rounded-2xl px-4 py-3 bg-white border border-gray-100 shadow-sm hover:shadow-md transition-shadow">
          <div className="prose prose-sm max-w-none prose-p:my-2 prose-pre:my-0 prose-pre:p-0 prose-pre:border-0 prose-code:bg-gray-100 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded prose-code:text-pink-600 prose-code:font-mono prose-code:text-sm prose-ul:my-1 prose-ol:my-1 prose-li:my-0.5 prose-headings:my-2 prose-h1:text-lg prose-h2:text-base prose-h3:text-sm prose-hr:my-3 prose-blockquote:border-l-purple-400 prose-blockquote:text-gray-500 prose-blockquote:not-italic prose-a:text-purple-600 prose-a:no-underline hover:prose-a:underline prose-table:my-2 prose-th:bg-gray-50 prose-td:border prose-td:border-gray-200 prose-img:rounded-lg">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                pre({ children }) {
                  const child = children as React.ReactElement;
                  if (child?.type === 'code' && child?.props?.className) {
                    const match = /language-(\w+)/.exec(String(child.props.className));
                    if (match) {
                      return (
                        <CodeBlock
                          language={match[1]}
                          value={String(child.props.children).replace(/\n$/, '')}
                        />
                      );
                    }
                  }
                  return <pre className="bg-gray-100 rounded-lg p-4 overflow-x-auto text-sm">{children}</pre>;
                },
                code({ className, children, ...props }) {
                  return (
                    <code className="bg-gray-100 px-1.5 py-0.5 rounded text-sm font-mono text-pink-600" {...props}>
                      {children}
                    </code>
                  );
                },
              }}
            >
              {message.content}
            </ReactMarkdown>
          </div>
        </div>
        {message.tokens && (
          <p className="text-xs text-gray-400 mt-1.5 ml-1">{message.tokens} tokens</p>
        )}
      </div>
    </div>
  );
});

const SystemMessage = memo(function SystemMessage({ message }: MessageBubbleProps) {
  return (
    <div className="flex justify-center animate-in fade-in zoom-in-95 duration-300">
      <div className="flex items-start gap-2 max-w-[70%] px-4 py-2.5 bg-amber-50 border border-amber-100 rounded-xl shadow-sm">
        <Info size={14} className="text-amber-500 flex-shrink-0 mt-0.5" />
        <div className="text-xs text-amber-800 leading-relaxed whitespace-pre-wrap">
          {message.content}
        </div>
      </div>
    </div>
  );
});

const ToolMessage = memo(function ToolMessage({ message }: MessageBubbleProps) {
  const [expanded, setExpanded] = useState(false);
  let parsed: { name?: string; args?: string; result?: string } | null = null;
  try {
    parsed = JSON.parse(message.content);
  } catch {
    parsed = null;
  }

  return (
    <div className="flex gap-4 pl-10 animate-in fade-in duration-300">
      <div className="flex-1 min-w-0">
        <div
          className="rounded-xl border border-orange-200 bg-orange-50/50 overflow-hidden cursor-pointer hover:bg-orange-50 transition-colors"
          onClick={() => setExpanded(!expanded)}
        >
          <div className="flex items-center gap-2 px-3 py-2 text-sm text-orange-700">
            <Wrench size={14} className="text-orange-500" />
            <span className="font-medium">
              {parsed?.name || '工具调用'}
            </span>
            <span className="flex-1" />
            {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </div>
          {expanded && (
            <div className="border-t border-orange-200 px-3 py-2 space-y-2">
              {parsed?.args && (
                <div>
                  <span className="text-xs font-medium text-gray-500">参数</span>
                  <pre className="mt-1 text-xs text-gray-700 bg-white rounded p-2 overflow-x-auto whitespace-pre-wrap">
                    {parsed.args}
                  </pre>
                </div>
              )}
              <div>
                <span className="text-xs font-medium text-gray-500">结果</span>
                <pre className="mt-1 text-xs text-gray-700 bg-white rounded p-2 overflow-x-auto whitespace-pre-wrap max-h-48 overflow-y-auto">
                  {parsed?.result || message.content}
                </pre>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
});
