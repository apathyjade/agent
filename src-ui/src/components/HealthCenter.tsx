import { useEffect, useState } from 'react';
import { Heart, RefreshCw, ArrowUpCircle, AlertTriangle, CheckCircle2, XCircle, Loader2 } from 'lucide-react';
import { useStore } from '../store';

const RUNTIME_EMOJI: Record<string, string> = {
  node: '\u26A1', python: '\U0001F40D', docker: '\U0001F433', uv: '\U0001F980', go: '\U0001F537',
  rust: '\U0001F980', java: '\u2615', deno: '\U0001F995', bun: '\U0001F95F', ruby: '\U0001F48E',
};

export function HealthCenter() {
  const { runtimes, versionUpdates, checkUpdates, fetchRuntimes, batchInstalling, batchInstallAll } = useStore();
  const [loading, setLoading] = useState(false);

  const refresh = async () => {
    setLoading(true);
    await Promise.all([checkUpdates(), fetchRuntimes()]);
    setLoading(false);
  };

  const handleUpgradeAll = () => {
    const installs = versionUpdates.map(update => ({
      runtimeType: update.runtime_type,
      version: update.latest_version,
    }));
    batchInstallAll(installs);
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
            <button
              onClick={handleUpgradeAll}
              disabled={batchInstalling}
              className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-xs transition-all"
            >
              <ArrowUpCircle size={12} className={batchInstalling ? 'animate-pulse' : ''} />
              全部升级
            </button>
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
            {versionUpdates.map((update) => (
              <div key={`${update.runtime_type}-${update.latest_version}`} className="p-3 bg-yellow-50 dark:bg-yellow-900/10 border border-yellow-200 dark:border-yellow-700/50 rounded-xl">
                <div className="flex items-start gap-3">
                  <div className="w-7 h-7 rounded-lg bg-yellow-100 dark:bg-yellow-900/30 flex items-center justify-center flex-shrink-0">
                    <AlertTriangle size={14} className="text-yellow-600" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-gray-900 dark:text-gray-100">
                      {RUNTIME_EMOJI[update.runtime_type] || ''} {update.runtime_type.toUpperCase()} {update.current_version}
                    </p>
                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                      {update.reason} — 建议升级到 {update.latest_version}
                    </p>
                  </div>
                  <button
                    onClick={() => window.dispatchEvent(new CustomEvent('install-runtime', { detail: { rt: update.runtime_type, version: update.latest_version } }))}
                    className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-xs transition-all flex-shrink-0"
                  >
                    <ArrowUpCircle size={12} />
                    升级
                  </button>
                </div>
              </div>
            ))}
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
