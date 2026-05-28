import { describe, it, expect, beforeEach } from 'vitest';
import { render } from '@testing-library/react';
import { ToastContainer } from '../Toast';
import { useStore } from '../../../store';

describe('Toast', () => {
  beforeEach(() => {
    useStore.setState({ toasts: [] });
  });

  it('renders nothing when no toasts', () => {
    const { container } = render(<ToastContainer />);
    expect(container.firstChild).toBeNull();
  });

  it('renders active toasts', () => {
    useStore.setState({
      toasts: [{ id: '1', message: 'Test toast', type: 'info' }],
    });

    const { container } = render(<ToastContainer />);
    expect(container.textContent).toContain('Test toast');
  });
});
