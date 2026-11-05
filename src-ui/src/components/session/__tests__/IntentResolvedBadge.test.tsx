import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import {
  IntentResolvedBadge,
  IntentUncertaintyNotice,
} from '../IntentResolvedBadge';
import type { IntentResolvedEvent } from '../../../api/intent';

function makeIntentResolved(
  overrides: Partial<IntentResolvedEvent> = {},
): IntentResolvedEvent {
  return {
    session_id: 'session-1',
    domain_id: 'domain-1',
    domain_name: '程序开发',
    intent_id: 'intent-1',
    intent_name: '实现功能',
    confidence: 0.82,
    uncertain: false,
    source: 'llm',
    candidates: [],
    elapsed_ms: 12,
    llm_calls: 1,
    probe_used: false,
    ...overrides,
  };
}

describe('IntentResolvedBadge', () => {
  it('renders domain and intent names when certain', () => {
    render(<IntentResolvedBadge intent={makeIntentResolved()} />);

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.textContent).toContain('程序开发');
    expect(badge.textContent).toContain('实现功能');
  });

  it('shows confidence and source in the title', () => {
    render(
      <IntentResolvedBadge
        intent={makeIntentResolved({ confidence: 0.82, source: 'llm' })}
      />,
    );

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.getAttribute('title')).toContain('来源: llm');
    expect(badge.getAttribute('title')).toContain('置信度: 82%');
  });

  it('renders possible intent label when uncertain', () => {
    render(
      <IntentResolvedBadge
        intent={makeIntentResolved({ uncertain: true, intent_name: '实现功能' })}
      />,
    );

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.textContent).toContain('可能：实现功能');
    expect(badge.textContent).not.toContain('/');
  });

  it('shows top candidates in the title', () => {
    render(
      <IntentResolvedBadge
        intent={makeIntentResolved({
          confidence: 0.45,
          source: 'embedding',
          candidates: [
            { intent_id: 'i1', name: '实现功能', domain_id: 'd1', score: 0.45, signal: 'embedding' },
            { intent_id: 'i2', name: '修复缺陷', domain_id: 'd1', score: 0.3, signal: 'embedding' },
            { intent_id: 'i3', name: '编写测试', domain_id: 'd1', score: 0.15, signal: 'embedding' },
          ],
        })}
      />,
    );

    const badge = screen.getByTestId('intent-resolved-badge');
    const title = badge.getAttribute('title');
    expect(title).toContain('候选: 实现功能 45%');
    expect(title).toContain('修复缺陷 30%');
    expect(title).toContain('编写测试 15%');
  });

  it('limits candidates in the title to top three', () => {
    render(
      <IntentResolvedBadge
        intent={makeIntentResolved({
          candidates: [
            { intent_id: 'i1', name: 'a', domain_id: 'd1', score: 0.4, signal: 'embedding' },
            { intent_id: 'i2', name: 'b', domain_id: 'd1', score: 0.3, signal: 'embedding' },
            { intent_id: 'i3', name: 'c', domain_id: 'd1', score: 0.2, signal: 'embedding' },
            { intent_id: 'i4', name: 'd', domain_id: 'd1', score: 0.1, signal: 'embedding' },
          ],
        })}
      />,
    );

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.getAttribute('title')).toContain('候选: a 40% · b 30% · c 20%');
    expect(badge.getAttribute('title')).not.toContain('d 10%');
  });
});

describe('IntentUncertaintyNotice', () => {
  it('renders a non-blocking notice when the current intent is uncertain', () => {
    render(
      <IntentUncertaintyNotice
        intent={makeIntentResolved({ uncertain: true, intent_name: '实现功能' })}
      />,
    );

    const notice = screen.getByTestId('intent-uncertainty-notice');
    expect(notice.textContent).toBe(
      '不确定意图，先按「实现功能」继续；可切换领域后重发',
    );
  });

  it('renders nothing when the current intent is certain', () => {
    const { container } = render(
      <IntentUncertaintyNotice intent={makeIntentResolved({ uncertain: false })} />,
    );

    expect(container.firstChild).toBeNull();
  });

  it('renders nothing when there is no current intent', () => {
    const { container } = render(<IntentUncertaintyNotice intent={null} />);

    expect(container.firstChild).toBeNull();
  });
});
