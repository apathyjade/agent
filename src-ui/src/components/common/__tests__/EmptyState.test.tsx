import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { EmptyState } from '../EmptyState';

describe('EmptyState', () => {
  it('renders sessions type', () => {
    const { container } = render(<EmptyState type="sessions" />);
    expect(container.textContent).toContain('会话');
  });

  it('renders messages type', () => {
    const { container } = render(<EmptyState type="messages" />);
    expect(container.textContent).toContain('对话');
  });
});
