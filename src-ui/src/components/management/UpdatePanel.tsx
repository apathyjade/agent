import { useState } from 'react';
import { RefreshCw, Download, ChevronDown, ChevronRight, AlertCircle, CheckCircle2, XCircle } from 'lucide-react';
import { useStore } from '../../store';
import type { SkillScope } from '../../types';

interface UpdatePanelProps {
  scope: SkillScope;
  projectPath: string | null;
}

export function UpdatePanel(_props: UpdatePanelProps) {
  const { updates, busy, checkUpdates, updateAll, skillError, clearSkillError } = useStore();
  const [rawOpen, setRawOpen] = useState(false);

  const isBusy = busy === 'update';

  const handleCheck = async () => {
    clearSkillError();
    await checkUpdates();
  };

  const handleUpdateAll = async () => {
    clearSkillError();
    await updateAll();
    await checkUpdates();
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <button
          onClick={handleCheck}
          disabled={isBusy}
          className="flex items-center gap-1.5 px-3 py-2 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 text-gray-700 dark:text-gray-200 rounded-lg text-sm transition-colors disabled:opacity-50"
        >
          <RefreshCw size={14} className={isBusy ? 'animate-spin' : ''} />
          检查更新
        </button>
        <button
          onClick={handleUpdateAll}
          disabled={isBusy}
          className="flex items-center gap-1.5 px-3 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-sm transition-all"
        >
          <Download size={14} />
          全部更新
        </button>
      </div>

      {skillError && (
        <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
          <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
          <span>{skillError}</span>
        </div>
      )}

      {!updates && !isBusy && !skillError && (
        <div className="flex flex-col items-center justify-center h-48 text-gray-400 dark:text-gray-500 gap-3">
          <RefreshCw size={32} className="opacity-40" />
          <p className="text-sm">点击「检查更新」查看技能更新状态</p>
        </div>
      )}

      {updates && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-3">
            {updates.has_updates ? (
              <span className="flex items-center gap-1.5 text-sm font-medium text-amber-700 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/30 px-3 py-1.5 rounded-full border border-amber-200 dark:border-amber-800/50">
                <AlertCircle size={14} />
                有可用更新
              </span>
            ) : (
              <span className="flex items-center gap-1.5 text-sm font-medium text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-900/30 px-3 py-1.5 rounded-full border border-green-200 dark:border-green-800/50">
                <CheckCircle2 size={14} />
                已是最新
              </span>
            )}
            {updates.has_failures && (
              <span className="flex items-center gap-1.5 text-sm font-medium text-red-700 dark:text-red-400 bg-red-50 dark:bg-red-900/30 px-3 py-1.5 rounded-full border border-red-200 dark:border-red-800/50">
                <XCircle size={14} />
                部分检查失败
              </span>
            )}
          </div>

          <div className="rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
            <button
              onClick={() => setRawOpen(!rawOpen)}
              className="w-full flex items-center justify-between px-4 py-2.5 bg-gray-50 dark:bg-gray-800 hover:bg-gray-100 dark:hover:bg-gray-700/50 transition-colors text-xs font-medium text-gray-600 dark:text-gray-300"
            >
              <span>原始输出</span>
              {rawOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </button>
            {rawOpen && (
              <pre className="px-4 py-3 max-h-96 overflow-auto text-xs font-mono text-gray-700 dark:text-gray-300 bg-white dark:bg-gray-800 whitespace-pre-wrap break-all">
                {updates.output || '（无输出）'}
              </pre>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
