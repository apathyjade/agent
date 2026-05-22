import { Server, ChevronDown, CheckCircle2, Loader2, Download, Trash2, Star, FolderOpen } from 'lucide-react';
import { LifecycleBadge } from './LifecycleBadge';
import type { RuntimeInfo, RuntimeVersion, VersionLifecycle } from '../types';

interface RuntimeCardProps {
  info: RuntimeInfo;
  versions: RuntimeVersion[];
  isInstalling: boolean;
  expanded: boolean;
  onToggleExpand: () => void;
  onInstall: () => void;
  onSwitch: (version: string) => void;
  onUninstall: (version: string) => void;
  onOpenDir?: (version: string) => void;
}

const RUNTIME_EMOJI: Record<string, string> = {
  node: '⚡', python: '🐍', docker: '🐳', uv: '🦀', go: '🔷',
  rust: '🦀', java: '☕', deno: '🦕', bun: '🥟', ruby: '💎',
};

function getLifecycleSimple(v: RuntimeVersion): VersionLifecycle {
  if (v.lts) return 'lts';
  return 'active';
}

export function RuntimeCard({
  info, versions, isInstalling, expanded,
  onToggleExpand, onInstall, onSwitch, onUninstall, onOpenDir,
}: RuntimeCardProps) {
  return (
    <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
      {/* Header (always visible) */}
      <div
        className="flex items-center gap-4 p-4 cursor-pointer hover:bg-gray-50/50 dark:hover:bg-gray-700/30 transition-colors"
        onClick={onToggleExpand}
      >
        <div className={`w-9 h-9 rounded-lg flex items-center justify-center text-lg ${
          isInstalling
            ? 'bg-purple-100 dark:bg-purple-900/30'
            : info.available
              ? 'bg-green-100 dark:bg-green-900/30'
              : 'bg-gray-100 dark:bg-gray-700'
        }`}>
          {isInstalling ? (
            <Loader2 size={18} className="animate-spin text-purple-500" />
          ) : info.available ? (
            <CheckCircle2 size={18} className="text-green-500" />
          ) : (
            <Server size={18} className="text-gray-400" />
          )}
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
              {RUNTIME_EMOJI[info.runtime_type] || '⚙️'} {info.display_name}
            </span>
            {info.available && info.version && (
              <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                {info.version}
              </span>
            )}
          </div>
          {info.available && info.executable_path && (
            <p className="text-[11px] text-gray-400 dark:text-gray-500 truncate mt-0.5">
              {info.executable_path}
            </p>
          )}
          {!info.available && (
            <p className="text-xs text-red-500 dark:text-red-400 mt-0.5">
              {info.error || '未安装'}
            </p>
          )}
        </div>

        <div className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
          <button
            onClick={onInstall}
            disabled={isInstalling}
            className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-xs transition-all"
          >
            <Download size={12} />
            {isInstalling ? '安装中...' : '安装'}
          </button>
          <ChevronDown size={16} className={`text-gray-400 transition-transform ${expanded ? 'rotate-180' : ''}`} />
        </div>
      </div>

      {/* Expanded content */}
      {expanded && (
        <div className="px-4 pb-4 border-t border-gray-100 dark:border-gray-700 pt-3 space-y-3">
          {/* Installed versions */}
          <div>
            <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2">
              已安装版本
            </h4>
            {info.installed_versions && info.installed_versions.length > 0 ? (
              <div className="space-y-1">
                {info.installed_versions.map(v => (
                  <div key={v.version} className={`flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs ${
                    v.is_active
                      ? 'bg-purple-50 dark:bg-purple-900/20 border border-purple-200 dark:border-purple-700'
                      : 'bg-gray-50 dark:bg-gray-700/50'
                  }`}>
                    <span className="flex-1 font-mono text-gray-700 dark:text-gray-300">{v.version}</span>
                    {v.is_active && (
                      <span className="text-[10px] text-purple-600 dark:text-purple-400 flex items-center gap-0.5">
                        <Star size={10} /> 当前
                      </span>
                    )}
                    {!v.is_active && (
                      <button onClick={() => onSwitch(v.version)} className="text-purple-500 hover:text-purple-600 hover:underline">
                        切换
                      </button>
                    )}
                    {onOpenDir && (
                      <button onClick={() => onOpenDir(v.version)} className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-blue-500 transition-colors" title="打开所在目录">
                        <FolderOpen size={12} />
                      </button>
                    )}
                    <button onClick={() => onUninstall(v.version)} className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-red-500 transition-colors" title="卸载">
                      <Trash2 size={12} />
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-xs text-gray-400 py-1">暂无已安装版本</p>
            )}
          </div>

          {/* Available remote versions */}
          {versions.length > 0 && (
            <div>
              <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2">
                可用版本
              </h4>
              <div className="max-h-40 overflow-y-auto space-y-1">
                {versions.slice(0, 10).map(v => (
                  <div key={v.version} className="flex items-center justify-between px-2.5 py-1.5 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors">
                    <div className="flex items-center gap-2 min-w-0">
                      <span className="text-xs font-mono text-gray-700 dark:text-gray-300">{v.version}</span>
                      <LifecycleBadge lifecycle={getLifecycleSimple(v)} lts={v.lts} />
                    </div>
                    <button
                      onClick={onInstall}
                      className="text-[11px] text-purple-500 hover:text-purple-600 hover:underline flex-shrink-0"
                    >
                      安装
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
