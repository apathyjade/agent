import { useEffect, useState, useRef, useCallback } from 'react';
import {
  Server, CheckCircle2, Loader2,
  RefreshCw, FolderOpen, Heart, AlertTriangle, X,
  Search, ClipboardList,
} from 'lucide-react';
import { openVersionDirectory } from '../../api/tauri';
import { useStore } from '../../store';
import { ManagerPageLayout } from '../common/ManagerPageLayout';
import { RuntimeCard } from './RuntimeCard';
import { VersionSelector } from './VersionSelector';
import type { RuntimeType } from '../../types';
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
    onClose();
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

// ── Install Progress Bar ──

function InstallProgressBar() {
  const { installProgress, installingRuntime, batchInstalling, clearInstallProgress, fetchRuntimes } = useStore();
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const notifiedRef = useRef(false);
  const [visible, setVisible] = useState(false);

  useEffect(() => { if ('Notification' in window && Notification.permission === 'default') Notification.requestPermission(); }, []);

  useEffect(() => { if (installingRuntime || batchInstalling) setVisible(true); }, [installingRuntime, batchInstalling]);

  const isDone = installProgress?.progress === 1.0;
  useEffect(() => {
    if (isDone && !batchInstalling) {
      if (!notifiedRef.current && 'Notification' in window && Notification.permission === 'granted') {
        notifiedRef.current = true;
        new Notification('运行时安装完成', {
          body: installProgress?.message || '安装完成',
          icon: '/vite.svg',
        });
      }
      timerRef.current = setTimeout(() => { setVisible(false); clearInstallProgress(); fetchRuntimes(); }, 2000);
    }
    return () => { if (timerRef.current) clearTimeout(timerRef.current); };
  }, [isDone, batchInstalling]);

  if (!visible || (!installingRuntime && !batchInstalling && !isDone)) return null;

  const pct = installProgress ? Math.round(installProgress.progress * 100) : 0;
  const message = installProgress?.message || (batchInstalling ? '批量安装中...' : '准备安装...');
  const label = batchInstalling ? '批量安装' : (installingRuntime ?? '安装');

  return (
    <div className="fixed bottom-6 right-6 z-40 slide-in-from-right-4 w-[320px]">
      <div className={`bg-white dark:bg-gray-800 rounded-xl shadow-lg border transition-all duration-300 overflow-hidden ${isDone ? 'border-green-200' : 'border-purple-200'}`}>
        <div className="flex items-center gap-2 px-4 py-2.5 border-b border-gray-100 dark:border-gray-700 bg-gray-50/50 dark:bg-gray-800/50">
          {isDone ? <CheckCircle2 size={16} className="text-green-500" /> : <Loader2 size={16} className="animate-spin text-purple-500" />}
          <span className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate flex-1">{isDone ? `${label} 完成` : `${label} 中`}</span>
          {isDone && <button onClick={() => { setVisible(false); clearInstallProgress(); }} className="text-gray-400 hover:text-gray-600"><X size={14} /></button>}
        </div>
        <div className="px-4 py-3">
          <p className="text-xs text-gray-500 dark:text-gray-400 truncate">{message}</p>
          {!isDone && (
            <div className="mt-2 flex items-center gap-2">
              <div className="flex-1 bg-gray-200 dark:bg-gray-700 rounded-full h-2 overflow-hidden">
                <div className="bg-gradient-to-r from-purple-500 to-indigo-500 h-full rounded-full transition-all duration-300" style={{width:`${Math.max(pct,2)}%`}} />
              </div>
              <span className="text-xs text-gray-400 w-10 text-right">{pct}%</span>
            </div>
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
    pathConflicts, fetchPathConflicts,
    installDir, fetchInstallDir,
    managers, activeManagers, fetchAllManagers, setManager,
  } = useStore();

  const [installDialogRt, setInstallDialogRt] = useState<RuntimeType | null>(null);
  const [selectedRuntime, setSelectedRuntime] = useState<RuntimeType | null>(null);
  const [activeTab, setActiveTab] = useState('versions');
  const [searchQuery, setSearchQuery] = useState('');
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Filtered lists
  const q = searchQuery.toLowerCase();
  const filteredRuntimes = q
    ? runtimes.filter(r =>
        r.display_name.toLowerCase().includes(q) ||
        r.runtime_type.toLowerCase().includes(q) ||
        (r.version && r.version.toLowerCase().includes(q))
      )
    : runtimes;
  const filteredConflicts = q
    ? pathConflicts.filter(c =>
        c.runtime_type.toLowerCase().includes(q) ||
        c.executables.some(exe => exe.path.toLowerCase().includes(q))
      )
    : pathConflicts;

  // Generate diagnostic report
  const generateReport = useCallback(async () => {
    const lines: string[] = [];
    lines.push('# Runtime Diagnostic Report');
    lines.push(`- Generated: ${new Date().toLocaleString()}`);
    lines.push(`- Install Dir: ${installDir || '(unknown)'}`);
    lines.push('');
    lines.push('## Installed Runtimes');
    for (const r of runtimes) {
      const status = r.available ? `v${r.version}` : '未安装';
      lines.push(`- ${r.display_name} (${r.runtime_type}): ${status}`);
      for (const iv of r.installed_versions) {
        lines.push(`  - ${iv.version} @ ${iv.path}${iv.is_active ? ' [active]' : ''}`);
      }
    }
    lines.push('');
    lines.push('## PATH Conflicts');
    for (const c of pathConflicts) {
      if (c.conflict) {
        lines.push(`- ${c.runtime_type}: ${c.executables.length} versions found`);
        for (const exe of c.executables) {
          lines.push(`  - ${exe.path}${exe.version ? ` (${exe.version})` : ''}${exe.is_active ? ' [active]' : ''}`);
        }
      }
    }
    if (pathConflicts.filter(c => c.conflict).length === 0) {
      lines.push('(no conflicts)');
    }
    await navigator.clipboard.writeText(lines.join('\n'));
  }, [runtimes, pathConflicts, installDir]);

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const isMeta = e.metaKey || e.ctrlKey;
      if (isMeta && e.key === 'k') { e.preventDefault(); searchInputRef.current?.focus(); }
      if (isMeta && e.key === 'r') { e.preventDefault(); fetchRuntimes(); fetchPathConflicts(); }
      if (e.key === 'Escape' && searchQuery) { setSearchQuery(''); searchInputRef.current?.blur(); }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [fetchRuntimes, fetchPathConflicts, searchQuery]);

  // Auto-select first runtime when runtimes load
  useEffect(() => {
    if (!selectedRuntime && runtimes.length > 0) {
      setSelectedRuntime(runtimes[0].runtime_type);
    }
  }, [runtimes, selectedRuntime]);

  useEffect(() => {
    fetchRuntimes();
    fetchPathConflicts();
    fetchInstallDir();
    fetchAllManagers();
  }, [fetchRuntimes, fetchPathConflicts, fetchInstallDir, fetchAllManagers]);

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
      searchBar={
        <div className="relative">
          <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
          <input
            ref={searchInputRef}
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="搜索运行时、版本、项目... (⌘K)"
            className="w-full pl-9 pr-3 py-2 bg-gray-100 dark:bg-gray-700 border border-gray-200 rounded-lg text-sm"
          />
        </div>
      }
      headerActions={
        <div className="flex items-center gap-2">
          <button
            onClick={generateReport}
            className="flex items-center gap-1.5 px-3 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-all"
            title="复制诊断报告"
          >
            <ClipboardList size={14} />
          </button>
          <button
            onClick={fetchRuntimes}
            disabled={runtimeLoading}
            className="flex items-center gap-1.5 px-4 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-all"
          >
            <RefreshCw size={14} className={runtimeLoading ? 'animate-spin' : ''} />
            刷新
          </button>
        </div>
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
        if (filteredRuntimes.length === 0) {
          return (
            <div className="p-8 text-center text-gray-400 dark:text-gray-500 h-full flex items-center justify-center">
              <div>
                <Server size={32} className="mx-auto mb-2 opacity-50" />
                <p className="text-sm">{searchQuery ? '没有匹配的运行时' : '暂无运行时'}</p>
                <p className="text-xs mt-1">{searchQuery ? '尝试其他搜索词' : '刷新以重新检测'}</p>
              </div>
            </div>
          );
        }

        const current = selectedRuntime ?? filteredRuntimes[0]?.runtime_type ?? null;
        const selectedInfo = filteredRuntimes.find(r => r.runtime_type === current);

        return (
          <div className="flex gap-0 h-full">
            {/* 左侧运行时 tab 列表 */}
            <div className="w-44 flex-shrink-0 border-r border-gray-200 dark:border-gray-700 pr-2 space-y-0.5 overflow-y-auto">
                {filteredRuntimes.map((runtime) => {
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
                    isInstalling={installingRuntime === selectedInfo.runtime_type}
                    onInstall={() => setInstallDialogRt(selectedInfo.runtime_type)}
                    onSwitch={(v) => switchVersion(selectedInfo.runtime_type, v)}
                    onUninstall={(v) => uninstallVersion(selectedInfo.runtime_type, v)}
                    onOpenDir={(v) => openVersionDirectory(selectedInfo.runtime_type, v)}
                    availableManagers={managers[selectedInfo.runtime_type]}
                    activeManager={activeManagers[selectedInfo.runtime_type]}
                    onSelectManager={(managerId) => setManager(selectedInfo.runtime_type, managerId)}
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
          const sysRuntimes = filteredRuntimes.filter(r => r.source === 'system' && r.available);
          const conflicts = filteredConflicts.filter(c => c.conflict);

          const copyPath = (path: string) => {
            navigator.clipboard.writeText(path);
          };

          const copyAllConflicts = () => {
            const text = conflicts.map(c => {
              const lines = [`${c.runtime_type.toUpperCase()} — ${c.executables.length} versions`];
              for (const exe of c.executables) {
                lines.push(`  ${exe.is_active ? '*' : ' '} ${exe.path}${exe.version ? ` (${exe.version})` : ''}`);
              }
              return lines.join('\n');
            }).join('\n\n');
            navigator.clipboard.writeText(text);
          };

          return (
            <div className="space-y-4">
              {/* PATH conflicts */}
              {conflicts.length > 0 && (
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider flex items-center gap-1">
                      <AlertTriangle size={12} />
                      PATH 冲突检测
                    </h4>
                    <button
                      onClick={copyAllConflicts}
                      className="flex items-center gap-1 text-[10px] px-2 py-1 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-md transition-colors"
                    >
                      <ClipboardList size={11} />
                      复制诊断报告
                    </button>
                  </div>
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
                              {exe.is_active ? (
                                <span className="text-[10px] text-purple-600 dark:text-purple-400 font-medium">当前</span>
                              ) : (
                                <button
                                  onClick={() => copyPath(exe.path)}
                                  className="text-[10px] px-1.5 py-0.5 bg-gray-200 dark:bg-gray-600 hover:bg-gray-300 dark:hover:bg-gray-500 text-gray-600 dark:text-gray-300 rounded transition-colors"
                                >
                                  复制路径
                                </button>
                              )}
                            </div>
                          ))}
                        </div>
                        <p className="text-[10px] text-yellow-600 dark:text-yellow-400 mt-2 flex items-center gap-1">
                          <AlertTriangle size={10} />
                          提示：PATH 中存在多个版本，建议统一版本或将非活跃版本从 PATH 中移除
                        </p>
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

        {/* Skeleton loading */}
        {runtimeLoading && !runtimeError && runtimes.length === 0 && (
          <div className="flex gap-4 p-4 animate-pulse">
            <div className="w-44 space-y-2">
              {[1,2,3,4,5].map(i => (
                <div key={i} className="flex items-center gap-2.5 px-3 py-2.5">
                  <div className="w-5 h-5 rounded bg-gray-200 dark:bg-gray-700" />
                  <div className="flex-1 space-y-1.5">
                    <div className="h-3 bg-gray-200 dark:bg-gray-700 rounded w-16" />
                    <div className="h-2 bg-gray-100 dark:bg-gray-700/50 rounded w-10" />
                  </div>
                </div>
              ))}
            </div>
            <div className="flex-1 space-y-3 pl-4">
              <div className="h-5 bg-gray-200 dark:bg-gray-700 rounded w-48" />
              <div className="h-4 bg-gray-100 dark:bg-gray-700/50 rounded w-64" />
              <div className="h-4 bg-gray-100 dark:bg-gray-700/50 rounded w-56" />
              <div className="h-20 bg-gray-200 dark:bg-gray-700 rounded" />
              <div className="h-4 bg-gray-100 dark:bg-gray-700/50 rounded w-60" />
              <div className="h-4 bg-gray-100 dark:bg-gray-700/50 rounded w-44" />
            </div>
          </div>
        )}

        {/* Error toast at top-right */}
        {runtimeError && (
          <div className="fixed top-4 right-4 z-50 slide-in-from-right-4 max-w-xs">
            <div className="bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-xl shadow-lg p-3">
              <div className="flex items-start gap-2.5">
                <AlertTriangle size={15} className="text-red-500 mt-0.5 flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-xs font-medium text-red-800 dark:text-red-300">操作失败</p>
                  <p className="text-[11px] text-red-600 dark:text-red-400 mt-0.5 break-words">{runtimeError}</p>
                  <div className="mt-2 flex gap-1.5">
                    <button onClick={() => { clearRuntimeError(); fetchRuntimes(); }} className="text-[10px] px-2 py-1 bg-red-100 dark:bg-red-900/40 hover:bg-red-200 dark:hover:bg-red-900/60 text-red-700 dark:text-red-300 rounded-md transition-colors">重试</button>
                    <button onClick={clearRuntimeError} className="text-[10px] px-2 py-1 text-red-400 hover:text-red-600 rounded-md transition-colors">关闭</button>
                  </div>
                </div>
              </div>
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

      {/* Install progress bar */}
      <InstallProgressBar />
    </ManagerPageLayout>
  );
}
