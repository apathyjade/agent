import { useCallback, useEffect, useState } from 'react';
import { Button, Drawer } from 'antd';
import { Activity, RefreshCw } from 'lucide-react';
import { getRecentRouteTraces, getScopeMetrics, getScopeUsage } from '../../api/intent';
import type { RouteTrace, ScopeMetrics, ToolUsage } from '../../api/intent';
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

/** 比率 → 百分比文本；`0` 保持 `0%`，不做四舍五入到 `-0%`。 */
export function formatRate(value: number, digits = 0): string {
  if (!Number.isFinite(value) || value <= 0) return '0%';
  return `${(value * 100).toFixed(digits)}%`;
}

/** 毫秒 → 取整文本。 */
export function formatMs(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 ms';
  return `${Math.round(value)} ms`;
}

/** 顶部指标行的一条；`testid` 供测试定位。 */
interface MetricItem {
  label: string;
  value: string;
  testid: string;
}

/** 把聚合结果摊平成展示项；样本为 0 时返回空数组。 */
export function metricItems(metrics: ScopeMetrics | null): MetricItem[] {
  if (!metrics || metrics.samples === 0) return [];
  return [
    { label: '采样', value: String(metrics.samples), testid: 'metric-samples' },
    { label: '收窄率', value: formatRate(metrics.applied_rate), testid: 'metric-applied-rate' },
    {
      label: '选中比',
      value: formatRate(metrics.selected_ratio, 1),
      testid: 'metric-selected-ratio',
    },
    {
      label: '上下文节省',
      value: formatRate(metrics.token_saving_rate),
      testid: 'metric-token-saving',
    },
    { label: '缓存', value: formatRate(metrics.cache_rate), testid: 'metric-cache-rate' },
    { label: '跳过', value: formatRate(metrics.skip_rate), testid: 'metric-skip-rate' },
    { label: '平均耗时', value: formatMs(metrics.avg_elapsed_ms), testid: 'metric-avg-elapsed' },
  ];
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
  const [metrics, setMetrics] = useState<ScopeMetrics | null>(null);
  const [usage, setUsage] = useState<ToolUsage[]>([]);
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      const [nextTraces, nextMetrics, nextUsage] = await Promise.all([
        getRecentRouteTraces(),
        getScopeMetrics(),
        getScopeUsage(),
      ]);
      setTraces(nextTraces);
      setMetrics(nextMetrics);
      setUsage(nextUsage);
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

        <div
          data-testid="scope-metrics"
          className="rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 p-3 mb-3 text-xs"
        >
          {metricItems(metrics).length === 0 ? (
            <span className="text-gray-400 dark:text-gray-500">暂无收窄样本。</span>
          ) : (
            <div className="flex flex-wrap gap-x-4 gap-y-1">
              {metricItems(metrics).map((item) => (
                <span key={item.testid} className="text-gray-600 dark:text-gray-300">
                  {item.label}{' '}
                  <span data-testid={item.testid} className="font-medium text-gray-800 dark:text-gray-100">
                    {item.value}
                  </span>
                </span>
              ))}
            </div>
          )}
        </div>

        {usage.length > 0 && (
          <div
            data-testid="scope-usage"
            className="rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 p-3 mb-3 text-xs"
          >
            <div className="mb-1 text-gray-500 dark:text-gray-400">
              收缩建议：纳入 ≥ 3 次且从未使用的工具可以考虑移出
            </div>
            <div className="space-y-1">
              {usage.map((row) => (
                <div
                  key={row.tool}
                  data-testid="usage-row"
                  className="flex flex-wrap items-center gap-x-3 gap-y-1 text-gray-600 dark:text-gray-300"
                >
                  <span className="font-mono break-all">{row.tool}</span>
                  <span>纳入 {row.scoped_times}</span>
                  <span>使用 {row.used_times}</span>
                  <span
                    data-testid={`usage-suggestion-${row.tool}`}
                    className={
                      row.suggested_removal
                        ? 'text-amber-600 dark:text-amber-400'
                        : 'text-gray-400 dark:text-gray-500'
                    }
                  >
                    {row.suggested_removal ? '建议移出' : '保留'}
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}

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
