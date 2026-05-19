import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ToastContainer } from '../Toast';
import { useStore } from '../../store';

describe('ToastContainer', () => {
  beforeEach(() => {
    useStore.setState({ toasts: [] });
  });

  it('renders nothing when no toasts', () => {
    const { container } = render(<ToastContainer />);
    expect(container.firstChild).toBeNull();
  });

  it('renders toast with message', () => {
    useStore.setState({
      toasts: [{ id: '1', type: 'success', message: 'Done!' }],
    });
    render(<ToastContainer />);
    expect(screen.getByText('Done!')).toBeDefined();
  });

  it('renders multiple toasts', () => {
    useStore.setState({
      toasts: [
        { id: '1', type: 'success', message: 'Success' },
        { id: '2', type: 'error', message: 'Error' },
      ],
    });
    render(<ToastContainer />);
    expect(screen.getByText('Success')).toBeDefined();
    expect(screen.getByText('Error')).toBeDefined();
  });

  it('renders all four toast types', () => {
    useStore.setState({
      toasts: [
        { id: '1', type: 'success', message: 'Ok' },
        { id: '2', type: 'error', message: 'Fail' },
        { id: '3', type: 'warning', message: 'Warn' },
        { id: '4', type: 'info', message: 'Info' },
      ],
    });
    render(<ToastContainer />);
    expect(screen.getByText('Ok')).toBeDefined();
    expect(screen.getByText('Fail')).toBeDefined();
    expect(screen.getByText('Warn')).toBeDefined();
    expect(screen.getByText('Info')).toBeDefined();
  });
});
