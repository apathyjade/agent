import { invoke } from '@tauri-apps/api/core';
import type { ExecutionPlan } from '../types';

/** `decompose_message` 返回的单个意图；`allowed_tools` 已是与会话白名单的交集。 */
export interface DecomposedIntentPayload {
  intent_id: string;
  intent_name: string;
  instruction: string;
  confidence: number;
  uncertain: boolean;
  allowed_tools: string[];
  model_id: string | null;
  max_iterations: number | null;
  path: string;
  probe_evidence: string | null;
}

/** 本回合资源作用域快照（只读）；`status` 说明第二级收窄的结局。 */
export interface ScopePayload {
  /** `not_requested` / `skipped_no_catalog` / `skipped_small_catalog` / `skipped_no_budget` / `skipped_unavailable` / `applied`。 */
  status: string;
  /** 第二级收窄是否命中缓存（仅 `applied` 时有意义）。 */
  cached: boolean;
  /** 最终允许的工具 id。 */
  tools: string[];
  /** 最终注入的提示词片段 id。 */
  prompts: string[];
  /** 记忆条数上限；`null` 表示沿用默认。 */
  memory_limit: number | null;
  /** 是否因预算被截断。 */
  truncated: boolean;
}

/** `decompose_message` 的返回值；`plan_id` 为空表示零意图，走普通聊天。 */
export interface DecomposeMessageResult {
  /**
   * 合法零意图也会返回 0 步缓存计划的 id；前端据此同步 `active_plan_id`，
   * 后续消息直接复用，不再重复拆解。
   */
  plan_id: string | null;
  intents: DecomposedIntentPayload[];
  confidence: number;
  uncertain: boolean;
  /** 分解器不可用（超时 / 调用失败）：本条按普通聊天发送，未落缓存，可重试。 */
  unavailable: boolean;
  /** 本回合资源作用域；未解析时工具/提示词为空，`status` 说明原因。 */
  scope?: ScopePayload;
}

/** 确认卡里的一行：一个可执行步骤 + 拆解阶段的意图元数据。 */
export interface PlanConfirmIntent {
  /** 计划步骤 id；为只读快照（拿不到计划详情）时为空串。 */
  step_id: string;
  intent_id: string;
  intent_name: string;
  instruction: string;
  confidence: number;
  uncertain: boolean;
  allowed_tools: string[];
  model_id: string | null;
  probe_evidence: string | null;
}

/** 拆解一条消息并落一条待确认计划（不产生任何副作用）。 */
export async function decomposeMessage(
  sessionId: string,
  content: string,
  turnId?: string,
): Promise<DecomposeMessageResult> {
  return invoke('decompose_message', { sessionId, content, turnId: turnId ?? null });
}

/** 改写待确认步骤的指令，返回更新后的计划。 */
export async function updatePlanStep(
  planId: string,
  stepId: string,
  instruction: string,
): Promise<ExecutionPlan> {
  return invoke('update_plan_step', { planId, stepId, instruction });
}

/** 从待确认计划里移除一个步骤，返回更新后的计划。 */
export async function removePlanStep(
  planId: string,
  stepId: string,
): Promise<ExecutionPlan> {
  return invoke('remove_plan_step', { planId, stepId });
}

/** 丢弃待确认计划；该消息不再发送。 */
export async function cancelPlan(planId: string): Promise<void> {
  return invoke('cancel_plan', { planId });
}

/**
 * 把计划步骤投影成确认卡条目。
 *
 * 计划步骤是「真正能被编辑/删除」的对象（`step_id`），拆解 payload 只补足
 * 意图名与探针证据。拿不到计划详情时退化为只读 payload（`step_id` 为空）。
 */
export function buildPlanConfirmIntents(
  plan: ExecutionPlan | null,
  payload: DecomposedIntentPayload[],
): PlanConfirmIntent[] {
  if (!plan) {
    return payload.map((item) => ({
      step_id: '',
      intent_id: item.intent_id,
      intent_name: item.intent_name,
      instruction: item.instruction,
      confidence: item.confidence,
      uncertain: item.uncertain,
      allowed_tools: item.allowed_tools,
      model_id: item.model_id,
      probe_evidence: item.probe_evidence,
    }));
  }

  const byIntent = new Map(payload.map((item) => [item.intent_id, item]));
  const out: PlanConfirmIntent[] = [];

  for (const step of plan.steps) {
    if (step.execution.type !== 'agent_task') continue;
    const matched = step.intent_id ? byIntent.get(step.intent_id) : undefined;
    out.push({
      step_id: step.id,
      intent_id: step.intent_id ?? matched?.intent_id ?? '',
      intent_name: matched?.intent_name ?? step.label,
      instruction: step.execution.instruction,
      confidence: step.confidence ?? matched?.confidence ?? 0,
      uncertain: step.uncertain ?? matched?.uncertain ?? false,
      allowed_tools: step.execution.allowed_tools ?? matched?.allowed_tools ?? [],
      model_id: step.execution.model_id ?? matched?.model_id ?? null,
      probe_evidence: matched?.probe_evidence ?? null,
    });
  }

  return out;
}
