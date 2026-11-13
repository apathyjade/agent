import { useCallback, useEffect, useState } from 'react';
import { Button, Drawer } from 'antd';
import { Activity, RefreshCw } from 'lucide-react';
import { getRecentRouteTraces } from '../../api/intent';
import type { RouteTrace } from '../../api/intent';
import { scopeStatusLabel } from '../common/scopeLabels';

/**
 * 一条 trace 的收窄摘要；`null` 表示后端未返回收窄信息（旧数据）。
 */
export function routeScopeSummary(trace: RouteTrace): string | null {
  const scope = trace.scope;
  if (!scope) return null;
  if (scope.skipped) return scopeStatusLabel(scope.skipped);
  const cached = scope.cached ? '（缓存）' : '';
  return `收窄 ${scope.selected}/${scope.catalog_size}${cached}`;
}

/**
 * 路由调试抽屉：只读展示进程内最近的路由决策与收窄结果。
 *
 * 自带触发按钮，挂在意图管理页的 `headerActions` 即可，不参与任何执行语义。
 */
export function RouteTracePanel() {
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [traces, setTraces] = useState<RouteTrace[]>([]);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      setTraces(await getRecentRouteTraces());
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  return (
    <>
      <button
        onClick={() => setOpen(true)}
        data-testid="route-trace-open"
        className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-gray-600 dark:text-gray-300 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 rounded-lg transition-colors"
      >
        <Activity size={14} /> 路由调试
      </button>
      <Drawer
        title="路由调试"
        open={open}
        onClose={() => setOpen(false)}
        width={720}
      >
        <div className="flex items-center justify-between mb-3">
          <p className="text-xs text-gray-400 dark:text-gray-500">
            只读快照：最近的路由决策与资源收窄结果，不参与执行。
          </p>
          <Button
            size="small"
            onClick={() => void load()}
            loading={loading}
            icon={<RefreshCw size={12} />}
          >
            刷新
          </Button>
        </div>

        {error && <p className="text-sm text-red-600 dark:text-red-400 mb-3">{error}</p>}

        {!loading && traces.length === 0 && !error && (
          <p className="text-sm text-gray-400 dark:text-gray-500">还没有路由记录。</p>
        )}

        <div className="space-y-2">
          {traces.map((trace, index) => {
            const summary = routeScopeSummary(trace);
            return (
              <div
                key={`${trace.at}-${index}`}
                data-testid="route-trace"
                className="rounded-lg border border-gray-200 dark:border-gray-700 p-3 text-xs space-y-1"
              >
                <div className="flex items-center gap-2 flex-wrap text-gray-400 dark:text-gray-500">
                  <span>{trace.at}</span>
                  <span>{trace.source}</span>
                  <span>{trace.elapsed_ms} ms</span>
                  <span>{trace.llm_calls} 次 LLM</span>
                  {trace.probe_used && <span>探针</span>}
                </div>
                <div className="text-gray-600 dark:text-gray-300 break-all">
                  {trace.message_excerpt}
                </div>
                <div className="text-gray-600 dark:text-gray-300">
                  意图:{' '}
                  {trace.intents.length > 0
                    ? trace.intents.map((intent) => intent.intent_name).join(' · ')
                    : '无（普通聊天）'}
                </div>
                {summary && (
                  <div data-testid="route-trace-scope" className="text-purple-600 dark:text-purple-400">
                    {summary}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </Drawer>
    </>
  );
}
