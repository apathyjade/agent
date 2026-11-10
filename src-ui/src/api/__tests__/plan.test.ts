import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import type { ExecutionPlan } from '../../types';
import {
  buildPlanConfirmIntents,
  cancelPlan,
  decomposeMessage,
  removePlanStep,
  updatePlanStep,
} from '../plan';
import type { DecomposedIntentPayload } from '../plan';

const invokeMock = vi.mocked(invoke);

function payload(overrides: Partial<DecomposedIntentPayload> = {}): DecomposedIntentPayload {
  return {
    intent_id: 'test.a',
    intent_name: '实现功能',
    instruction: '实现 CSV 导出',
    confidence: 0.82,
    uncertain: false,
    allowed_tools: ['read', 'shell'],
    model_id: 'model-a',
    max_iterations: 5,
    path: 'fast',
    probe_evidence: null,
    ...overrides,
  };
}

function planWith(overrides: Partial<ExecutionPlan> = {}): ExecutionPlan {
  return {
    id: 'plan-1',
    session_id: 'session-1',
    source: { type: 'dynamic', goal: '实现 CSV 导出', generated_by: 'intent_decomposition' },
    steps: [
      {
        id: 'step-1',
        label: '实现功能',
        execution: {
          type: 'agent_task',
          instruction: '实现 CSV 导出',
          model_id: 'model-a',
          max_iterations: 5,
          allowed_tools: ['read', 'shell'],
        },
        status: 'pending',
        intent_id: 'test.a',
        confidence: 0.82,
        uncertain: false,
      },
    ],
    status: 'pending',
    created_at: '2026-11-09T00:00:00Z',
    finished_at: null,
    ...overrides,
  };
}

describe('plan api', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('decomposeMessage sends sessionId and content', async () => {
    invokeMock.mockResolvedValue({ plan_id: null, intents: [], confidence: 0, uncertain: false });

    await decomposeMessage('session-1', '你好');

    expect(invokeMock).toHaveBeenCalledWith('decompose_message', {
      sessionId: 'session-1',
      content: '你好',
    });
  });

  it('updatePlanStep sends planId / stepId / instruction', async () => {
    await updatePlanStep('plan-1', 'step-1', '改成别的');

    expect(invokeMock).toHaveBeenCalledWith('update_plan_step', {
      planId: 'plan-1',
      stepId: 'step-1',
      instruction: '改成别的',
    });
  });

  it('removePlanStep sends planId and stepId', async () => {
    await removePlanStep('plan-1', 'step-1');

    expect(invokeMock).toHaveBeenCalledWith('remove_plan_step', {
      planId: 'plan-1',
      stepId: 'step-1',
    });
  });

  it('cancelPlan sends planId', async () => {
    await cancelPlan('plan-1');

    expect(invokeMock).toHaveBeenCalledWith('cancel_plan', { planId: 'plan-1' });
  });
});

describe('buildPlanConfirmIntents', () => {
  it('maps plan steps and enriches them with the decompose payload', () => {
    const intents = buildPlanConfirmIntents(planWith(), [payload({ probe_evidence: 'README 提到导出' })]);

    expect(intents).toHaveLength(1);
    expect(intents[0]).toEqual({
      step_id: 'step-1',
      intent_id: 'test.a',
      intent_name: '实现功能',
      instruction: '实现 CSV 导出',
      confidence: 0.82,
      uncertain: false,
      allowed_tools: ['read', 'shell'],
      model_id: 'model-a',
      probe_evidence: 'README 提到导出',
    });
  });

  it('falls back to the step snapshot when the plan is missing', () => {
    const intents = buildPlanConfirmIntents(null, [payload()]);

    expect(intents).toHaveLength(1);
    expect(intents[0].step_id).toBe('');
    expect(intents[0].intent_name).toBe('实现功能');
  });

  it('skips non agent-task steps', () => {
    const plan = planWith({
      steps: [
        {
          id: 'step-1',
          label: '工具',
          execution: { type: 'tool_call', tool: 'shell', params: {} },
          status: 'pending',
        },
      ],
    });

    expect(buildPlanConfirmIntents(plan, [payload()])).toEqual([]);
  });
});
