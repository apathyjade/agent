import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import {
  getSessionIntentState,
  importIntentDomain,
  listIntents,
  setActiveIntentDomain,
  setSessionIntentDomain,
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

  it('getSessionIntentState invokes the right command', async () => {
    invokeMock.mockResolvedValue({
      domain_id: 'domain-1',
      domain_name: '程序开发',
      mode: 'pinned',
    });

    const result = await getSessionIntentState('session-1');

    expect(invokeMock).toHaveBeenCalledWith('get_session_intent_state', { sessionId: 'session-1' });
    expect(result).toEqual({
      domain_id: 'domain-1',
      domain_name: '程序开发',
      mode: 'pinned',
    });
  });

  it('setSessionIntentDomain sends null for auto mode', async () => {
    invokeMock.mockResolvedValue({
      domain_id: null,
      domain_name: null,
      mode: 'auto',
    });

    const result = await setSessionIntentDomain('session-1', null);

    expect(invokeMock).toHaveBeenCalledWith('set_session_intent_domain', {
      sessionId: 'session-1',
      domainId: null,
    });
    expect(result.mode).toBe('auto');
  });

  it('setSessionIntentDomain sends a domain id for pinned mode', async () => {
    invokeMock.mockResolvedValue({
      domain_id: 'domain-1',
      domain_name: '程序开发',
      mode: 'pinned',
    });

    const result = await setSessionIntentDomain('session-1', 'domain-1');

    expect(invokeMock).toHaveBeenCalledWith('set_session_intent_domain', {
      sessionId: 'session-1',
      domainId: 'domain-1',
    });
    expect(result.mode).toBe('pinned');
  });
});
