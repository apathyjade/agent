import { beforeEach, describe, expect, it, vi } from 'vitest';
import { create } from 'zustand';

vi.mock('../../api/tauri', () => ({
  getSkillsEnv: vi.fn().mockResolvedValue({ available: true }),
  listInstalledSkills: vi.fn().mockResolvedValue([
    { name: 'x', description: '', agents: [], status: 'tracked', scope: 'global', has_skill_md: true },
  ]),
  listSkillAgents: vi.fn().mockResolvedValue([]),
  listMarketTopSkills: vi.fn().mockResolvedValue([]),
  searchMarketSkills: vi.fn().mockResolvedValue([]),
  installSkills: vi.fn().mockResolvedValue('ok'),
  removeSkill: vi.fn().mockResolvedValue('ok'),
  getSkillContent: vi.fn().mockResolvedValue('# skill'),
  checkSkillUpdates: vi.fn().mockResolvedValue({ has_updates: false, has_failures: false, output: '' }),
  updateSkills: vi.fn().mockResolvedValue('ok'),
  detectLegacySkills: vi.fn().mockResolvedValue([]),
  importLegacySkills: vi.fn().mockResolvedValue({ imported: [], failed: [] }),
  previewSkillSource: vi.fn().mockResolvedValue({ skills: [], raw: '' }),
}));

import * as api from '../../api/tauri';
import { createSkillSlice, type SkillSlice } from '../skillSlice';

function testStore() {
  return create<SkillSlice>()((...args) => createSkillSlice(...args));
}

describe('skillSlice', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('loads installed skills', async () => {
    const store = testStore();
    await store.getState().fetchInstalled();
    expect(store.getState().installed).toHaveLength(1);
    expect(store.getState().skillLoading).toBe(false);
  });

  it('refreshes after install', async () => {
    const store = testStore();
    await store.getState().installSkill({
      source: 'o/r', skills: ['x'], agents: ['opencode'], scope: 'global', copy: false,
    });
    expect(api.installSkills).toHaveBeenCalled();
    expect(api.listInstalledSkills).toHaveBeenCalled();
  });

  it('records errors without throwing', async () => {
    vi.mocked(api.listInstalledSkills).mockRejectedValueOnce(new Error('boom'));
    const store = testStore();
    await store.getState().fetchInstalled();
    expect(store.getState().skillError).toContain('boom');
  });

  it('switches scope and refetches installed skills when a project is selected', async () => {
    const store = testStore();
    await store.getState().setProjectPath('C:/work/app');
    vi.mocked(api.listInstalledSkills).mockClear();
    await store.getState().setScope('project');
    expect(store.getState().scope).toBe('project');
    expect(api.listInstalledSkills).toHaveBeenCalledWith('project', 'C:/work/app');
  });

  it('treats project scope without a project path as an empty state', async () => {
    const store = testStore();
    await store.getState().fetchInstalled();
    expect(store.getState().installed).toHaveLength(1);
    store.setState({ skillError: 'stale error' });
    vi.mocked(api.listInstalledSkills).mockClear();

    await store.getState().setScope('project');

    expect(store.getState().scope).toBe('project');
    expect(store.getState().installed).toEqual([]);
    expect(store.getState().skillError).toBeNull();
    expect(api.listInstalledSkills).not.toHaveBeenCalled();
  });

  it('loads skill content', async () => {
    const store = testStore();
    await store.getState().fetchContent('x', 'global');
    expect(store.getState().skillContent).toBe('# skill');
  });

  it('rethrows install failures and clears busy', async () => {
    vi.mocked(api.installSkills).mockRejectedValueOnce(new Error('install failed'));
    const store = testStore();
    await expect(
      store.getState().installSkill({
        source: 'o/r', skills: ['x'], agents: ['opencode'], scope: 'global', copy: false,
      }),
    ).rejects.toThrow('install failed');
    expect(store.getState().busy).toBeNull();
    expect(store.getState().skillError).toContain('install failed');
  });

  it('re-lists after a failed install so a partial install stays visible', async () => {
    vi.mocked(api.installSkills).mockRejectedValueOnce(new Error('install failed'));
    const store = testStore();
    await expect(
      store.getState().installSkill({
        source: 'o/r', skills: ['x'], agents: ['opencode'], scope: 'global', copy: false,
      }),
    ).rejects.toThrow('install failed');
    expect(api.listInstalledSkills).toHaveBeenCalled();
    expect(store.getState().skillError).toContain('install failed');
    expect(store.getState().installed).toHaveLength(1);
  });

  it('keeps a post-update refresh error visible', async () => {
    vi.mocked(api.updateSkills).mockResolvedValueOnce('ok');
    vi.mocked(api.listInstalledSkills).mockRejectedValueOnce(new Error('refresh failed'));
    const store = testStore();
    await store.getState().updateAll();
    expect(store.getState().skillError).toContain('refresh failed');
    expect(store.getState().busy).toBeNull();
    expect(store.getState().installed).toHaveLength(0);
  });

  it('returns the import result and refreshes both lists', async () => {
    vi.mocked(api.importLegacySkills).mockResolvedValueOnce({ imported: ['old'], failed: [] });
    vi.mocked(api.detectLegacySkills).mockResolvedValueOnce([
      { name: 'old', path: 'C:/skills/old', importable: true },
    ]);
    const store = testStore();
    const result = await store.getState().importLegacy(['C:/skills/old'], ['opencode'], false);
    expect(result).toEqual({ imported: ['old'], failed: [] });
    expect(api.listInstalledSkills).toHaveBeenCalled();
    expect(api.detectLegacySkills).toHaveBeenCalled();
    expect(store.getState().installed).toHaveLength(1);
    expect(store.getState().legacySkills).toHaveLength(1);
    expect(store.getState().busy).toBeNull();
  });
});
