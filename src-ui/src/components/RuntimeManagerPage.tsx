import { useEffect, useState } from 'react';
import {
  Server, CheckCircle2, Loader2,
  RefreshCw, FolderOpen, Heart, AlertTriangle, X,
} from 'lucide-react';
import { openVersionDirectory } from '../api/tauri';
import { useStore } from '../store';
import { ManagerPageLayout } from './ManagerPageLayout';
import { RuntimeCard } from './RuntimeCard';
import { VersionSelector } from './VersionSelector';
import type { RuntimeType } from '../types';
import { ProjectBindingPanel } from './ProjectBindingPanel';
import { HealthCenter } from './HealthCenter';

// ── Runtime display config ──

const RUNTIME_LABELS: Record<RuntimeType, { icon: string; color: string }> = {
  node:   { icon: '⚡', color: 'text-green-600' },
  python: { icon: '🐍', color: 'text-blue-600' },
  docker: { icon: '🐳', color: 'text-sky-600' },
  uv:     { icon: '🦀', color: 'text-orange-600' },
  go:     { icon: '🔷', color: 'text-cyan-600' },
  rust:   { icon: '🦀', color: 'text-orange-700' },
  java:   { icon: '☕', color: 'text-red-600' },
  deno:   { icon: '🦕', color: 'text-green-700' },
};

// ── Install Dialog ──

function InstallDialog({
  rt: runtimeType,
  onClose,
}: {
  rt: RuntimeType;
  onClose: () => void;
}) {
  const { installRuntime, installingRuntime } = useStore();
  const [selectedVersion, setSelectedVersion] = useState<string>('');

  const handleInstall = () => {
    installRuntime(runtimeType, selectedVersion || undefined);
  };

  const isInstalling = installingRuntime === runtimeType;

  return (
    <VersionSelector
      runtimeType={runtimeType}
      selectedVersion={selectedVersion}
      onSelect={setSelectedVersion}
      onInstall={handleInstall}
      onClose={onClose}
      isInstalling={isInstalling}
    />
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

// ── Main Page ──

export function RuntimeManagerPage() {
  const {
    runtimes, runtimeLoading, runtimeError,
    fetchRuntimes, clearRuntimeError,
    installingRuntime, switchVersion, uninstallVersion,
    fetchAvailableVersions, versionCache,
    pathConflicts, fetchPathConflicts, batchInstalling,
  } = useStore();

  const [installDialogRt, setInstallDialogRt] = useState<RuntimeType | null>(null);
  const [selectedRuntime, setSelectedRuntime] = useState<RuntimeType | null>(null);
  const [activeTab, setActiveTab] = useState('versions');

  // Auto-select first runtime when runtimes load
  useEffect(() => {
    if (!selectedRuntime && runtimes.length > 0) {
      setSelectedRuntime(runtimes[0].runtime_type);
    }
  }, [runtimes, selectedRuntime]);

  useEffect(() => {
    fetchRuntimes();
    fetchPathConflicts();
  }, [fetchRuntimes, fetchPathConflicts]);

  // Auto-dismiss error after 8 seconds
  useEffect(() => {
    if (runtimeError) {
      const timer = setTimeout(() => clearRuntimeError(), 8000);
      return () => clearTimeout(timer);
    }
  }, [runtimeError, clearRuntimeError]);

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
      tabs={[
        { key: 'versions', label: '运行时列表', icon: <Server size={13} /> },
        { key: 'projects', label: '项目绑定', icon: <FolderOpen size={13} /> },
        { key: 'system',   label: '系统检测', icon: <CheckCircle2 size={13} /> },
        { key: 'health',   label: '健康中心', icon: <Heart size={13} /> },
      ]}
      activeTab={activeTab}
      onTabChange={setActiveTab}
    >
      <div className="h-full">
        {/* 运行时列表 tab — 全高左右布局，独立滚动 */}
        {activeTab === 'versions' && (() => {
        if (runtimes.length === 0) {
          return (
            <div className="p-8 text-center text-gray-400 dark:text-gray-500 h-full flex items-center justify-center">
              <div>
                <Server size={32} className="mx-auto mb-2 opacity-50" />
                <p className="text-sm">暂无运行时</p>
                <p className="text-xs mt-1">刷新以重新检测</p>
              </div>
            </div>
          );
        }

        const current = selectedRuntime ?? runtimes[0]?.runtime_type ?? null;
        const selectedInfo = runtimes.find(r => r.runtime_type === current);

        return (
          <div className="flex gap-0 h-full">
            {/* 左侧运行时 tab 列表 */}
            <div className="w-44 flex-shrink-0 border-r border-gray-200 dark:border-gray-700 pr-2 space-y-0.5 overflow-y-auto">
                {runtimes.map((runtime) => {
                  const isSelected = runtime.runtime_type === current;
                  const rtLabel = RUNTIME_LABELS[runtime.runtime_type];
                  return (
                    <button
                      key={runtime.runtime_type}
                      onClick={() => {
                        setSelectedRuntime(runtime.runtime_type);
                        if (!versionCache[runtime.runtime_type]) {
                          fetchAvailableVersions(runtime.runtime_type);
                        }
                      }}
                      className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-left text-xs transition-all ${
                        isSelected
                          ? 'bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-300 font-medium shadow-sm'
                          : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700/50'
                      }`}
                    >
                      <span className="text-base leading-none">{rtLabel?.icon || '⚙️'}</span>
                      <div className="flex-1 min-w-0">
                        <div className="truncate">{runtime.display_name}</div>
                        <div className="text-[10px] text-gray-400 dark:text-gray-500 mt-0.5">
                          {runtime.available && runtime.version
                            ? `v${runtime.version}`
                            : '未安装'}
                        </div>
                      </div>
                      {runtime.available && (
                        <span className="w-1.5 h-1.5 rounded-full bg-green-500 flex-shrink-0" />
                      )}
                    </button>
                  );
                })}
              </div>

              {/* 右侧详情区域 — 独立滚动 */}
              <div className="flex-1 pl-4 min-w-0 overflow-y-auto">
                {selectedInfo ? (
                  <RuntimeCard
                    key={selectedInfo.runtime_type}
                    info={selectedInfo}
                    versions={versionCache[selectedInfo.runtime_type] || []}
                    isInstalling={installingRuntime === selectedInfo.runtime_type}
                    expanded={true}
                    onToggleExpand={() => {}}
                    onInstall={() => setInstallDialogRt(selectedInfo.runtime_type)}
                    onSwitch={(v) => switchVersion(selectedInfo.runtime_type, v)}
                    onUninstall={(v) => uninstallVersion(selectedInfo.runtime_type, v)}
                    onOpenDir={(v) => openVersionDirectory(selectedInfo.runtime_type, v)}
                  />
                ) : (
                  <div className="flex items-center justify-center h-64 text-gray-400 dark:text-gray-500">
                    <p className="text-sm">请从左侧选择一个运行时</p>
                  </div>
                )}
              </div>
            </div>
          );
        })()}

        {/* 非 versions tab 保持 max-width 约束 */}
        {activeTab !== 'versions' && (
          <div className="max-w-3xl mx-auto space-y-4">
            {activeTab === 'projects' && <ProjectBindingPanel />}

            {activeTab === 'system' && (() => {
          const sysRuntimes = runtimes.filter(r => r.source === 'system' && r.available);
          const conflicts = pathConflicts.filter(c => c.conflict);

          return (
            <div className="space-y-4">
              {/* PATH conflicts */}
              {conflicts.length > 0 && (
                <div>
                  <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2 flex items-center gap-1">
                    <AlertTriangle size={12} />
                    PATH 冲突检测
                  </h4>
                  <div className="space-y-2">
                    {conflicts.map((conflict) => (
                      <div key={conflict.runtime_type} className="p-3 bg-yellow-50 dark:bg-yellow-900/10 border border-yellow-200 dark:border-yellow-700/50 rounded-xl">
                        <div className="flex items-center gap-2 mb-2">
                          <AlertTriangle size={14} className="text-yellow-600" />
                          <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                            {RUNTIME_LABELS[conflict.runtime_type]?.icon || '⚙️'} {conflict.runtime_type.toUpperCase()}
                          </span>
                          <span className="text-xs text-yellow-600 dark:text-yellow-400">
                            发现 {conflict.executables.length} 个版本
                          </span>
                        </div>
                        <div className="space-y-1">
                          {conflict.executables.map((exe, idx) => (
                            <div key={idx} className={`flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs ${
                              exe.is_active
                                ? 'bg-purple-50 dark:bg-purple-900/20 border border-purple-200 dark:border-purple-700'
                                : 'bg-gray-50 dark:bg-gray-700/50'
                            }`}>
                              <span className={`w-2 h-2 rounded-full ${exe.is_active ? 'bg-purple-500' : 'bg-gray-400'}`} />
                              <span className="flex-1 font-mono text-gray-700 dark:text-gray-300 truncate">{exe.path}</span>
                              {exe.version && <span className="text-gray-500">{exe.version}</span>}
                              {exe.is_active && <span className="text-[10px] text-purple-600 dark:text-purple-400">当前</span>}
                            </div>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* System runtimes */}
              <div>
                <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2 flex items-center gap-1">
                  <CheckCircle2 size={12} />
                  系统 PATH 检测
                  <span className="text-gray-400 font-normal normal-case ml-1">— 以下为系统 PATH 中检测到的运行时</span>
                </h4>
                {sysRuntimes.length === 0 ? (
                  <div className="p-8 text-center text-gray-400 dark:text-gray-500">
                    <CheckCircle2 size={32} className="mx-auto mb-2 opacity-50" />
                    <p className="text-sm">未检测到系统安装的运行时</p>
                  </div>
                ) : (
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
                                {RUNTIME_LABELS[runtime.runtime_type]?.icon || '⚙️'} {runtime.display_name}
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
                )}
              </div>
            </div>
          );
        })()}

            {activeTab === 'health' && <HealthCenter />}
          </div>
        )}

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

        {/* Batch installing indicator */}
        {batchInstalling && (
          <div className="flex items-center justify-center py-2 text-purple-600 dark:text-purple-400 text-xs gap-1.5">
            <Loader2 size={12} className="animate-spin" />
            正在批量安装...
          </div>
        )}

        {/* Enhanced error banner with retry */}
        {runtimeError && (
          <div className="p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-xl shadow-sm">
            <div className="flex items-start gap-3">
              <AlertTriangle size={16} className="text-red-500 mt-0.5 flex-shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium text-red-800 dark:text-red-300 mb-0.5">
                  操作失败
                </p>
                <p className="text-xs text-red-600 dark:text-red-400 break-words">
                  {runtimeError}
                </p>
              </div>
              <button
                onClick={clearRuntimeError}
                className="p-1 rounded hover:bg-red-100 dark:hover:bg-red-900/30 text-red-400 hover:text-red-600 transition-colors flex-shrink-0"
                title="关闭"
              >
                <X size={14} />
              </button>
            </div>
            <div className="mt-3 flex gap-2">
              <button
                onClick={() => clearRuntimeError()}
                className="flex items-center gap-1 text-xs px-3 py-1.5 bg-red-100 dark:bg-red-900/30 hover:bg-red-200 dark:hover:bg-red-900/50 text-red-700 dark:text-red-300 rounded-lg transition-colors"
              >
                清除并重试
              </button>
            </div>
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
