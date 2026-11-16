import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('../../../api/session', () => ({
  getSessionTurnLog: vi.fn(),
  getSessionTurnLogs: vi.fn(),
}));

import { getSessionTurnLog } from '../../../api/session';
import { SessionLogDrawer } from '../SessionLogDrawer';
import { useStore } from '../../../store';
import type { TurnLogRecord, TurnLogSummary } from '../../../api/session';

const getTurnLogMock = vi.mocked(getSessionTurnLog);

function summary(overrides: Partial<TurnLogSummary> = {}): TurnLogSummary {
  return {
    turn_id: 'turn-1',
    session_id: 'session-1',
    status: 'completed',
    path: 'fast',
    question: '帮我解释一下这段代码',
    model_id: 'model-1',
    persona_name: null,
    started_at: '2026-01-01T10:00:00Z',
    total_ms: 1200,
    entry_count: 2,
    ...overrides,
  };
}

describe('SessionLogDrawer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useStore.setState({
      sessionLogOpen: true,
      sessionLogs: [],
      liveTurnLog: null,
    });
  });

  it('无记录时显示空状态文案', () => {
    render(<SessionLogDrawer />);

    expect(screen.getByText('还没有可查看的记录，发送一条消息后再来')).toBeTruthy();
  });

  it('渲染历史回合行与路径标签', () => {
    useStore.setState({ sessionLogs: [summary()] });
    render(<SessionLogDrawer />);

    expect(screen.getByText('帮我解释一下这段代码')).toBeTruthy();
    expect(screen.getByText('快速回答')).toBeTruthy();
    expect(screen.getByText('完成')).toBeTruthy();
  });

  it('点击行按需加载并展开完整明细', async () => {
    const record: TurnLogRecord = {
      ...summary(),
      finished_at: '2026-01-01T10:00:01Z',
      entries: [
        {
          seq: 1,
          at: '2026-01-01T10:00:00Z',
          stage: 'route',
          level: 'info',
          title: '选择执行路径',
          summary: '快速回答',
          elapsed_ms: 30,
          detail: { reason: '零意图' },
        },
      ],
    };
    getTurnLogMock.mockResolvedValue(record);
    useStore.setState({ sessionLogs: [summary()] });
    render(<SessionLogDrawer />);

    await userEvent.click(screen.getByText('帮我解释一下这段代码'));

    expect(getTurnLogMock).toHaveBeenCalledWith('turn-1');
    expect(await screen.findByText('选择执行路径')).toBeTruthy();
    expect(screen.getByText('原始数据')).toBeTruthy();
  });
});
