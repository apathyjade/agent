import { describe, expect, it } from 'vitest';
import { filterInstalled, isInstalled, agentIdsFromNames } from '../skillUtils';
import type { InstalledSkill, AgentOption } from '../../../types';

const rows: InstalledSkill[] = [
  { name: 'find-skills', description: 'Find', path: '/a', scope: 'global', agents: ['OpenCode'], status: 'tracked', has_skill_md: true },
  { name: 'web-design', description: 'UI', path: '/b', scope: 'project', agents: ['Claude Code'], status: 'tracked', has_skill_md: true },
];

const agentOptions: AgentOption[] = [
  { id: 'opencode', name: 'OpenCode', project_path: '.opencode/skills', global_path: '.opencode/skills' },
  { id: 'claude-code', name: 'Claude Code', project_path: '.claude/skills' },
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
  it('maps agent display names to ids, falling back to lowercased input', () => {
    expect(agentIdsFromNames(['OpenCode', 'Claude Code'], agentOptions)).toEqual(['opencode', 'claude-code']);
    expect(agentIdsFromNames(['OpenCode', 'opencode'], agentOptions)).toEqual(['opencode']);
    expect(agentIdsFromNames(['Unknown Agent'], agentOptions)).toEqual(['unknown agent']);
  });
});

