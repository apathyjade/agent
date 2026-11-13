import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('../../../api/intent', () => ({ getRecentRouteTraces: vi.fn() }));

import { getRecentRouteTraces } from '../../../api/intent';
import type { RouteTrace } from '../../../api/intent';
import { RouteTracePanel, routeScopeSummary } from '../RouteTracePanel';

const getTracesMock = vi.mocked(getRecentRouteTraces);

function trace(overrides: Partial<RouteTrace> = {}): RouteTrace {
  return {
    at: '2026-11-14T02:00:00Z',
    session_id: 'session-1',
    message_excerpt: '帮我导出 CSV',
    intents: [
      {
        intent_id: 'dev.export',
        intent_name: '实现功能',
        instruction: '导出 CSV',
        confidence: 0.8,
        uncertain: false,
      },
    ],
    candidates: [],
    confidence: 0.8,
    uncertain: false,
    source: 'llm',
    elapsed_ms: 120,
    llm_calls: 2,
    probe_used: false,
    scope: { catalog_size: 12, selected: 4, cached: false, skipped: null },
    ...overrides,
  };
}

describe('routeScopeSummary', () => {
  it('summarizes an applied narrowing', () => {
    expect(routeScopeSummary(trace())).toBe('收窄 4/12');
  });

  it('marks cache hits', () => {
    const cached = trace({
      scope: { catalog_size: 12, selected: 4, cached: true, skipped: null },
    });
    expect(routeScopeSummary(cached)).toBe('收窄 4/12（缓存）');
  });

  it('explains why narrowing was skipped', () => {
    const skipped = trace({
      scope: {
        catalog_size: 12,
        selected: 0,
        cached: false,
        skipped: 'skipped_small_catalog',
      },
    });
    expect(routeScopeSummary(skipped)).toBe('候选资源太少，未收窄');
  });

  it('returns null when the backend sent no scope', () => {
    expect(routeScopeSummary(trace({ scope: null }))).toBeNull();
  });
});

describe('RouteTracePanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('lists recent traces with the narrowing summary', async () => {
    const user = userEvent.setup();
    getTracesMock.mockResolvedValue([trace()]);
    render(<RouteTracePanel />);

    await user.click(screen.getByTestId('route-trace-open'));

    const row = await screen.findByTestId('route-trace');
    expect(row.textContent).toContain('帮我导出 CSV');
    expect(row.textContent).toContain('实现功能');
    expect(screen.getByTestId('route-trace-scope').textContent).toBe('收窄 4/12');
  });

  it('shows an empty state when nothing has been routed', async () => {
    const user = userEvent.setup();
    getTracesMock.mockResolvedValue([]);
    render(<RouteTracePanel />);

    await user.click(screen.getByTestId('route-trace-open'));

    expect(await screen.findByText('还没有路由记录。')).toBeTruthy();
  });
});
