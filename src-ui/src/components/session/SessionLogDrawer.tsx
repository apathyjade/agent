import { useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, History, RefreshCw, X } from 'lucide-react';
import { useStore } from '../../store';
import { getSessionTurnLog } from '../../api/session';
import type { TurnLogEntry, TurnLogRecord, TurnLogStage, TurnLogSummary } from '../../api/session';

const stageLabels: Record<TurnLogStage, string> = {
  question: '提问',
  understand: '理解',
  plan: '计划',
  confirm: '确认',
  route: '路径',
  prepare: '准备',
  execute: '执行',
  review: '评审',
  answer: '回答',
  notice: '提示',
};

const pathLabels: Record<TurnLogSummary['path'], string> = {
  fast: '快速回答',
  deep: '深度执行',
  planned: '计划执行',
  autonomous: '自主执行',
  unknown: '未知',
};

const statusLabels: Record<TurnLogSummary['status'], string> = {
  completed: '完成',
  failed: '失败',
  cancelled: '已取消',
};

const statusClasses: Record<TurnLogSummary['status'], string> = {
  completed: 'bg-green-50 text-green-600 dark:bg-green-900/30 dark:text-green-400',
  failed: 'bg-red-50 text-red-600 dark:bg-red-900/30 dark:text-red-400',
  cancelled: 'bg-amber-50 text-amber-600 dark:bg-amber-900/30 dark:text-amber-400',
};

const levelClasses: Record<TurnLogEntry['level'], string> = {
  info: 'bg-blue-50 text-blue-600 dark:bg-blue-900/30 dark:text-blue-400',
  warn: 'bg-amber-50 text-amber-600 dark:bg-amber-900/30 dark:text-amber-400',
  error: 'bg-red-50 text-red-600 dark:bg-red-900/30 dark:text-red-400',
};

/** 毫秒转可读时长：不足 1s 显示 ms，否则显示 1 位小数秒。 */
export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return '—';
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

/** 历史行时间：本地年月日时分。 */
export function formatStartedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function StageBadge({ stage, level }: { stage: TurnLogStage; level: TurnLogEntry['level'] }) {
  return (
    <span className={`px-1.5 py-0.5 rounded text-[10px] flex-shrink-0 ${levelClasses[level]}`}>
      {stageLabels[stage] ?? stage}
    </span>
  );
}

function EntryTimeline({ entries }: { entries: TurnLogEntry[] }) {
  return (
    <div className="space-y-1.5">
      {entries.map((entry) => (
        <div key={`${entry.seq}-${entry.at}`} className="flex items-start gap-2 text-xs">
          <span className="w-6 text-right text-[10px] text-gray-300 dark:text-gray-600 flex-shrink-0 pt-0.5">
            {entry.seq}
          </span>
          <StageBadge stage={entry.stage} level={entry.level} />
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline gap-2">
              <span className="text-gray-700 dark:text-gray-200">{entry.title}</span>
              {entry.elapsed_ms != null && (
                <span className="text-[10px] text-gray-400 dark:text-gray-500">
                  {formatDuration(entry.elapsed_ms)}
                </span>
              )}
            </div>
            {entry.summary && (
              <div className="text-[11px] text-gray-500 dark:text-gray-400 break-words">{entry.summary}</div>
            )}
            {entry.detail != null && (
              <details className="mt-1">
                <summary className="cursor-pointer text-[10px] text-gray-400 hover:text-gray-600 dark:text-gray-500 dark:hover:text-gray-300 select-none">
                  原始数据
                </summary>
                <pre className="mt-1 p-2 max-h-[240px] overflow-auto rounded bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 text-[10px] font-mono text-gray-600 dark:text-gray-300 whitespace-pre-wrap">
                  {JSON.stringify(entry.detail, null, 2)}
                </pre>
              </details>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

export function SessionLogDrawer() {
  const open = useStore((state) => state.sessionLogOpen);
  const liveTurnLog = useStore((state) => state.liveTurnLog);
  const sessionLogs = useStore((state) => state.sessionLogs);
  const closeSessionLog = useStore((state) => state.closeSessionLog);
  const fetchSessionLogs = useStore((state) => state.fetchSessionLogs);

  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [records, setRecords] = useState<Record<string, TurnLogRecord>>({});
  const [loadingId, setLoadingId] = useState<string | null>(null);
  const liveEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // 新条目到达后滚到实时区底部。
    liveEndRef.current?.scrollIntoView?.({ behavior: 'smooth', block: 'end' });
  }, [liveTurnLog?.entries.length]);

  if (!open) return null;

  const handleToggle = async (summary: TurnLogSummary) => {
    if (expandedId === summary.turn_id) {
      setExpandedId(null);
      return;
    }
    setExpandedId(summary.turn_id);
    if (records[summary.turn_id]) return;
    setLoadingId(summary.turn_id);
    try {
      const record = await getSessionTurnLog(summary.turn_id);
      if (record) {
        setRecords((prev) => ({ ...prev, [record.turn_id]: record }));
      }
    } catch (err) {
      console.error('Failed to load turn log:', err);
    } finally {
      setLoadingId(null);
    }
  };

  const liveEntries = liveTurnLog?.entries ?? [];
  const isEmpty = sessionLogs.length === 0 && liveEntries.length === 0;

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      <div
        className="absolute inset-0 bg-black/30"
        onClick={closeSessionLog}
        aria-hidden="true"
      />
      <aside className="relative w-[480px] max-w-full h-full flex flex-col bg-white dark:bg-gray-900 border-l border-gray-200 dark:border-gray-700 shadow-xl transition-colors">
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-100 dark:border-gray-700 flex-shrink-0">
          <div className="flex items-center gap-2 text-sm font-medium text-gray-700 dark:text-gray-200">
            <History size={15} className="text-purple-500" />
            会话日志
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => void fetchSessionLogs()}
              title="刷新"
              className="p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-800 text-gray-500 dark:text-gray-400"
            >
              <RefreshCw size={14} />
            </button>
            <button
              type="button"
              onClick={closeSessionLog}
              title="关闭"
              className="p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-800 text-gray-500 dark:text-gray-400"
            >
              <X size={14} />
            </button>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto px-4 py-3 space-y-5">
          {liveEntries.length > 0 && (
            <section>
              <h3 className="mb-2 text-[11px] font-semibold text-gray-400 dark:text-gray-500 tracking-wide">
                当前回合
              </h3>
              <EntryTimeline entries={liveEntries} />
              <div ref={liveEndRef} />
            </section>
          )}

          <section>
            <h3 className="mb-2 text-[11px] font-semibold text-gray-400 dark:text-gray-500 tracking-wide">
              历史回合
            </h3>
            {isEmpty ? (
              <div className="py-12 text-center text-xs text-gray-400 dark:text-gray-500">
                还没有可查看的记录，发送一条消息后再来
              </div>
            ) : (
              <div className="space-y-1">
                {sessionLogs.map((summary) => {
                  const expanded = expandedId === summary.turn_id;
                  const record = records[summary.turn_id];
                  return (
                    <div
                      key={summary.turn_id}
                      className="rounded border border-gray-100 dark:border-gray-800"
                    >
                      <button
                        type="button"
                        onClick={() => void handleToggle(summary)}
                        className="w-full flex items-start gap-2 px-2 py-2 text-left hover:bg-gray-50 dark:hover:bg-gray-800/60"
                      >
                        {expanded ? (
                          <ChevronDown size={13} className="mt-0.5 text-gray-400 flex-shrink-0" />
                        ) : (
                          <ChevronRight size={13} className="mt-0.5 text-gray-400 flex-shrink-0" />
                        )}
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="text-[10px] text-gray-400 dark:text-gray-500 flex-shrink-0">
                              {formatStartedAt(summary.started_at)}
                            </span>
                            <span
                              className={`px-1.5 py-0.5 rounded text-[10px] flex-shrink-0 ${statusClasses[summary.status]}`}
                            >
                              {statusLabels[summary.status]}
                            </span>
                            <span className="px-1.5 py-0.5 rounded text-[10px] bg-gray-50 text-gray-500 dark:bg-gray-800 dark:text-gray-400 flex-shrink-0">
                              {pathLabels[summary.path]}
                            </span>
                            <span className="ml-auto text-[10px] text-gray-400 dark:text-gray-500 flex-shrink-0">
                              {formatDuration(summary.total_ms)}
                            </span>
                          </div>
                          <div className="mt-1 text-xs text-gray-700 dark:text-gray-200 truncate">
                            {summary.question}
                          </div>
                        </div>
                      </button>
                      {expanded && (
                        <div className="px-2 pb-2 pl-7">
                          {loadingId === summary.turn_id && (
                            <div className="py-2 text-[11px] text-gray-400">加载中...</div>
                          )}
                          {record && <EntryTimeline entries={record.entries} />}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        </div>
      </aside>
    </div>
  );
}
