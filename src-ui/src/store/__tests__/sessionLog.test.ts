import { beforeEach, describe, expect, it, vi } from 'vitest';
import { create } from 'zustand';

vi.mock('../../api/tauri', () => ({
  getSessionTurnLogs: vi.fn(),
  getSessionTurnLog: vi.fn(),
}));

import * as api from '../../api/tauri';
import { createSessionSlice } from '../sessionSlice';
import type { SessionSlice } from '../sessionSlice';
import type { SessionLogEvent, TurnLogEntry } from '../../api/session';

const apiMock = vi.mocked(api);

function testStore() {
  return create<SessionSlice>((...args) => ({
    ...createSessionSlice(...(args as Parameters<typeof createSessionSlice>)),
  }));
}

function entry(overrides: Partial<TurnLogEntry> = {}): TurnLogEntry {
  return {
    seq: 1,
    at: '2026-01-01T00:00:00Z',
    stage: 'answer',
    level: 'info',
    title: '输出回答',
    summary: '123 字',
    elapsed_ms: 1000,
    ...overrides,
  };
}

function event(overrides: Partial<SessionLogEvent> = {}): SessionLogEvent {
  return {
    session_id: 'session-1',
    turn_id: 'turn-1',
    entry: entry(),
    ...overrides,
  };
}

describe('sessionSlice 会话日志', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('beginTurn 生成并保存 activeTurnId', () => {
    const store = testStore();
    const turnId = store.getState().beginTurn();

    expect(turnId).toBeTruthy();
    expect(store.getState().activeTurnId).toBe(turnId);
    expect(store.getState().liveTurnLog).toEqual({ turnId, entries: [] });
  });

  it('appendLiveLog 追加当前会话条目', () => {
    const store = testStore();
    store.setState({
      currentSession: { id: 'session-1', title: '测试', model_id: 'm', created_at: '', updated_at: '' },
    });

    store.getState().appendLiveLog(event());
    store.getState().appendLiveLog(event({ entry: entry({ seq: 2, title: '调用工具' }) }));

    const live = store.getState().liveTurnLog;
    expect(live?.turnId).toBe('turn-1');
    expect(live?.entries.map((e) => e.title)).toEqual(['输出回答', '调用工具']);
  });

  it('appendLiveLog 丢弃其他会话的事件', () => {
    const store = testStore();
    store.setState({
      currentSession: { id: 'session-1', title: '测试', model_id: 'm', created_at: '', updated_at: '' },
    });

    store.getState().appendLiveLog(event({ session_id: 'session-2' }));

    expect(store.getState().liveTurnLog).toBeNull();
  });

  it('fetchSessionLogs 拉取当前会话历史', async () => {
    const logs = [
      {
        turn_id: 'turn-1',
        session_id: 'session-1',
        status: 'completed' as const,
        path: 'fast' as const,
        question: '你好',
        model_id: null,
        persona_name: null,
        started_at: '2026-01-01T00:00:00Z',
        total_ms: 1200,
        entry_count: 2,
      },
    ];
    apiMock.getSessionTurnLogs.mockResolvedValue(logs);
    const store = testStore();
    store.setState({
      currentSession: { id: 'session-1', title: '测试', model_id: 'm', created_at: '', updated_at: '' },
    });

    await store.getState().fetchSessionLogs();

    expect(apiMock.getSessionTurnLogs).toHaveBeenCalledWith('session-1');
    expect(store.getState().sessionLogs).toEqual(logs);
  });
});
