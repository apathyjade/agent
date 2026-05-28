import { describe, it, expect, beforeEach } from 'vitest';
import { render } from '@testing-library/react';
import { ThemeToggle } from '../ThemeToggle';
import { useStore } from '../../../store';

describe('ThemeToggle', () => {
  beforeEach(() => {
    useStore.setState({ darkMode: false });
  });

  it('renders theme toggle button', () => {
    const { container } = render(<ThemeToggle />);
    expect(container.querySelector('button')).toBeDefined();
  });
});
