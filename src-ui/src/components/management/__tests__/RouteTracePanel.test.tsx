import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('../../../api/intent', () => ({
  getRecentRouteTraces: vi.fn(),
  getScopeMetrics: vi.fn(),
}));

import { getRecentRouteTraces, getScopeMetrics } from '../../../api/intent';
import type { RouteTrace, ScopeMetrics } from '../../../api/intent';
import {
  RouteTracePanel,
  formatMs,
  formatRate,
  metricItems,
  routeScopeSummary,
} from '../RouteTracePanel';

const getTracesMock = vi.mocked(getRecentRouteTraces);
const getMetricsMock = vi.mocked(getScopeMetrics);

function metrics(overrides: Partial<ScopeMetrics> = {}): ScopeMetrics {
  return {
    samples: 4,
    attempted: 2,
    applied: 1,
    cached: 1,
    skipped: 1,
    not_requested: 1,
    selected_total: 6,
    catalog_total: 20,
    avg_elapsed_ms: 150.4,
    avg_llm_calls: 2,
    applied_rate: 1,
    cache_rate: 0.5,
    skip_rate: 0.25,
    selected_ratio: 0.3,
    ...overrides,
  };
}

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

describe('metric formatting', () => {
  it('formats rates as percentages', () => {
    expect(formatRate(0.5)).toBe('50%');
    expect(formatRate(0.25, 1)).toBe('25.0%');
    expect(formatRate(0)).toBe('0%');
    expect(formatRate(Number.NaN)).toBe('0%');
  });

  it('formats milliseconds without fractions', () => {
    expect(formatMs(150.4)).toBe('150 ms');
    expect(formatMs(0)).toBe('0 ms');
  });

  it('flattens metrics into labeled items', () => {
    const items = metricItems(metrics());

    expect(items.map((item) => item.testid)).toEqual([
      'metric-samples',
      'metric-applied-rate',
      'metric-selected-ratio',
      'metric-cache-rate',
      'metric-skip-rate',
      'metric-avg-elapsed',
    ]);
    expect(items.map((item) => item.value)).toEqual(['4', '100%', '30.0%', '50%', '25%', '150 ms']);
  });

  it('returns no items for empty or missing metrics', () => {
    expect(metricItems(null)).toEqual([]);
    expect(metricItems(metrics({ samples: 0 }))).toEqual([]);
  });
});

describe('RouteTracePanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('lists recent traces with the narrowing summary', async () => {
    const user = userEvent.setup();
    getTracesMock.mockResolvedValue([trace()]);
    getMetricsMock.mockResolvedValue(metrics());
    render(<RouteTracePanel />);

    await user.click(screen.getByTestId('route-trace-open'));

    const row = await screen.findByTestId('route-trace');
    expect(row.textContent).toContain('帮我导出 CSV');
    expect(row.textContent).toContain('实现功能');
    expect(screen.getByTestId('route-trace-scope').textContent).toBe('收窄 4/12');
  });

  it('shows the aggregate metrics row above the traces', async () => {
    const user = userEvent.setup();
    getTracesMock.mockResolvedValue([trace()]);
    getMetricsMock.mockResolvedValue(metrics());
    render(<RouteTracePanel />);

    await user.click(screen.getByTestId('route-trace-open'));

    await screen.findByTestId('route-trace');
    expect(screen.getByTestId('metric-samples').textContent).toBe('4');
    expect(screen.getByTestId('metric-applied-rate').textContent).toBe('100%');
    expect(screen.getByTestId('metric-selected-ratio').textContent).toBe('30.0%');
    expect(screen.getByTestId('metric-avg-elapsed').textContent).toBe('150 ms');
  });

  it('shows an empty metrics state when no samples exist', async () => {
    const user = userEvent.setup();
    getTracesMock.mockResolvedValue([]);
    getMetricsMock.mockResolvedValue(metrics({ samples: 0 }));
    render(<RouteTracePanel />);

    await user.click(screen.getByTestId('route-trace-open'));

    expect(await screen.findByText('暂无收窄样本。')).toBeTruthy();
  });

  it('shows an empty state when nothing has been routed', async () => {
    const user = userEvent.setup();
    getTracesMock.mockResolvedValue([]);
    getMetricsMock.mockResolvedValue(metrics({ samples: 0 }));
    render(<RouteTracePanel />);

    await user.click(screen.getByTestId('route-trace-open'));

    expect(await screen.findByText('还没有路由记录。')).toBeTruthy();
  });
});
