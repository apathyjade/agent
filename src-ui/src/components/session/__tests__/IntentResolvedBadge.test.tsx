import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntentResolvedBadge } from '../IntentResolvedBadge';
import type { IntentResolvedEvent } from '../../../api/intent';

function makeIntentResolved(overrides: Partial<IntentResolvedEvent> = {}): IntentResolvedEvent {
  return {
    session_id: 'session-1',
    domain_id: 'domain-1',
    domain_name: '程序开发',
    intent_id: 'intent-1',
    intent_name: '实现功能',
    confidence: 0.82,
    source: 'llm',
    widened: false,
    ...overrides,
  };
}

describe('IntentResolvedBadge', () => {
  it('renders domain and intent names', () => {
    render(<IntentResolvedBadge intent={makeIntentResolved()} />);

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.textContent).toContain('程序开发');
    expect(badge.textContent).toContain('实现功能');
  });

  it('shows confidence and source in the title', () => {
    render(<IntentResolvedBadge intent={makeIntentResolved({ confidence: 0.82, source: 'llm' })} />);

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.getAttribute('title')).toContain('来源: llm');
    expect(badge.getAttribute('title')).toContain('置信度: 82%');
  });

  it('shows widened flag in the title when true', () => {
    render(<IntentResolvedBadge intent={makeIntentResolved({ widened: true })} />);

    const badge = screen.getByTestId('intent-resolved-badge');
    expect(badge.getAttribute('title')).toContain('已扩域');
  });
});
