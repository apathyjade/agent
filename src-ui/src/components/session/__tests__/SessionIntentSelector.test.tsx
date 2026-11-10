import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useStore } from '../../../store';
import { SessionIntentSelector } from '../SessionIntentSelector';
import type { IntentDomainInfo } from '../../../api/intent';

vi.mock('../../../api/intent', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../api/intent')>();
  return {
    ...actual,
    listIntentDomains: vi.fn(),
    setSessionIntentDomain: vi.fn(),
  };
});

import { listIntentDomains, setSessionIntentDomain } from '../../../api/intent';

const listIntentDomainsMock = vi.mocked(listIntentDomains);
const setSessionIntentDomainMock = vi.mocked(setSessionIntentDomain);

function makeDomain(overrides: Partial<IntentDomainInfo> = {}): IntentDomainInfo {
  return {
    id: 'domain-1',
    name: '程序开发',
    description: '',
    is_active: false,
    source: 'builtin',
    version: 1,
    created_at: '',
    updated_at: '',
    ...overrides,
  };
}

describe('SessionIntentSelector', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useStore.setState({
      currentSession: null,
      intentDomainId: null,
      intentDomainName: null,
      intentDomainMode: 'none',
    });
    listIntentDomainsMock.mockResolvedValue([]);
    setSessionIntentDomainMock.mockResolvedValue({
      domain_id: 'domain-1',
      domain_name: '程序开发',
      mode: 'pinned',
    });
  });

  it('renders nothing when there is no current session', () => {
    const { container } = render(<SessionIntentSelector />);
    expect(container.firstChild).toBeNull();
    expect(listIntentDomainsMock).not.toHaveBeenCalled();
  });

  it('lists domains and defaults to the generic option', async () => {
    useStore.setState({
      currentSession: { id: 'session-1', title: '测试', model_id: 'm1', created_at: '', updated_at: '' },
      intentDomainMode: 'none',
    });
    listIntentDomainsMock.mockResolvedValue([
      makeDomain({ id: 'domain-1', name: '程序开发' }),
      makeDomain({ id: 'domain-2', name: '写作' }),
    ]);

    render(<SessionIntentSelector />);

    const select = await screen.findByLabelText('会话领域');
    expect(select).toBeTruthy();
    expect((select as HTMLSelectElement).value).toBe('');
    expect(screen.getByText('程序开发')).toBeTruthy();
    expect(screen.getByText('写作')).toBeTruthy();
    // 无绑定时显示「通用（不绑定）」而不是旧的「自动」。
    expect(screen.queryByText('自动')).toBeNull();
    expect(screen.getByText('通用（不绑定）')).toBeTruthy();
    expect(screen.getByText('通用')).toBeTruthy();
  });

  it('selecting a domain calls the command with the right payload', async () => {
    const user = userEvent.setup();
    useStore.setState({
      currentSession: { id: 'session-1', title: '测试', model_id: 'm1', created_at: '', updated_at: '' },
      intentDomainMode: 'none',
    });
    listIntentDomainsMock.mockResolvedValue([
      makeDomain({ id: 'domain-1', name: '程序开发' }),
      makeDomain({ id: 'domain-2', name: '写作' }),
    ]);

    render(<SessionIntentSelector />);

    const select = await screen.findByLabelText('会话领域');
    await user.selectOptions(select, 'domain-2');

    await waitFor(() =>
      expect(setSessionIntentDomainMock).toHaveBeenCalledWith('session-1', 'domain-2')
    );
  });

  it('selecting the generic option sends null to the command', async () => {
    const user = userEvent.setup();
    useStore.setState({
      currentSession: { id: 'session-1', title: '测试', model_id: 'm1', created_at: '', updated_at: '' },
      intentDomainId: 'domain-1',
      intentDomainMode: 'pinned',
    });
    listIntentDomainsMock.mockResolvedValue([
      makeDomain({ id: 'domain-1', name: '程序开发' }),
    ]);
    setSessionIntentDomainMock.mockResolvedValue({
      domain_id: null,
      domain_name: null,
      mode: 'none',
    });

    render(<SessionIntentSelector />);

    const select = await screen.findByLabelText('会话领域');
    await user.selectOptions(select, '');

    await waitFor(() =>
      expect(setSessionIntentDomainMock).toHaveBeenCalledWith('session-1', null)
    );
  });
});
