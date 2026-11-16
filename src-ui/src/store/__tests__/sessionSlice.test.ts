import { beforeEach, describe, expect, it, vi } from 'vitest';
import { create } from 'zustand';

vi.mock('../../api/tauri', () => ({
  decomposeMessage: vi.fn(),
  getPlanDetail: vi.fn(),
  cancelPlan: vi.fn(),
  updatePlanStep: vi.fn(),
  removePlanStep: vi.fn(),
  setSessionIntentDomain: vi.fn(),
}));

import * as api from '../../api/tauri';
import { createSessionSlice } from '../sessionSlice';
import type { SessionSlice } from '../sessionSlice';

const apiMock = vi.mocked(api);

function testStore() {
  const sendMessageStream = vi.fn().mockResolvedValue(undefined);
  const store = create<SessionSlice>((...args) => ({
    ...createSessionSlice(...(args as Parameters<typeof createSessionSlice>)),
    // 流式发送由另一个 slice 提供，这里注入替身以便断言「消息是否发送」。
    ...({
      sendMessageStream,
    } as unknown as SessionSlice),
  }));
  return { store, sendMessageStream };
}

function seedSession(store: ReturnType<typeof testStore>['store']) {
  store.setState({
    currentSession: {
      id: 'session-1',
      title: '测试',
      model_id: 'model-1',
      created_at: '',
      updated_at: '',
    },
  });
}

const emptyResult = { plan_id: null, intents: [], confidence: 0, uncertain: false, unavailable: false };

describe('sessionSlice plan confirmation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('零意图时不弹确认卡，返回 chat', async () => {
    apiMock.decomposeMessage.mockResolvedValue(emptyResult);
    const { store } = testStore();
    seedSession(store);

    const outcome = await store.getState().requestPlan('今天天气怎么样');

    expect(outcome).toBe('chat');
    expect(apiMock.decomposeMessage).toHaveBeenCalledWith('session-1', '今天天气怎么样', expect.any(String));
    expect(apiMock.getPlanDetail).not.toHaveBeenCalled();
    expect(store.getState().planConfirmOpen).toBe(false);
    expect(store.getState().planId).toBeNull();
  });

  it('合法零意图会缓存 0 步计划，后续消息不再拆解', async () => {
    apiMock.decomposeMessage.mockResolvedValue({
      plan_id: 'plan-empty',
      intents: [],
      confidence: 0,
      uncertain: false,
      unavailable: false,
    });
    const { store } = testStore();
    seedSession(store);

    const outcome = await store.getState().requestPlan('你好');

    expect(outcome).toBe('chat');
    expect(store.getState().planConfirmOpen).toBe(false);
    // 后端已落 0 步计划；本地同步指向，下一条消息直接走缓存。
    expect(store.getState().currentSession?.active_plan_id).toBe('plan-empty');

    const again = await store.getState().requestPlan('继续说');
    expect(again).toBe('chat');
    expect(apiMock.decomposeMessage).toHaveBeenCalledTimes(1);
  });

  it('分解器不可用时提示用户、不弹确认卡、也不缓存', async () => {
    apiMock.decomposeMessage.mockResolvedValue({
      plan_id: null,
      intents: [],
      confidence: 0,
      uncertain: false,
      unavailable: true,
    });
    const { store } = testStore();
    seedSession(store);

    const outcome = await store.getState().requestPlan('实现导出');

    expect(outcome).toBe('chat');
    expect(store.getState().planConfirmOpen).toBe(false);
    expect(store.getState().currentSession?.active_plan_id).toBeFalsy();
    expect(
      (store.getState() as unknown as { error?: string }).error,
    ).toContain('不可用');
    // 下一条消息应重试拆解，而不是被固化成零意图会话。
    apiMock.decomposeMessage.mockResolvedValue({ ...emptyResult, plan_id: 'plan-1' });
    await store.getState().requestPlan('再试一次');
    expect(apiMock.decomposeMessage).toHaveBeenCalledTimes(2);
  });

  it('拆解失败时退化为普通聊天，消息仍可发送', async () => {
    apiMock.decomposeMessage.mockRejectedValue(new Error('model down'));
    const { store } = testStore();
    seedSession(store);

    const outcome = await store.getState().requestPlan('实现导出');

    expect(outcome).toBe('chat');
    expect(store.getState().planConfirmOpen).toBe(false);
  });

  it('会话已有拆解结果时，后续消息不重新拆解', async () => {
    const { store } = testStore();
    store.setState({
      currentSession: {
        id: 'session-1',
        title: '测试',
        model_id: 'model-1',
        created_at: '',
        updated_at: '',
        active_plan_id: 'plan-1',
      },
    });

    const outcome = await store.getState().requestPlan('继续说');

    expect(outcome).toBe('chat');
    expect(apiMock.decomposeMessage).not.toHaveBeenCalled();
  });

  it('有意图时打开确认卡并带上步骤 id', async () => {
    apiMock.decomposeMessage.mockResolvedValue({
      plan_id: 'plan-1',
      intents: [
        {
          intent_id: 'test.a',
          intent_name: '实现功能',
          instruction: '实现 CSV 导出',
          confidence: 0.9,
          uncertain: false,
          allowed_tools: ['read'],
          model_id: null,
          max_iterations: null,
          path: 'fast',
          probe_evidence: null,
        },
      ],
      confidence: 0.9,
      uncertain: false,
      unavailable: false,
    });
    apiMock.getPlanDetail.mockResolvedValue({
      plan: {
        id: 'plan-1',
        session_id: 'session-1',
        source: { type: 'dynamic', goal: '', generated_by: 'intent_decomposition' },
        steps: [
          {
            id: 'step-1',
            label: '实现功能',
            execution: {
              type: 'agent_task',
              instruction: '实现 CSV 导出',
              allowed_tools: ['read'],
            },
            status: 'pending',
            intent_id: 'test.a',
            confidence: 0.9,
            uncertain: false,
          },
        ],
        status: 'pending',
        created_at: '',
        finished_at: null,
      },
      step_records: [],
    });

    const { store } = testStore();
    seedSession(store);

    const outcome = await store.getState().requestPlan('实现 CSV 导出');

    expect(outcome).toBe('confirm');
    expect(store.getState().planConfirmOpen).toBe(true);
    expect(store.getState().planId).toBe('plan-1');
    expect(store.getState().planIntents).toHaveLength(1);
    expect(store.getState().planIntents[0].step_id).toBe('step-1');
    expect(store.getState().planDraftContent).toBe('实现 CSV 导出');
  });

  it('重新规划会丢弃旧计划并重新拆解', async () => {
    const intentPayload = {
      intent_id: 'test.a',
      intent_name: '实现功能',
      instruction: '实现 CSV 导出',
      confidence: 0.9,
      uncertain: false,
      allowed_tools: [],
      model_id: null,
      max_iterations: null,
      path: 'fast',
      probe_evidence: null,
    };
    apiMock.decomposeMessage.mockResolvedValueOnce({
      plan_id: 'plan-1',
      intents: [intentPayload],
      confidence: 0.9,
      uncertain: false,
      unavailable: false,
    });
    apiMock.getPlanDetail.mockResolvedValue(null);

    const { store } = testStore();
    seedSession(store);
    await store.getState().requestPlan('实现 CSV 导出');

    apiMock.decomposeMessage.mockResolvedValueOnce({
      plan_id: 'plan-2',
      intents: [intentPayload],
      confidence: 0.9,
      uncertain: false,
      unavailable: false,
    });
    await store.getState().replanPlan();

    expect(apiMock.cancelPlan).toHaveBeenCalledWith('plan-1');
    expect(apiMock.decomposeMessage).toHaveBeenCalledTimes(2);
    expect(store.getState().planId).toBe('plan-2');
    expect(store.getState().planConfirmOpen).toBe(true);
  });

  it('取消确认时丢弃计划且不发送消息', async () => {
    apiMock.decomposeMessage.mockResolvedValue({
      plan_id: 'plan-1',
      intents: [
        {
          intent_id: 'test.a',
          intent_name: '实现功能',
          instruction: '实现 CSV 导出',
          confidence: 0.9,
          uncertain: false,
          allowed_tools: [],
          model_id: null,
          max_iterations: null,
          path: 'fast',
          probe_evidence: null,
        },
      ],
      confidence: 0.9,
      uncertain: false,
      unavailable: false,
    });
    apiMock.getPlanDetail.mockResolvedValue(null);

    const { store, sendMessageStream } = testStore();
    seedSession(store);
    await store.getState().requestPlan('实现 CSV 导出');

    await store.getState().cancelPlanConfirmation();

    expect(apiMock.cancelPlan).toHaveBeenCalledWith('plan-1');
    expect(sendMessageStream).not.toHaveBeenCalled();
    expect(store.getState().planConfirmOpen).toBe(false);
    expect(store.getState().planId).toBeNull();
  });
});
