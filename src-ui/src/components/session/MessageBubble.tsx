import { useState, memo, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { User, Sparkles, Wrench, ChevronDown, ChevronRight, Copy, Check } from 'lucide-react';
import { CodeBlock } from '../common/CodeBlock';
import type { Message } from '../../types';

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

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return '';
  }
}

const UserMessage = memo(function UserMessage({ message }: MessageBubbleProps) {
  return (
    <div className="group">
      <hr className="tui-divider" />
      <div className="tui-header">
        <User size={12} className="text-purple-500" />
        <span className="text-purple-600 dark:text-purple-400 font-semibold">You</span>
        <span>{formatTime(message.created_at)}</span>
      </div>
      <div className="tui-content">
        <span className="text-gray-400 dark:text-gray-500 mr-2 font-mono">&gt;</span>
        {message.content}
      </div>
    </div>
  );
});

const AssistantMessage = memo(function AssistantMessage({ message }: MessageBubbleProps) {
  return (
    <div className="group">
      <hr className="tui-divider" />
      <div className="tui-header">
        <Sparkles size={12} className="text-purple-500" />
        <span className="text-purple-600 dark:text-purple-400 font-semibold">Agent</span>
        <span>{formatTime(message.created_at)}</span>
        {message.tokens && (
          <span className="ml-auto text-gray-400">{message.tokens} tokens</span>
        )}
      </div>
      <div className="tui-content">
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={{
            pre({ children }) {
              const child = children as React.ReactElement<{
                className?: string;
                children?: React.ReactNode;
              }>;
              if (child?.type === 'code' && child?.props?.className) {
                const match = /language-(\w+)/.exec(
                  String(child.props.className),
                );
                if (match) {
                  return (
                    <CodeBlock
                      language={match[1]}
                      value={String(child.props.children).replace(/\n$/, '')}
                    />
                  );
                }
              }
              return (
                <pre className="bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg p-4 overflow-x-auto text-sm text-gray-700 dark:text-gray-300">
                  {children}
                </pre>
              );
            },
            code({ className, children, ...props }) {
              return (
                <code
                  className="bg-gray-100 dark:bg-gray-700 text-pink-600 dark:text-pink-300 px-1.5 py-0.5 rounded text-sm font-mono"
                  {...props}
                >
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
  );
});

const SystemMessage = memo(function SystemMessage({ message }: MessageBubbleProps) {
  return (
    <div className="group">
      <hr className="tui-divider" />
      <div className="tui-header">
        <span className="text-amber-500">◆</span>
        <span className="text-amber-600 dark:text-amber-400 font-medium">System</span>
        <span>{formatTime(message.created_at)}</span>
      </div>
      <div className="tui-content text-amber-700 dark:text-amber-300 text-sm">
        {message.content}
      </div>
    </div>
  );
});

const ToolMessage = memo(function ToolMessage({ message }: MessageBubbleProps) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);

  let parsed: { name?: string; args?: string; result?: string } | null = null;
  try {
    parsed = JSON.parse(message.content);
  } catch {
    parsed = null;
  }

  const toolName = parsed?.name || message.tool_call_id || 'tool_call';
  const displayArgs = parsed?.args || '';
  const resultContent = parsed?.result || message.content;

  let prettyResult = resultContent;
  try {
    const json = JSON.parse(prettyResult);
    prettyResult = JSON.stringify(json, null, 2);
  } catch { /* not JSON, use as-is */ }

  let prettyArgs = displayArgs;
  try {
    if (displayArgs) {
      const json = JSON.parse(displayArgs);
      prettyArgs = JSON.stringify(json, null, 2);
    }
  } catch { /* not JSON */ }

  const truncated =
    prettyResult.length > 120
      ? prettyResult.substring(0, 120) + '...'
      : prettyResult;

  const handleCopy = useCallback(async () => {
    await navigator.clipboard.writeText(prettyResult);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [prettyResult]);

  return (
    <div className="group">
      <hr className="tui-divider" />
      <div
        className="tui-header cursor-pointer select-none"
        onClick={() => setExpanded(!expanded)}
      >
        <Wrench size={12} className="text-orange-500" />
        <span className="text-orange-600 dark:text-orange-400 capitalize font-medium">
          {toolName}
        </span>
        <span>{formatTime(message.created_at)}</span>
        <span className="ml-auto flex items-center gap-1 text-gray-400">
          <span className="text-[11px]">{expanded ? 'hide' : 'show'}</span>
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </span>
      </div>
      {!expanded && (
        <div className="text-xs text-gray-400 dark:text-gray-500 px-1 py-1">{truncated}</div>
      )}
      {expanded && (
        <div className="py-1 space-y-2">
          {prettyArgs && (
            <div>
              <div className="text-[11px] text-gray-400 dark:text-gray-500 mb-1 font-mono">args</div>
              <pre className="bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg p-3 overflow-x-auto text-xs font-mono text-gray-700 dark:text-gray-300">
                {prettyArgs}
              </pre>
            </div>
          )}
          <div className="flex items-center justify-end">
            <button
              onClick={(e) => {
                e.stopPropagation();
                handleCopy();
              }}
              className="text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors flex items-center gap-1"
            >
              {copied ? (
                <>
                  <Check size={10} className="text-green-500" /> 已复制
                </>
              ) : (
                <>
                  <Copy size={10} /> 复制
                </>
              )}
            </button>
          </div>
          <pre className="bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg p-3 overflow-x-auto text-xs font-mono text-gray-700 dark:text-gray-300">
            {prettyResult}
          </pre>
        </div>
      )}
    </div>
  );
});
