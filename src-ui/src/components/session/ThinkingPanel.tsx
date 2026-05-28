import { useState, useEffect } from 'react';
import {
  Brain,
  ChevronDown,
  ChevronRight,
  CheckCircle,
  XCircle,
  Loader2,
  Sparkles,
} from 'lucide-react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';

/* ── Types ── */

interface TaskEvent {
  task_id: string;
  label: string;
  worker?: string;
  summary?: string;
  error?: string;
  duration_ms?: number;
}

interface CritiqueEvent {
  task_id: string;
  decision: string;
  issues: string[];
}

interface TaskProgress {
  task_id: string;
  label: string;
  worker: string;
  status: 'running' | 'completed' | 'failed';
  summary?: string;
  error?: string;
  duration_ms?: number;
}

/* ── Component ── */

export function ThinkingPanel() {
  const [phase, setPhase] = useState<string | null>(null);
  const [tasks, setTasks] = useState<Map<string, TaskProgress>>(new Map());
  const [critiques, setCritiques] = useState<CritiqueEvent[]>([]);
  const [expanded, setExpanded] = useState(true);
  const [visible, setVisible] = useState(false);

  const isActive = phase !== null && phase !== 'done' && phase !== 'idle';

  // Show panel when phase changes to a non-idle state
  useEffect(() => {
    if (isActive) {
      setVisible(true);
      setExpanded(true);
    }
  }, [phase]); // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-collapse when done (after delay)
  useEffect(() => {
    if (phase === 'done') {
      const timer = setTimeout(() => setExpanded(false), 2000);
      return () => clearTimeout(timer);
    }
  }, [phase]);

  // Reset when idle
  useEffect(() => {
    if (phase === 'idle' || phase === null) {
      const timer = setTimeout(() => {
        setVisible(false);
        setTasks(new Map());
        setCritiques([]);
      }, 1000);
      return () => clearTimeout(timer);
    }
  }, [phase]);

  // Listen to orchestration events
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    const setup = async () => {
      unlisteners.push(
        await listen<string>('orchestrator_phase', (e) => {
          setPhase(e.payload);
        }),
      );

      unlisteners.push(
        await listen<TaskEvent>('orchestrator_task_start', (e) => {
          const { task_id, label, worker } = e.payload;
          setTasks((prev) => {
            const next = new Map(prev);
            next.set(task_id, {
              task_id,
              label,
              worker: worker ?? 'unknown',
              status: 'running',
            });
            return next;
          });
        }),
      );

      unlisteners.push(
        await listen<TaskEvent>('orchestrator_task_complete', (e) => {
          const { task_id, summary, duration_ms } = e.payload;
          setTasks((prev) => {
            const next = new Map(prev);
            const existing = next.get(task_id);
            if (existing) {
              next.set(task_id, { ...existing, status: 'completed', summary, duration_ms });
            }
            return next;
          });
        }),
      );

      unlisteners.push(
        await listen<TaskEvent>('orchestrator_task_fail', (e) => {
          const { task_id, error } = e.payload;
          setTasks((prev) => {
            const next = new Map(prev);
            const existing = next.get(task_id);
            if (existing) {
              next.set(task_id, { ...existing, status: 'failed', error });
            }
            return next;
          });
        }),
      );

      unlisteners.push(
        await listen<CritiqueEvent>('orchestrator_critique', (e) => {
          setCritiques((prev) => [...prev, e.payload]);
        }),
      );
    };

    setup();

    return () => {
      unlisteners.forEach((u) => u());
    };
  }, []);

  if (!visible) return null;

  const taskList = Array.from(tasks.values());

  return (
    <div className="border border-purple-200 dark:border-purple-800 rounded-lg bg-purple-50 dark:bg-purple-950/30 mb-3 overflow-hidden">
      {/* Header */}
      <button
        className="w-full flex items-center gap-2 px-3 py-2 text-xs font-medium text-purple-700 dark:text-purple-300 hover:bg-purple-100 dark:hover:bg-purple-900/40 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <Brain size={14} />
        <span>Deep Thinking</span>
        {phase && (
          <span className="ml-1 text-purple-500 dark:text-purple-400 italic">
            {phase}
          </span>
        )}
        {isActive && <Loader2 size={12} className="animate-spin ml-auto" />}
        <span className="ml-auto">
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </span>
      </button>

      {/* Body */}
      {expanded && (
        <div className="px-3 pb-2 space-y-2 text-xs">
          {/* Phase indicator */}
          <div className="flex items-center gap-2 text-gray-500 dark:text-gray-400">
            <Sparkles size={12} />
            <span className="capitalize">{phase ?? 'idle'}</span>
          </div>

          {/* Task list */}
          {taskList.length > 0 && (
            <div className="space-y-1">
              {taskList.map((task) => (
                <div
                  key={task.task_id}
                  className="flex items-start gap-2 py-1 px-2 rounded bg-white/50 dark:bg-gray-900/50"
                >
                  {task.status === 'running' && (
                    <Loader2 size={12} className="animate-spin text-blue-500 mt-0.5 shrink-0" />
                  )}
                  {task.status === 'completed' && (
                    <CheckCircle size={12} className="text-green-500 mt-0.5 shrink-0" />
                  )}
                  {task.status === 'failed' && (
                    <XCircle size={12} className="text-red-500 mt-0.5 shrink-0" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="font-medium text-gray-700 dark:text-gray-300 truncate">
                      {task.label}
                    </div>
                    <div className="text-gray-400 dark:text-gray-500">
                      {task.worker}
                      {task.duration_ms != null && ` · ${(task.duration_ms / 1000).toFixed(1)}s`}
                    </div>
                    {task.summary && (
                      <div className="text-gray-500 dark:text-gray-400 mt-0.5 line-clamp-2">
                        {task.summary}
                      </div>
                    )}
                    {task.error && (
                      <div className="text-red-500 mt-0.5 line-clamp-2">{task.error}</div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Critiques */}
          {critiques.length > 0 && (
            <div className="space-y-1">
              <div className="text-gray-400 dark:text-gray-500 font-medium text-[11px] uppercase tracking-wider">
                Reviews
              </div>
              {critiques.map((c, i) => (
                <div
                  key={i}
                  className="flex items-start gap-2 py-1 px-2 rounded bg-amber-50 dark:bg-amber-950/30"
                >
                  {c.decision === 'revise' ? (
                    <XCircle size={12} className="text-amber-500 mt-0.5 shrink-0" />
                  ) : (
                    <CheckCircle size={12} className="text-green-500 mt-0.5 shrink-0" />
                  )}
                  <div className="text-gray-500 dark:text-gray-400">
                    {c.issues.length > 0 ? (
                      <ul className="list-disc list-inside">
                        {c.issues.map((issue, j) => (
                          <li key={j}>{issue}</li>
                        ))}
                      </ul>
                    ) : (
                      <span>Review passed</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Empty state */}
          {taskList.length === 0 && critiques.length === 0 && (
            <div className="text-gray-400 dark:text-gray-500 italic">
              Thinking in progress...
            </div>
          )}
        </div>
      )}
    </div>
  );
}
