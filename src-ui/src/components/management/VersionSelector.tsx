import { useState, useMemo, useEffect } from 'react';
import { Search, RefreshCw, Download, Loader2 } from 'lucide-react';
import { useStore } from '../../store';
import { LifecycleBadge } from './LifecycleBadge';
import type { RuntimeType, RuntimeVersion, VersionLifecycle } from '../../types';

interface VersionSelectorProps {
  runtimeType: RuntimeType;
  selectedVersion: string;
  onSelect: (version: string) => void;
  onInstall: () => void;
  onClose: () => void;
  isInstalling: boolean;
}

// Helper to determine lifecycle from version data
function getLifecycle(v: RuntimeVersion): VersionLifecycle {
  if (v.lts) return 'lts';
  if (!v.is_stable) return 'active';
  return 'active';
}

export function VersionSelector({
  runtimeType, selectedVersion, onSelect, onInstall, onClose, isInstalling,
}: VersionSelectorProps) {
  const { availableVersions, availableVersionsLoading, fetchAvailableVersions } = useStore();
  const [search, setSearch] = useState('');

  useEffect(() => {
    fetchAvailableVersions(runtimeType);
  }, [runtimeType, fetchAvailableVersions]);

  const filtered = useMemo(() => {
    if (!availableVersions.length) return [];
    const list = availableVersions as RuntimeVersion[];
    return list.filter(v =>
      v.version.includes(search) ||
      v.display_name.toLowerCase().includes(search.toLowerCase())
    );
  }, [availableVersions, search]);

  const recommended = useMemo(() => {
    const list = filtered as RuntimeVersion[];
    return list.filter(v => v.lts || !v.is_stable).slice(0, 3);
  }, [filtered]);

  const allVersions = useMemo(() => {
    return filtered.filter(v => !recommended.includes(v as never));
  }, [filtered, recommended]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-gray-200 dark:border-gray-700 w-[560px] max-h-[650px] flex flex-col">
        {/* Header */}
        <div className="p-6 pb-4 border-b border-gray-100 dark:border-gray-700">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-3">
            选择版本
          </h3>
          {/* Search */}
          <div className="relative">
            <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="搜索版本..."
              className="w-full pl-9 pr-3 py-2 bg-gray-100 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500"
            />
          </div>
        </div>

        {/* Version list */}
        <div className="flex-1 overflow-y-auto p-2">
          {availableVersionsLoading ? (
            <div className="flex items-center justify-center py-8 text-gray-400">
              <Loader2 size={16} className="animate-spin mr-2" />
              <span className="text-sm">加载版本列表中...</span>
            </div>
          ) : filtered.length === 0 ? (
            <div className="py-8 text-center text-gray-400 text-sm">
              {search ? '无匹配版本' : '暂无可用版本'}
            </div>
          ) : (
            <>
              {/* Recommended section */}
              {recommended.length > 0 && (
                <div className="mb-2">
                  <p className="text-[10px] font-medium text-gray-400 uppercase tracking-wider px-3 py-1.5">
                    推荐
                  </p>
                  {recommended.map(v => {
                    const lc = getLifecycle(v as RuntimeVersion);
                    return (
                      <button
                        key={(v as RuntimeVersion).version}
                        onClick={() => onSelect((v as RuntimeVersion).version)}
                        className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-left transition-colors ${
                          selectedVersion === (v as RuntimeVersion).version
                            ? 'bg-purple-50 dark:bg-purple-900/20 ring-1 ring-purple-300 dark:ring-purple-700'
                            : 'hover:bg-gray-50 dark:hover:bg-gray-700/50'
                        }`}
                      >
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                              {(v as RuntimeVersion).display_name}
                            </span>
                            <LifecycleBadge lifecycle={lc} lts={(v as RuntimeVersion).lts} />
                          </div>
                        </div>
                        {(v as RuntimeVersion).file_size && (
                          <span className="text-[11px] text-gray-400 flex-shrink-0">
                            {formatFileSize((v as RuntimeVersion).file_size!)}
                          </span>
                        )}
                      </button>
                    );
                  })}
                </div>
              )}

              {/* All versions */}
              {allVersions.length > 0 && (
                <div>
                  <p className="text-[10px] font-medium text-gray-400 uppercase tracking-wider px-3 py-1.5">
                    全部版本
                  </p>
                  {allVersions.map(v => (
                    <button
                      key={(v as RuntimeVersion).version}
                      onClick={() => onSelect((v as RuntimeVersion).version)}
                      className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-left transition-colors ${
                        selectedVersion === (v as RuntimeVersion).version
                          ? 'bg-purple-50 dark:bg-purple-900/20 ring-1 ring-purple-300 dark:ring-purple-700'
                          : 'hover:bg-gray-50 dark:hover:bg-gray-700/50'
                      }`}
                    >
                      <span className="font-mono text-sm text-gray-700 dark:text-gray-300">
                        {(v as RuntimeVersion).version}
                      </span>
                      <span className="text-xs text-gray-400">
                        {(v as RuntimeVersion).display_name}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </>
          )}
        </div>

        {/* Footer */}
        <div className="p-5 border-t border-gray-100 dark:border-gray-700 flex items-center justify-between">
          <button
            onClick={() => fetchAvailableVersions(runtimeType)}
            className="flex items-center gap-1 text-xs text-gray-500 hover:text-purple-600 transition-colors"
          >
            <RefreshCw size={12} />
            刷新
          </button>
          <div className="flex gap-2">
            <button onClick={onClose} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200">
              取消
            </button>
            <button
              onClick={onInstall}
              disabled={isInstalling || !selectedVersion}
              className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-sm font-medium transition-all"
            >
              {isInstalling ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
              {isInstalling ? '安装中...' : '安装'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}
