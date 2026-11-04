import type { InstalledSkill, AgentOption } from '../../types';

export function filterInstalled(
  skills: InstalledSkill[],
  query: string,
  agentId: string | null,
): InstalledSkill[] {
  const q = query.trim().toLowerCase();
  return skills.filter((skill) => {
    if (q) {
      const inName = skill.name.toLowerCase().includes(q);
      const inDesc = (skill.description || '').toLowerCase().includes(q);
      const inAgents = skill.agents?.some((a) => a.toLowerCase().includes(q)) ?? false;
      if (!inName && !inDesc && !inAgents) return false;
    }
    if (agentId) {
      if (!skill.agents?.some((a) => a.toLowerCase() === agentId.toLowerCase())) return false;
    }
    return true;
  });
}

export function isInstalled(marketName: string, installed: InstalledSkill[]): boolean {
  return installed.some((s) => s.name.toLowerCase() === marketName.toLowerCase());
}

export function agentIdsFromNames(names: string[], agents: AgentOption[]): string[] {
  return names
    .map((name) => {
      const lower = name.toLowerCase();
      const match = agents.find(
        (a) => a.name.toLowerCase() === lower || a.id.toLowerCase() === lower
      );
      return match ? match.id : lower;
    })
    .filter((id, index, arr) => arr.indexOf(id) === index);
}
