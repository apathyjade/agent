import { useEffect, useState } from 'react';
import { Heart, RefreshCw, ArrowUpCircle, AlertTriangle, CheckCircle2, XCircle, Loader2, HardDrive } from 'lucide-react';
import { useStore } from '../store';

const RUNTIME_EMOJI: Record<string, string> = {
  node: '\u26A1', python: '\U0001F40D', docker: '\U0001F433', uv: '\U0001F980', go: '\U0001F537',
  rust: '\U0001F980', java: '\u2615', deno: '\U0001F995', bun: '\U0001F95F', ruby: '\U0001F48E',
};

function getMajorVer(v: string): number {
  const m = v.match(/^v?(\d+)/);
  return m ? parseInt(m[1], 10) : 0;
}

const COLORS: Record<string, string> = {
  node: '#339933', python: '#3776AB', docker: '#2496ED', uv: '#FFD43B',
  go: '#00ADD8', rust: '#DE5826', java: '#ED8B00', deno: '#70FFAF',
  bun: '#F9F3E0', ruby: '#CC342D',
};

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return (bytes / 1024 ** i).toFixed(i > 0 ? 1 : 0) + ' ' + units[i];
}

export function HealthCenter() {
  const { runtimes, versionUpdates, checkUpdates, fetchRuntimes, batchInstalling, batchInstallAll, diskUsage, fetchDiskUsage, diskUsageLoading } = useStore();
  const [loading, setLoading] = useState(false);
  const [selectedUpdates, setSelectedUpdates] = useState<Set<string>>(new Set());
  const selectedCount = selectedUpdates.size;

  const refresh = async () => {
    setLoading(true);
    await Promise.all([checkUpdates(), fetchRuntimes(), fetchDiskUsage()]);
    setLoading(false);
  };

  const toggleSelect = (rt: string) => {
    setSelectedUpdates(prev => {
      const next = new Set(prev);
      if (next.has(rt)) next.delete(rt); else next.add(rt);
      return next;
    });
  };

  const toggleSelectAll = () => {
    if (selectedUpdates.size === versionUpdates.length) setSelectedUpdates(new Set());
    else setSelectedUpdates(new Set(versionUpdates.map(u => u.runtime_type)));
  };

  const handleUpgradeSelected = () => {
    const installs = versionUpdates.filter(u => selectedUpdates.has(u.runtime_type)).map(u => ({ runtimeType: u.runtime_type, version: u.latest_version }));
    if (installs.length > 0) batchInstallAll(installs);
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => { setSelectedUpdates(new Set(versionUpdates.map(u => u.runtime_type))); }, [versionUpdates]);

  // Build runtime status list from runtimes
  const runtimeStatuses = runtimes.map(rt => {
    if (!rt.available) {
      return { ...rt, statusIcon: XCircle, statusColor: 'text-red-500', statusText: '\u672A\u5B89\u88C5' };
    }
    // Check if there's an update for this runtime
    const update = versionUpdates.find(u => u.runtime_type === rt.runtime_type);
    if (update) {
      return { ...rt, statusIcon: AlertTriangle, statusColor: 'text-yellow-500', statusText: '\u6709\u66F4\u65B0' };
    }
    return { ...rt, statusIcon: CheckCircle2, statusColor: 'text-green-500', statusText: '\u6B63\u5E38' };
  });

  const allHealthy = versionUpdates.length === 0 && runtimeStatuses.length > 0 && runtimeStatuses.every(rt => rt.available);

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <p className="text-xs text-gray-500 dark:text-gray-400">
          运行时健康监控、版本更新提醒
        </p>
        <div className="flex items-center gap-2">
          {versionUpdates.length > 0 && (
            <>
              <button
                onClick={toggleSelectAll}
                className="px-3 py-1.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-xs transition-colors"
              >
                {selectedUpdates.size === versionUpdates.length ? '取消全选' : '全选'}
              </button>
              <button
                onClick={handleUpgradeSelected}
                disabled={batchInstalling || selectedCount === 0}
                className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-xs transition-all"
              >
                <ArrowUpCircle size={12} className={batchInstalling ? 'animate-pulse' : ''} />
                升级选中项 ({selectedCount})
              </button>
            </>
          )}
          <button
            onClick={refresh}
            disabled={loading}
            className="flex items-center gap-1 px-3 py-1.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-xs transition-colors"
          >
            <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
            刷新
          </button>
        </div>
      </div>

      {/* Loading state */}
      {loading && (
        <div className="flex items-center justify-center py-4 text-gray-400">
          <Loader2 size={16} className="animate-spin mr-2" />
          <span className="text-sm">刷新中...</span>
        </div>
      )}

      {/* Version updates */}
      {versionUpdates.length > 0 && (
        <div>
          <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2 flex items-center gap-1">
            <ArrowUpCircle size={12} />
            版本更新
          </h4>
          <div className="space-y-2">
            {versionUpdates.map((update) => {
              const currentMajor = getMajorVer(update.current_version);
              const latestMajor = getMajorVer(update.latest_version);
              const isMajorUpgrade = latestMajor > currentMajor + 1;
              const checked = selectedUpdates.has(update.runtime_type);
              return (
                <div key={`${update.runtime_type}-${update.latest_version}`}
                  className={`p-3 border rounded-xl transition-colors cursor-pointer ${checked ? 'bg-purple-50 dark:bg-purple-900/10 border-purple-300 dark:border-purple-600' : 'bg-yellow-50 dark:bg-yellow-900/10 border-yellow-200 dark:border-yellow-700/50'}`}
                  onClick={() => toggleSelect(update.runtime_type)}
                >
                  <div className="flex items-start gap-3">
                    <div className="flex items-center h-7 mt-0.5">
                      <div className={`w-4 h-4 rounded border-2 flex items-center justify-center transition-colors ${checked ? 'bg-purple-600 border-purple-600' : 'border-gray-400'}`}>
                        {checked && <span className="text-white text-[10px] leading-none">\u2713</span>}
                      </div>
                    </div>
                    <div className="w-7 h-7 rounded-lg bg-yellow-100 dark:bg-yellow-900/30 flex items-center justify-center flex-shrink-0">
                      <AlertTriangle size={14} className="text-yellow-600" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                          {RUNTIME_EMOJI[update.runtime_type] || ''} {update.runtime_type.toUpperCase()}
                        </p>
                        {isMajorUpgrade && (
                          <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 border border-orange-300 dark:border-orange-700">
                            major upgrade
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                        <span className="line-through text-gray-400">{update.current_version}</span>
                        {' \u2192 '}
                        <span className="text-green-600 dark:text-green-400 font-medium">{update.latest_version}</span>
                      </p>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                        {update.reason}
                      </p>
                    </div>
                    <button
                      onClick={(e) => { e.stopPropagation(); window.dispatchEvent(new CustomEvent('install-runtime', { detail: { rt: update.runtime_type, version: update.latest_version } })); }}
                      className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-xs transition-all flex-shrink-0"
                    >
                      <ArrowUpCircle size={12} />
                      升级
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Runtime status */}
      <div>
        <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2 flex items-center gap-1">
          <Heart size={12} />
          运行时状态
        </h4>
        <div className="space-y-1">
          {runtimeStatuses.length === 0 && !loading && (
            <div className="p-4 text-center text-gray-400 text-sm">暂无运行时信息</div>
          )}
          {runtimeStatuses.map((rt) => {
            const StatusIcon = rt.statusIcon;
            return (
              <div key={rt.runtime_type} className="flex items-center gap-3 px-3 py-2 rounded-lg bg-gray-50 dark:bg-gray-700/30">
                <StatusIcon size={14} className={rt.statusColor} />
                <span className="text-sm font-medium text-gray-700 dark:text-gray-300 min-w-[80px]">
                  {RUNTIME_EMOJI[rt.runtime_type] || '⚙️'} {rt.display_name}
                </span>
                <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                  {rt.version || '-'}
                </span>
                <span className={`ml-auto text-xs ${rt.statusColor}`}>
                  {rt.statusText}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      {/* Disk usage */}
      <div>
        <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2 flex items-center gap-1">
          <HardDrive size={12} />
          磁盘占用
        </h4>
        {diskUsageLoading ? (
          <div className="flex items-center justify-center py-4 text-gray-400">
            <Loader2 size={16} className="animate-spin mr-2" />
            <span className="text-sm">计算磁盘占用中...</span>
          </div>
        ) : diskUsage.length > 0 ? (
          <div className="space-y-2">
            <div className="h-4 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden flex">
              {(() => {
                const total = diskUsage.reduce((s, d) => s + d.size_bytes, 0);
                return diskUsage.map((item) => {
                  if (total === 0) return null;
                  const pct = (item.size_bytes / total) * 100;
                  if (pct < 0.5) return null;
                  return (
                    <div
                      key={item.runtime_type}
                      className="h-full transition-all"
                      style={{ width: `${pct}%`, backgroundColor: COLORS[item.runtime_type] || '#888' }}
                      title={`${item.display_name}: ${formatBytes(item.size_bytes)}`}
                    />
                  );
                });
              })()}
            </div>
            <div className="space-y-1">
              {diskUsage.map((item) => (
                <div key={item.runtime_type} className="flex items-center gap-2.5 px-3 py-2 rounded-lg bg-gray-50 dark:bg-gray-700/30">
                  <div className="w-2.5 h-2.5 rounded-full flex-shrink-0" style={{ backgroundColor: COLORS[item.runtime_type] || '#888' }} />
                  <span className="text-sm text-gray-700 dark:text-gray-300 min-w-[72px]">{item.display_name}</span>
                  <span className="text-xs font-mono text-gray-500 dark:text-gray-400">{formatBytes(item.size_bytes)}</span>
                  <span className="text-[10px] text-gray-400 dark:text-gray-500">
                    {item.installed_count} 个版本
                  </span>
                  {item.active_version && (
                    <span className="text-[10px] text-purple-500 dark:text-purple-400 ml-auto">v{item.active_version}</span>
                  )}
                </div>
              ))}
            </div>
            <p className="text-[10px] text-gray-400 text-right">
              总计 {formatBytes(diskUsage.reduce((s, d) => s + d.size_bytes, 0))}
            </p>
          </div>
        ) : (
          <div className="p-4 text-center text-gray-400 text-sm">暂无磁盘占用数据</div>
        )}
      </div>

      {/* Empty state when everything is healthy */}
      {allHealthy && (
        <div className="p-6 text-center">
          <CheckCircle2 size={32} className="mx-auto mb-2 text-green-500" />
          <p className="text-sm text-gray-500 dark:text-gray-400">所有运行时运行正常</p>
        </div>
      )}
    </div>
  );
}
