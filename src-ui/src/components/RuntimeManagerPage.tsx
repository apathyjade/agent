import { useEffect, useState } from 'react';
import {
  Server, Download, CheckCircle2, XCircle, Loader2,
  RefreshCw, Trash2, Star,
} from 'lucide-react';
import { useStore } from '../store';
import { ManagerPageLayout } from './ManagerPageLayout';
import type { RuntimeType, RuntimeSource, InstalledVersion } from '../types';

// ── Runtime display config ──

const RUNTIME_LABELS: Record<RuntimeType, { icon: string; color: string }> = {
  node:   { icon: '⚡', color: 'text-green-600' },
  python: { icon: '🐍', color: 'text-blue-600' },
  docker: { icon: '🐳', color: 'text-sky-600' },
  uv:     { icon: '🦀', color: 'text-orange-600' },
  go:     { icon: '🔷', color: 'text-cyan-600' },
};

function formatSource(source: RuntimeSource): string {
  switch (source) {
    case 'system': return '系统安装';
    case 'built_in': return '应用内置';
    case 'none': return '未安装';
  }
}

function getSourceBadgeStyle(source: RuntimeSource): string {
  switch (source) {
    case 'system': return 'bg-blue-100 text-blue-600 dark:bg-blue-900/40 dark:text-blue-400';
    case 'built_in': return 'bg-purple-100 text-purple-600 dark:bg-purple-900/40 dark:text-purple-400';
    case 'none': return 'bg-gray-100 text-gray-500 dark:bg-gray-700 dark:text-gray-400';
  }
}

// ── Install Dialog ──

function InstallDialog({
  rt: runtimeType,
  onClose,
}: {
  rt: RuntimeType;
  onClose: () => void;
}) {
  const { installRuntime, installingRuntime, availableVersions, availableVersionsLoading, fetchAvailableVersions } = useStore();
  const [selectedVersion, setSelectedVersion] = useState<string>('latest');

  useEffect(() => {
    fetchAvailableVersions(runtimeType);
  }, [runtimeType, fetchAvailableVersions]);

  const handleInstall = () => {
    installRuntime(runtimeType, selectedVersion === 'latest' ? undefined : selectedVersion);
  };

  const isInstalling = installingRuntime === runtimeType;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-gray-200 dark:border-gray-700 w-[420px] p-5">
        <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-4">
          安装 {RUNTIME_LABELS[runtimeType].icon} {runtimeType.toUpperCase()}
        </h3>

        {/* Version selector */}
        <div className="mb-4">
          <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">
            选择版本
          </label>
          <select
            value={selectedVersion}
            onChange={(e) => setSelectedVersion(e.target.value)}
            disabled={isInstalling}
            className="w-full bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100"
          >
            <option value="latest">最新版本</option>
            {availableVersionsLoading && (
              <option disabled>加载中...</option>
            )}
            {availableVersions.map((v) => (
              <option key={v.version} value={v.version}>
                {v.display_name}
              </option>
            ))}
          </select>
        </div>

        {/* Install button */}
        <div className="flex gap-2">
          <button
            onClick={handleInstall}
            disabled={isInstalling}
            className="flex-1 flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2 rounded-lg text-sm transition-all font-medium"
          >
            {isInstalling ? (
              <><Loader2 size={14} className="animate-spin" /> 安装中...</>
            ) : (
              <><Download size={14} /> 开始安装</>
            )}
          </button>
          <button
            onClick={onClose}
            disabled={isInstalling}
            className="px-4 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
          >
            取消
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Install Progress Modal ──

function InstallProgressModal() {
  const { installProgress, installingRuntime, clearInstallProgress, fetchRuntimes } = useStore();

  if (!installingRuntime) return null;

  const pct = installProgress ? Math.round(installProgress.progress * 100) : 0;
  const message = installProgress?.message || '准备安装...';
  const isDone = installProgress?.progress === 1.0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-gray-200 dark:border-gray-700 w-[400px] p-6">
        <div className="text-center">
          <div className="w-12 h-12 mx-auto mb-3 rounded-xl bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center">
            {isDone ? (
              <CheckCircle2 size={24} className="text-green-600" />
            ) : (
              <Loader2 size={24} className="text-purple-600 animate-spin" />
            )}
          </div>
          <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-1">
            {isDone ? '安装完成' : `安装中...`}
          </h3>
          <p className="text-xs text-gray-500 dark:text-gray-400 mb-4">{message}</p>

          {!isDone && (
            <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2 mb-2">
              <div
                className="bg-gradient-to-r from-purple-500 to-indigo-500 h-2 rounded-full transition-all duration-300"
                style={{ width: `${Math.max(pct, 2)}%` }}
              />
            </div>
          )}

          {isDone ? (
            <button
              onClick={() => { clearInstallProgress(); fetchRuntimes(); }}
              className="mt-2 px-4 py-2 bg-purple-600 hover:bg-purple-700 text-white rounded-lg text-sm"
            >
              完成
            </button>
          ) : (
            <p className="text-[10px] text-gray-400">{pct}%</p>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Version List ──

function VersionList({
  versions,
  onSwitch,
  onUninstall,
}: {
  versions: InstalledVersion[];
  onSwitch: (version: string) => void;
  onUninstall: (version: string) => void;
}) {
  if (versions.length === 0) {
    return <p className="text-xs text-gray-400 py-2 text-center">暂无已安装版本</p>;
  }

  return (
    <div className="space-y-1 mt-2">
      {versions.map((v) => (
        <div
          key={v.version}
          className={`flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs ${
            v.is_active
              ? 'bg-purple-50 dark:bg-purple-900/20 border border-purple-200 dark:border-purple-700'
              : 'bg-gray-100/50 dark:bg-gray-700/50'
          }`}
        >
          <span className="flex-1 font-mono text-gray-700 dark:text-gray-300">
            {v.version}
          </span>
          {v.is_active && (
            <span className="text-[10px] text-purple-600 dark:text-purple-400 flex items-center gap-0.5">
              <Star size={10} /> 当前
            </span>
          )}
          {!v.is_active && (
            <button
              onClick={() => onSwitch(v.version)}
              className="text-purple-500 hover:text-purple-600 hover:underline"
            >
              切换
            </button>
          )}
          <button
            onClick={() => onUninstall(v.version)}
            className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-red-500 transition-colors"
            title="卸载"
          >
            <Trash2 size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}

// ── Main Page ──

export function RuntimeManagerPage() {
  const {
    runtimes, runtimeLoading, runtimeError,
    fetchRuntimes, clearRuntimeError,
    installingRuntime, switchVersion, uninstallVersion,
  } = useStore();

  const [installDialogRt, setInstallDialogRt] = useState<RuntimeType | null>(null);
  const [expandedRt, setExpandedRt] = useState<RuntimeType | null>(null);
  const [activeTab, setActiveTab] = useState<'managed' | 'system'>('managed');

  useEffect(() => {
    fetchRuntimes();
  }, [fetchRuntimes]);

  return (
    <ManagerPageLayout
      icon={<Server size={20} className="text-white" />}
      title="运行时管理"
      subtitle="本地运行环境管理 — 安装、版本切换、目录配置"
      headerActions={
        <button
          onClick={fetchRuntimes}
          disabled={runtimeLoading}
          className="flex items-center gap-1.5 px-4 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-all"
        >
          <RefreshCw size={14} className={runtimeLoading ? 'animate-spin' : ''} />
          刷新
        </button>
      }
    >
      <div className="max-w-3xl mx-auto space-y-4">
        {/* Tabs */}
        <div className="flex gap-1 bg-gray-100 dark:bg-gray-800 rounded-xl p-1">
          <button
            onClick={() => setActiveTab('managed')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-4 py-2 rounded-lg text-sm font-medium transition-all ${
              activeTab === 'managed'
                ? 'bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
            }`}
          >
            <Server size={14} />
            应用管理
          </button>
          <button
            onClick={() => setActiveTab('system')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-4 py-2 rounded-lg text-sm font-medium transition-all ${
              activeTab === 'system'
                ? 'bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm'
                : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
            }`}
          >
            <CheckCircle2 size={14} />
            系统安装
          </button>
        </div>

        {/* Managed tab content */}
        {activeTab === 'managed' && (() => {
          const mgmtRuntimes = runtimes.filter(r => r.source !== 'system');
          if (mgmtRuntimes.length === 0) {
            return (
              <div className="p-8 text-center text-gray-400 dark:text-gray-500">
                <Server size={32} className="mx-auto mb-2 opacity-50" />
                <p className="text-sm">暂无应用管理的运行时</p>
                <p className="text-xs mt-1">点击「安装」按钮添加</p>
              </div>
            );
          }
          return (
            <div className="space-y-2">
              {mgmtRuntimes.map((runtime) => {
                const cfg = RUNTIME_LABELS[runtime.runtime_type];
                const isInstalling = installingRuntime === runtime.runtime_type;
                return (
                  <div key={runtime.runtime_type} className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
                    <div className="flex items-center gap-4">
                      <div className="w-9 h-9 rounded-lg bg-gray-100 dark:bg-gray-600 flex items-center justify-center text-lg">
                        {isInstalling ? (
                          <Loader2 size={18} className="animate-spin text-purple-500" />
                        ) : runtime.available ? (
                          <CheckCircle2 size={18} className="text-green-500" />
                        ) : (
                          <XCircle size={18} className="text-red-400" />
                        )}
                      </div>

                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <h4 className="text-sm font-medium text-gray-900 dark:text-gray-100">
                            {cfg.icon} {runtime.display_name}
                          </h4>
                          {runtime.source !== 'none' && (
                            <span className={`text-[10px] px-1.5 py-0.5 rounded-full ${getSourceBadgeStyle(runtime.source)}`}>
                              {formatSource(runtime.source)}
                            </span>
                          )}
                        </div>

                        {runtime.available ? (
                          <div className="flex items-center gap-3 mt-0.5">
                            <span className="text-[11px] text-gray-500 dark:text-gray-400 font-mono">
                              {runtime.version || '版本未知'}
                            </span>
                            {runtime.executable_path && (
                              <span className="text-[10px] text-gray-400 dark:text-gray-500 truncate max-w-[300px]">
                                {runtime.executable_path}
                              </span>
                            )}
                          </div>
                        ) : (
                          <p className="text-xs text-red-500 dark:text-red-400 mt-0.5">
                            {runtime.error || '未安装'}
                          </p>
                        )}
                      </div>

                      <div className="flex items-center gap-1 flex-shrink-0">
                        {runtime.available && runtime.source === 'built_in' && (
                          <button
                            onClick={() => setExpandedRt(expandedRt === runtime.runtime_type ? null : runtime.runtime_type)}
                            className="text-xs text-purple-500 hover:text-purple-600 px-1"
                          >
                            版本 ({runtime.installed_versions?.length || 0})
                          </button>
                        )}
                        <button
                          onClick={() => setInstallDialogRt(runtime.runtime_type)}
                          disabled={isInstalling}
                          className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-xs transition-all"
                        >
                          <Download size={12} />
                          {isInstalling ? '安装中...' : '安装'}
                        </button>
                      </div>
                    </div>

                    {expandedRt === runtime.runtime_type && runtime.source === 'built_in' && (
                      <VersionList
                        versions={runtime.installed_versions || []}
                        onSwitch={(v) => switchVersion(runtime.runtime_type, v)}
                        onUninstall={(v) => uninstallVersion(runtime.runtime_type, v)}
                      />
                    )}
                  </div>
                );
              })}
            </div>
          );
        })()}

        {/* System tab content (read-only) */}
        {activeTab === 'system' && (() => {
          const sysRuntimes = runtimes.filter(r => r.source === 'system' && r.available);
          if (sysRuntimes.length === 0) {
            return (
              <div className="p-8 text-center text-gray-400 dark:text-gray-500">
                <CheckCircle2 size={32} className="mx-auto mb-2 opacity-50" />
                <p className="text-sm">未检测到系统安装的运行时</p>
              </div>
            );
          }
          return (
            <div className="space-y-2">
              {sysRuntimes.map((runtime) => (
                <div key={runtime.runtime_type} className="p-3 bg-gray-50/50 dark:bg-gray-700/30 rounded-xl border border-gray-100 dark:border-gray-700/50">
                  <div className="flex items-center gap-3">
                    <div className="w-7 h-7 rounded-lg bg-blue-100 dark:bg-blue-900/30 flex items-center justify-center text-sm">
                      <CheckCircle2 size={15} className="text-blue-500" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-gray-800 dark:text-gray-200">
                          {RUNTIME_LABELS[runtime.runtime_type].icon} {runtime.display_name}
                        </span>
                        <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-100 text-blue-600 dark:bg-blue-900/40 dark:text-blue-400">
                          系统安装
                        </span>
                      </div>
                      <div className="flex items-center gap-2 mt-0.5">
                        <span className="text-[11px] text-gray-500 dark:text-gray-400 font-mono">
                          {runtime.version || '版本未知'}
                        </span>
                        {runtime.executable_path && (
                          <span className="text-[10px] text-gray-400 dark:text-gray-500 truncate max-w-[280px]">
                            {runtime.executable_path}
                          </span>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          );
        })()}

        {/* Empty state */}
        {!runtimeLoading && runtimes.length === 0 && (
          <div className="p-8 text-center text-gray-400 dark:text-gray-500">
            <Server size={32} className="mx-auto mb-2 opacity-50" />
            <p className="text-sm">未检测到运行时环境</p>
            <p className="text-xs mt-1">刷新以重新检测</p>
          </div>
        )}

        {/* Loading */}
        {runtimeLoading && (
          <div className="flex items-center justify-center py-4 text-gray-400">
            <Loader2 size={16} className="animate-spin mr-2" />
            <span className="text-sm">处理中...</span>
          </div>
        )}

        {/* Error banner */}
        {runtimeError && (
          <div className="p-3 bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-800 rounded-lg text-sm text-red-600 dark:text-red-400 flex items-center justify-between">
            <span>{runtimeError}</span>
            <button onClick={clearRuntimeError} className="underline text-xs ml-2">关闭</button>
          </div>
        )}
      </div>

      {/* Install dialog */}
      {installDialogRt && (
        <InstallDialog
          rt={installDialogRt}
          onClose={() => setInstallDialogRt(null)}
        />
      )}

      {/* Install progress modal */}
      <InstallProgressModal />
    </ManagerPageLayout>
  );
}
