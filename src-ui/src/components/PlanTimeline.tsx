import { useStore } from '../store';

export function PlanTimeline() {
  const activePlan = useStore((s) => s.activePlan);
  const planProgress = useStore((s) => s.planProgress);
  const sessionMode = useStore((s) => s.sessionMode);

  if (sessionMode !== 'autonomous' || !activePlan) return null;

  const totalSteps = activePlan.steps.length;
  const completedSteps = planProgress?.completed_steps ?? 0;

  return (
    <div className="mx-3 my-2 p-3 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
      <div className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2">
        执行计划 · {completedSteps}/{totalSteps} 步
      </div>
      <div className="space-y-1">
        {activePlan.steps.map((step, i) => {
          const isCurrent =
            planProgress?.event_type === 'step_started' &&
            planProgress?.step_index === i;
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
                  className="text-red-400 truncate max-w-[150px] flex-shrink-0"
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
  );
}
