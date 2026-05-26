import { Modal, Button } from 'antd';
import { useStore } from '../store';
import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ExecutionPlan, PlanStep } from '../types';

function StepList({ steps }: { steps: PlanStep[] }) {
  return (
    <div className="space-y-1.5 mt-3">
      {steps.map((step, i) => (
        <div key={step.id} className="flex items-center gap-2 text-sm">
          <span className="w-6 h-6 rounded-full bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300 flex items-center justify-center text-xs font-medium flex-shrink-0">
            {i + 1}
          </span>
          <div className="flex-1 min-w-0">
            <div className="text-gray-800 dark:text-gray-200 truncate">{step.label}</div>
            <div className="text-xs text-gray-400 dark:text-gray-500">
              {step.execution.type === 'tool_call' && `工具: ${step.execution.tool}`}
              {step.execution.type === 'agent_task' && 'AI 自主任务'}
              {step.execution.type === 'llm_call' && 'LLM 调用'}
              {step.execution.type === 'condition' && '条件判断'}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

export function PlanConfirmDialog() {
  const [plan, setPlan] = useState<ExecutionPlan | null>(null);
  const [visible, setVisible] = useState(false);

  const cancelExecution = useStore((s) => s.cancelExecution);
  const currentSession = useStore((s) => s.currentSession);

  useEffect(() => {
    const unlisten = listen<ExecutionPlan>('plan_generated', (event) => {
      setPlan(event.payload);
      setVisible(true);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleCancel = async () => {
    if (currentSession) {
      await cancelExecution(currentSession.id);
    }
    setVisible(false);
    setPlan(null);
  };

  // Auto-close when plan completes
  useEffect(() => {
    const unlisten = listen('plan_progress', (event: any) => {
      if (event.payload?.event_type === 'plan_completed' || event.payload?.event_type === 'plan_failed') {
        setVisible(false);
        setPlan(null);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <Modal
      title="📋 执行计划"
      open={visible}
      onCancel={handleCancel}
      width={520}
      closable={false}
      maskClosable={false}
      footer={
        <div className="flex justify-end">
          <Button onClick={handleCancel} size="small">
            取消执行
          </Button>
        </div>
      }
    >
      {plan && (
        <div>
          <p className="text-sm text-green-600 dark:text-green-400 mb-1 font-medium">
            ✅ 计划已自动开始执行
          </p>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            共 {plan.steps.length} 步，可在时间线中查看进度：
          </p>
          <StepList steps={plan.steps} />
        </div>
      )}
    </Modal>
  );
}
