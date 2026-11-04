import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import {
  importIntentDomain,
  listIntents,
  setActiveIntentDomain,
  updateIntent,
} from '../intent';

const invokeMock = vi.mocked(invoke);

describe('intent api', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('setActiveIntentDomain invokes the right command', async () => {
    await setActiveIntentDomain('domain-1');

    expect(invokeMock).toHaveBeenCalledWith('set_active_intent_domain', { id: 'domain-1' });
  });

  it('listIntents sends domainId as a camelCase command argument', async () => {
    await listIntents('domain-1');

    expect(invokeMock).toHaveBeenCalledWith('list_intents', { domainId: 'domain-1' });
  });

  it('importIntentDomain passes json and name', async () => {
    await importIntentDomain('[]', '导入领域');

    expect(invokeMock).toHaveBeenCalledWith('import_intent_domain', {
      json: '[]',
      name: '导入领域',
    });
  });

  it('updateIntent passes a sparse payload through unchanged', async () => {
    const payload = { name: '新名字' };

    await updateIntent('intent-1', payload);

    expect(invokeMock).toHaveBeenCalledWith('update_intent', {
      id: 'intent-1',
      intent: payload,
    });
    expect(invokeMock.mock.calls[0][1]).toEqual({ id: 'intent-1', intent: payload });
  });

  it('updateIntent keeps explicit nulls so the backend clears a value', async () => {
    await updateIntent('intent-1', { max_iterations: null, examples: [] });

    expect(invokeMock).toHaveBeenCalledWith('update_intent', {
      id: 'intent-1',
      intent: { max_iterations: null, examples: [] },
    });
  });
});
