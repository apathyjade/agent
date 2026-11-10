import { useState } from 'react';
import { Modal, Button, Input } from 'antd';
import { AlertTriangle, FileEdit, Terminal, Trash2 } from 'lucide-react';
import type { PlanConfirmIntent } from '../../api/plan';

/**
 * 从工具白名单推出副作用标注。只做展示，不改变任何执行语义。
 */
export function planIntentSideEffects(tools: string[]): string[] {
  const effects: string[] = [];
  if (tools.includes('shell')) effects.push('将执行命令');
  if (tools.includes('file_system')) effects.push('将写入文件');
  if (tools.includes('code_executor')) effects.push('将执行代码');
  return effects;
}

export interface PlanConfirmDialogProps {
  open: boolean;
  planId: string | null;
  intents: PlanConfirmIntent[];
  loading?: boolean;
  onConfirm: () => void | Promise<void>;
  onCancel: () => void | Promise<void>;
  onReplan: () => void | Promise<void>;
  onRemoveIntent: (stepId: string) => void | Promise<void>;
  onUpdateInstruction: (stepId: string, instruction: string) => void | Promise<void>;
}

/** 拆解结果的确认卡：确认前不执行任何步骤。 */
export function PlanConfirmDialog({
  open,
  planId,
  intents,
  loading = false,
  onConfirm,
  onCancel,
  onReplan,
  onRemoveIntent,
  onUpdateInstruction,
}: PlanConfirmDialogProps) {
  const [editingStepId, setEditingStepId] = useState<string | null>(null);
  const [draft, setDraft] = useState('');

  const startEditing = (intent: PlanConfirmIntent) => {
    setEditingStepId(intent.step_id);
    setDraft(intent.instruction);
  };

  const saveEditing = async () => {
    if (!editingStepId) return;
    const instruction = draft.trim();
    if (!instruction) return;
    await onUpdateInstruction(editingStepId, instruction);
    setEditingStepId(null);
  };

  return (
    <Modal
      title="📋 确认执行计划"
      open={open}
      onCancel={() => void onCancel()}
      width={640}
      closable={false}
      mask={{ closable: false }}
      footer={
        <div className="flex items-center justify-between gap-2">
          <Button onClick={() => void onReplan()} disabled={loading} data-testid="plan-replan">
            重新规划
          </Button>
          <div className="flex gap-2">
            <Button onClick={() => void onCancel()} disabled={loading} autoInsertSpace={false}>
              取消
            </Button>
            <Button
              type="primary"
              onClick={() => void onConfirm()}
              loading={loading}
              disabled={loading || intents.length === 0}
            >
              确认执行
            </Button>
          </div>
        </div>
      }
    >
      <div className="space-y-3">
        <p className="text-sm text-gray-500 dark:text-gray-400">
          识别到 {intents.length} 个意图。确认前不会执行任何步骤；取消则这条消息不发送。
        </p>
        {!planId && (
          <p className="text-xs text-amber-600 dark:text-amber-400">
            计划详情不完整，只能整体确认或取消。
          </p>
        )}

        {intents.map((intent, index) => {
          const effects = planIntentSideEffects(intent.allowed_tools);
          const editable = Boolean(intent.step_id);
          return (
            <div
              key={intent.step_id || `${intent.intent_id}-${index}`}
              data-testid="plan-intent"
              className="rounded-lg border border-gray-200 dark:border-gray-700 p-3"
            >
              <div className="flex items-start gap-2">
                <span className="w-6 h-6 rounded-full bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300 flex items-center justify-center text-xs font-medium flex-shrink-0">
                  {index + 1}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="text-sm font-medium text-gray-800 dark:text-gray-100">
                      {intent.intent_name}
                    </span>
                    {intent.uncertain && (
                      <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 text-[10px] rounded bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 border border-amber-100 dark:border-amber-800">
                        <AlertTriangle size={10} />
                        置信度低
                      </span>
                    )}
                    <span className="text-[10px] text-gray-400">
                      {(intent.confidence * 100).toFixed(0)}%
                    </span>
                  </div>

                  {editingStepId === intent.step_id ? (
                    <div className="mt-1.5">
                      <Input.TextArea
                        aria-label="意图指令"
                        value={draft}
                        onChange={(event) => setDraft(event.target.value)}
                        rows={3}
                      />
                      <div className="flex justify-end gap-2 mt-1.5">
                        <Button size="small" onClick={() => setEditingStepId(null)}>
                          取消修改
                        </Button>
                        <Button size="small" type="primary" onClick={() => void saveEditing()}>
                          保存指令
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <p className="text-xs text-gray-600 dark:text-gray-300 whitespace-pre-wrap mt-1">
                      {intent.instruction}
                    </p>
                  )}

                  <div className="flex items-center gap-3 flex-wrap mt-1.5 text-[11px] text-gray-400 dark:text-gray-500">
                    <span className="inline-flex items-center gap-1">
                      <Terminal size={11} />
                      工具: {intent.allowed_tools.length > 0 ? intent.allowed_tools.join(' · ') : '无'}
                    </span>
                    <span>模型: {intent.model_id ?? '跟随会话'}</span>
                  </div>

                  {effects.length > 0 && (
                    <div
                      data-testid="plan-intent-side-effect"
                      className="mt-1.5 text-[11px] text-red-600 dark:text-red-400"
                    >
                      {effects.join(' / ')}
                    </div>
                  )}

                  {intent.probe_evidence && (
                    <div className="mt-1.5 text-[11px] text-gray-400 dark:text-gray-500">
                      证据: {intent.probe_evidence}
                    </div>
                  )}
                </div>

                <div className="flex flex-col gap-1 flex-shrink-0">
                  <button
                    onClick={() => startEditing(intent)}
                    disabled={!editable || loading}
                    className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] rounded bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    <FileEdit size={10} />
                    改写指令
                  </button>
                  <button
                    onClick={() => void onRemoveIntent(intent.step_id)}
                    disabled={!editable || loading || intents.length <= 1}
                    className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] rounded bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/50 disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    <Trash2 size={10} />
                    删除该意图
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </Modal>
  );
}
