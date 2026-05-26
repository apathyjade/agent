import { useStore } from '../store';
import type { ExecStatus } from '../types';

function getStatusInfo(status: ExecStatus) {
  switch (status.type) {
    case 'running':
      return { label: '执行中...', dot: 'bg-green-500 animate-pulse', bg: 'bg-purple-50 dark:bg-purple-900/20 border-purple-200 dark:border-purple-800' };
    case 'paused':
      return { label: '已暂停', dot: 'bg-yellow-500', bg: 'bg-yellow-50 dark:bg-yellow-900/20 border-yellow-200 dark:border-yellow-800' };
    case 'completed':
      return { label: '执行完成', dot: 'bg-blue-500', bg: 'bg-blue-50 dark:bg-blue-900/20 border-blue-200 dark:border-blue-800' };
    case 'failed':
      return { label: '执行失败', dot: 'bg-red-500', bg: 'bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800' };
    case 'cancelled':
      return { label: '已取消', dot: 'bg-gray-500', bg: 'bg-gray-50 dark:bg-gray-800 border-gray-200 dark:border-gray-700' };
    default:
      return { label: '', dot: '', bg: '' };
  }
}

export function ExecutionStatusBar() {
  const sessionMode = useStore((s) => s.sessionMode);
  const executionStatus = useStore((s) => s.executionStatus);
  const activePlan = useStore((s) => s.activePlan);
  const currentSession = useStore((s) => s.currentSession);
  const pauseExecution = useStore((s) => s.pauseExecution);
  const resumeExecution = useStore((s) => s.resumeExecution);
  const cancelExecution = useStore((s) => s.cancelExecution);

  if (sessionMode !== 'autonomous' || !activePlan) return null;

  const info = getStatusInfo(executionStatus);
  if (!info.label) return null;

  const isRunning = executionStatus.type === 'running';
  const isPaused = executionStatus.type === 'paused';
  const sessionId = currentSession?.id;

  return (
    <div className={`flex items-center gap-2 px-3 py-1.5 border-b text-sm ${info.bg}`}>
      <span className="flex items-center gap-1">
        <span className={`w-2 h-2 rounded-full ${info.dot}`} />
        <span className="font-medium text-purple-700 dark:text-purple-300">
          {info.label}
        </span>
      </span>

      <span className="text-gray-500 dark:text-gray-400 text-xs ml-2">
        {activePlan.steps.length} 步计划
      </span>

      <div className="ml-auto flex gap-1">
        {isRunning && sessionId && (
          <>
            <button
              onClick={() => pauseExecution(sessionId)}
              className="px-2 py-0.5 text-xs rounded bg-yellow-100 dark:bg-yellow-800 hover:bg-yellow-200 dark:hover:bg-yellow-700 text-yellow-800 dark:text-yellow-200"
            >
              暂停
            </button>
            <button
              onClick={() => cancelExecution(sessionId)}
              className="px-2 py-0.5 text-xs rounded bg-red-100 dark:bg-red-800 hover:bg-red-200 dark:hover:bg-red-700 text-red-800 dark:text-red-200"
            >
              取消
            </button>
          </>
        )}
        {isPaused && sessionId && (
          <>
            <button
              onClick={() => resumeExecution(sessionId)}
              className="px-2 py-0.5 text-xs rounded bg-green-100 dark:bg-green-800 hover:bg-green-200 dark:hover:bg-green-700 text-green-800 dark:text-green-200"
            >
              继续
            </button>
            <button
              onClick={() => cancelExecution(sessionId)}
              className="px-2 py-0.5 text-xs rounded bg-red-100 dark:bg-red-800 hover:bg-red-200 dark:hover:bg-red-700 text-red-800 dark:text-red-200"
            >
              取消
            </button>
          </>
        )}
      </div>
    </div>
  );
}
