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
    expect(store.getState().loading).toBe(false);
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
    expect(store.getState().error).toContain('boom');
  });

  it('switches scope and refetches installed skills', async () => {
    const store = testStore();
    await store.getState().setScope('project');
    expect(store.getState().scope).toBe('project');
    expect(api.listInstalledSkills).toHaveBeenCalledWith('project', null);
  });

  it('loads skill content', async () => {
    const store = testStore();
    await store.getState().fetchContent('x', 'global');
    expect(store.getState().skillContent).toBe('# skill');
  });
});
