import { useState, useRef, useEffect } from 'react';
import { useStore } from '../../store';
import { Terminal, ChevronDown, ChevronUp, Trash2 } from 'lucide-react';

export function ExecutionLogPanel() {
  const executionLogs = useStore((s) => s.executionLogs);
  const clearExecutionLogs = useStore((s) => s.clearExecutionLogs);
  const [collapsed, setCollapsed] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom on new entries
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [executionLogs.length]);

  if (executionLogs.length === 0) return null;

  const getLevelColor = (level: string) => {
    switch (level) {
      case 'error': return 'text-red-500 dark:text-red-400 bg-red-50 dark:bg-red-900/10';
      case 'warn': return 'text-yellow-600 dark:text-yellow-400 bg-yellow-50 dark:bg-yellow-900/10';
      default: return 'text-gray-600 dark:text-gray-400';
    }
  };

  const getStepBadge = (step: string) => {
    switch (step) {
      case 'intent': return 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300';
      case 'planner': return 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300';
      case 'execution': return 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300';
      case 'runtime': return 'bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300';
      default: return 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400';
    }
  };

  return (
    <div className="fixed bottom-20 right-4 w-[420px] max-h-[400px] bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 shadow-xl z-50 flex flex-col">
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-850 rounded-t-lg flex-shrink-0">
        <Terminal size={14} className="text-gray-500" />
        <span className="text-xs font-medium text-gray-700 dark:text-gray-300">执行日志</span>
        <span className="text-[10px] text-gray-400 ml-1">{executionLogs.length} 条</span>
        <div className="flex-1" />
        <button
          onClick={clearExecutionLogs}
          className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 hover:text-red-500"
          title="清空日志"
        >
          <Trash2 size={12} />
        </button>
        <button
          onClick={() => setCollapsed(!collapsed)}
          className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400"
        >
          {collapsed ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
        </button>
      </div>

      {/* Log entries */}
      {!collapsed && (
        <div
          ref={scrollRef}
          className="flex-1 overflow-y-auto p-2 space-y-1 font-mono text-[11px] leading-relaxed"
          style={{ maxHeight: '320px' }}
        >
          {executionLogs.map((entry, i) => (
            <div key={i} className={`p-1.5 rounded ${getLevelColor(entry.level)}`}>
              <div className="flex items-start gap-1.5">
                <span className="text-[10px] text-gray-400 dark:text-gray-500 whitespace-nowrap mt-0.5">
                  {entry.timestamp.slice(11, 19)}
                </span>
                <span className={`text-[10px] px-1 rounded font-medium whitespace-nowrap ${getStepBadge(entry.step)}`}>
                  {entry.step}
                </span>
                <span className="flex-1 break-words min-w-0">
                  {entry.message}
                </span>
              </div>
              {entry.detail && (
                <div className="mt-1 ml-[52px] text-[10px] text-gray-500 dark:text-gray-400 bg-white/50 dark:bg-black/20 p-1 rounded">
                  {entry.detail}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
