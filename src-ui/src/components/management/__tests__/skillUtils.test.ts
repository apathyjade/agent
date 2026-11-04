import { describe, expect, it } from 'vitest';
import { filterInstalled, isInstalled } from '../skillUtils';
import type { InstalledSkill } from '../../../types';

const rows: InstalledSkill[] = [
  { name: 'find-skills', description: 'Find', path: '/a', scope: 'global', agents: ['OpenCode'], status: 'tracked', has_skill_md: true },
  { name: 'web-design', description: 'UI', path: '/b', scope: 'project', agents: ['Claude Code'], status: 'tracked', has_skill_md: true },
];

describe('skillUtils', () => {
  it('filters by query and agent', () => {
    expect(filterInstalled(rows, 'find', null).map((s) => s.name)).toEqual(['find-skills']);
    expect(filterInstalled(rows, '', 'OpenCode').map((s) => s.name)).toEqual(['find-skills']);
  });
  it('matches installed names case-insensitively', () => {
    expect(isInstalled('Find-Skills', rows)).toBe(true);
    expect(isInstalled('missing', rows)).toBe(false);
  });
});
