import { useState, useEffect } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useStore } from '../../store';

export function PlanTimeline() {
  const activePlan = useStore((s) => s.activePlan);
  const planProgress = useStore((s) => s.planProgress);
  const sessionMode = useStore((s) => s.sessionMode);
  const executionStatus = useStore((s) => s.executionStatus);
  const currentSession = useStore((s) => s.currentSession);
  const cancelExecution = useStore((s) => s.cancelExecution);

  const [collapsed, setCollapsed] = useState(false);

  // Auto-collapse when plan completes
  useEffect(() => {
    if (executionStatus.type === 'completed' || executionStatus.type === 'failed') {
      setCollapsed(true);
    }
  }, [executionStatus.type]);

  if (sessionMode !== 'autonomous' || !activePlan) return null;

  const totalSteps = activePlan.steps.length;
  const completedSteps = planProgress?.completed_steps ?? 0;
  const isIdle = executionStatus.type === 'idle' || executionStatus.type === 'completed';

  return (
    <div className="mx-3 my-2 rounded-lg border border-gray-200 dark:border-gray-700 overflow-hidden">
      {/* Header — clickable to collapse/expand */}
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="w-full flex items-center gap-2 px-3 py-2 bg-gray-50 dark:bg-gray-800 hover:bg-gray-100 dark:hover:bg-gray-750 transition-colors text-left"
      >
        {collapsed ? (
          <ChevronRight size={14} className="text-gray-400 flex-shrink-0" />
        ) : (
          <ChevronDown size={14} className="text-gray-400 flex-shrink-0" />
        )}
        <span className="text-xs font-medium text-gray-600 dark:text-gray-400">
          执行计划
        </span>
        <span className="text-xs text-gray-400 dark:text-gray-500">
          {completedSteps}/{totalSteps} 步
        </span>

        {/* Progress bar */}
        <div className="flex-1 h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden max-w-[120px] ml-2">
          <div
            className="h-full bg-purple-500 rounded-full transition-all duration-500"
            style={{ width: `${totalSteps > 0 ? (completedSteps / totalSteps) * 100 : 0}%` }}
          />
        </div>

        <div className="ml-auto flex gap-1">
          {executionStatus.type === 'failed' && currentSession && (
            <span
              onClick={(e) => {
                e.stopPropagation();
                cancelExecution(currentSession.id);
              }}
              className="px-1.5 py-0.5 text-[10px] rounded bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 hover:bg-red-200"
            >
              关闭
            </span>
          )}
        </div>
      </button>

      {/* Steps list — collapsible */}
      {!collapsed && (
        <div className="px-3 pb-2 pt-1 bg-white dark:bg-gray-850 transition-all">
          <div className="space-y-1">
            {activePlan.steps.map((step, i) => {
              const isCurrent =
                planProgress?.event_type === 'step_started' &&
                planProgress?.step_index === i &&
                !isIdle;
              const isDone =
                step.status === 'completed' ||
                (planProgress && planProgress.step_index != null && i < planProgress.step_index);
              const isFailed =
                step.status === 'failed' ||
                (planProgress?.step_index === i && planProgress?.event_type === 'step_failed');
              const isSkipped = step.status === 'skipped';

              let icon = '○';
              let color = 'text-gray-400';
              if (isDone) { icon = '●'; color = 'text-green-500'; }
              if (isCurrent) { icon = '◉'; color = 'text-blue-500'; }
              if (isFailed) { icon = '✕'; color = 'text-red-500'; }
              if (isSkipped) { icon = '—'; color = 'text-gray-300'; }

              return (
                <div key={step.id} className="flex items-center gap-2 text-xs">
                  <span className={`${color} w-4 text-center flex-shrink-0`}>{icon}</span>
                  <span
                    className={`flex-1 truncate ${
                      isDone || isCurrent
                        ? 'text-gray-900 dark:text-gray-100'
                        : 'text-gray-500 dark:text-gray-500'
                    }`}
                  >
                    {step.label}
                  </span>
                  {step.duration_ms != null && (
                    <span className="text-gray-400 tabular-nums flex-shrink-0">
                      {(step.duration_ms / 1000).toFixed(1)}s
                    </span>
                  )}
                  {isFailed && step.error && (
                    <span
                      className="text-red-400 truncate max-w-[120px] flex-shrink-0"
                      title={step.error}
                    >
                      {step.error}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
