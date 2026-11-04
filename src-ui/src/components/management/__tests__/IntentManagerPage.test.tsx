import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

vi.mock('../../../api/intent', () => ({
  listIntentDomains: vi.fn(),
  getActiveIntentDomain: vi.fn(),
  createIntentDomain: vi.fn(),
  deleteIntentDomain: vi.fn(),
  setActiveIntentDomain: vi.fn(),
  listIntents: vi.fn(),
  createIntent: vi.fn(),
  updateIntent: vi.fn(),
  deleteIntent: vi.fn(),
  setIntentEnabled: vi.fn(),
  importIntentDomain: vi.fn(),
  exportIntentDomain: vi.fn(),
}));

import * as api from '../../../api/intent';
import type { IntentDomainInfo, IntentInfo } from '../../../api/intent';
import { IntentManagerPage } from '../IntentManagerPage';

function makeDomain(overrides: Partial<IntentDomainInfo> = {}): IntentDomainInfo {
  return {
    id: 'domain-1',
    name: '程序开发',
    description: '内置的软件开发领域',
    is_active: true,
    source: 'builtin',
    version: 1,
    created_at: '',
    updated_at: '',
    ...overrides,
  };
}

function makeIntent(overrides: Partial<IntentInfo> = {}): IntentInfo {
  return {
    id: 'intent-1',
    domain_id: 'domain-1',
    name: '规划方案',
    description: '',
    examples: [],
    workflow: '',
    allowed_tools: [],
    default_agent: '',
    scope: 'global',
    enabled: true,
    version: 1,
    confidence_threshold: 0.7,
    fallback_intent: '',
    path: 'fast',
    system_prompt_appendix: '',
    model_id: '',
    max_iterations: null,
    sort_order: 0,
    created_at: '',
    updated_at: '',
    ...overrides,
  };
}

describe('IntentManagerPage', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listIntentDomains).mockResolvedValue([]);
    vi.mocked(api.getActiveIntentDomain).mockResolvedValue(null);
    vi.mocked(api.listIntents).mockResolvedValue([]);
  });

  it('renders empty state when no active domain', async () => {
    vi.mocked(api.listIntentDomains).mockResolvedValue([
      makeDomain({ id: 'domain-1', is_active: false }),
    ]);

    render(<IntentManagerPage />);

    expect(await screen.findByText('没有默认领域')).toBeTruthy();
    expect(screen.getByText(/请选择或新建一个领域/)).toBeTruthy();
    // The right column (and its create-intent action) must not render without an active domain.
    expect(screen.queryByRole('button', { name: '新建意图' })).toBeNull();
  });

  it('disables the enable toggle on the last enabled intent', async () => {
    vi.mocked(api.listIntentDomains).mockResolvedValue([makeDomain()]);
    vi.mocked(api.getActiveIntentDomain).mockResolvedValue(makeDomain());
    vi.mocked(api.listIntents).mockResolvedValue([
      makeIntent({ id: 'intent-1', name: '规划方案', enabled: true }),
      makeIntent({ id: 'intent-2', name: '闲聊对话', enabled: false, sort_order: 1 }),
    ]);

    render(<IntentManagerPage />);

    const toggles = await screen.findAllByRole('switch');
    expect(toggles).toHaveLength(2);

    const enabledToggle = toggles.find((el) => el.getAttribute('aria-checked') === 'true');
    expect(enabledToggle).toBeTruthy();
    expect((enabledToggle as HTMLButtonElement).disabled).toBe(true);
    expect(enabledToggle?.getAttribute('title')).toContain('最后一个');
  });

  it('surfaces an error when a delete is rejected', async () => {
    const active = makeDomain({ id: 'domain-1', name: '程序开发', is_active: true });
    const other = makeDomain({
      id: 'domain-2',
      name: '写作',
      is_active: false,
      source: 'user',
      description: '',
    });

    vi.mocked(api.listIntentDomains).mockResolvedValue([active, other]);
    vi.mocked(api.getActiveIntentDomain).mockResolvedValue(active);
    vi.mocked(api.listIntents).mockResolvedValue([makeIntent()]);
    vi.mocked(api.deleteIntentDomain).mockRejectedValue('领域内仍有意图，无法删除');

    const user = userEvent.setup();
    render(<IntentManagerPage />);

    await user.click(await screen.findByLabelText('删除领域 写作'));
    await user.click(screen.getByRole('button', { name: '确认删除' }));

    expect(await screen.findByText(/领域内仍有意图/)).toBeTruthy();
  });

  it('sends only the changed field when editing a description', async () => {
    vi.mocked(api.listIntentDomains).mockResolvedValue([makeDomain()]);
    vi.mocked(api.getActiveIntentDomain).mockResolvedValue(makeDomain());
    vi.mocked(api.listIntents).mockResolvedValue([
      makeIntent({
        id: 'intent-1',
        name: '规划方案',
        description: '旧描述',
        allowed_tools: ['file_system', 'custom_tool'],
      }),
    ]);
    vi.mocked(api.updateIntent).mockResolvedValue(makeIntent());

    const user = userEvent.setup();
    render(<IntentManagerPage />);

    await user.click(await screen.findByText('规划方案'));

    // An imported tool name outside the built-in set must stay visible and checked.
    const customTool = screen.getByLabelText('工具 custom_tool') as HTMLInputElement;
    expect(customTool.checked).toBe(true);

    const description = screen.getByLabelText('描述');
    fireEvent.change(description, { target: { value: '只改描述' } });
    await user.click(screen.getByRole('button', { name: '保存' }));

    await waitFor(() => expect(api.updateIntent).toHaveBeenCalledTimes(1));
    expect(api.updateIntent).toHaveBeenCalledWith('intent-1', { description: '只改描述' });
  });

  it('surfaces an error when an intent delete is rejected', async () => {
    vi.mocked(api.listIntentDomains).mockResolvedValue([makeDomain()]);
    vi.mocked(api.getActiveIntentDomain).mockResolvedValue(makeDomain());
    vi.mocked(api.listIntents).mockResolvedValue([
      makeIntent({ id: 'intent-1', name: '规划方案', enabled: true }),
    ]);
    vi.mocked(api.deleteIntent).mockRejectedValue('不能删除领域内最后一个启用的意图');

    const user = userEvent.setup();
    render(<IntentManagerPage />);

    await user.click(await screen.findByText('规划方案'));
    await user.click(screen.getByRole('button', { name: '删除意图' }));
    await user.click(screen.getByRole('button', { name: '确认删除' }));

    expect(await screen.findByText(/最后一个启用的意图/)).toBeTruthy();
    expect(api.deleteIntent).toHaveBeenCalledWith('intent-1');
  });

  it('does not render an auto_escalate control in the intent editor', async () => {
    vi.mocked(api.listIntentDomains).mockResolvedValue([makeDomain()]);
    vi.mocked(api.getActiveIntentDomain).mockResolvedValue(makeDomain());
    vi.mocked(api.listIntents).mockResolvedValue([makeIntent({ name: '规划方案' })]);

    const user = userEvent.setup();
    render(<IntentManagerPage />);

    await user.click(await screen.findByText('规划方案'));

    expect(screen.queryByLabelText(/自动升级/i)).toBeNull();
    expect(screen.queryByText(/auto_escalate/i)).toBeNull();
  });
});
