import { useState, useMemo } from 'react';
import { AlertTriangle, Archive, ChevronRight, X, Check } from 'lucide-react';
import type { LegacySkill, AgentOption, LegacyImportResult } from '../../types';

interface LegacySkillsBannerProps {
  skills: LegacySkill[];
  agents: AgentOption[];
  onImport: (paths: string[], agents: string[], copy: boolean) => Promise<unknown>;
}

export function LegacySkillsBanner({ skills, agents, onImport }: LegacySkillsBannerProps) {
  const [expanded, setExpanded] = useState(false);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(new Set());
  const [copy, setCopy] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importResult, setImportResult] = useState<LegacyImportResult | null>(null);

  const importable = useMemo(() => skills.filter((s) => s.importable), [skills]);
  const incompatible = useMemo(() => skills.filter((s) => !s.importable), [skills]);

  if (skills.length === 0) return null;

  const togglePath = (path: string) => {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const toggleAgent = (id: string) => {
    setSelectedAgents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleSelectAll = () => {
    setSelectedPaths(new Set(importable.map((s) => s.path)));
  };

  const handleImport = async () => {
    const paths = Array.from(selectedPaths);
    const agentIds = Array.from(selectedAgents);
    if (paths.length === 0 || agentIds.length === 0) return;
    setImporting(true);
    setImportResult(null);
    try {
      const result = (await onImport(paths, agentIds, copy)) as LegacyImportResult;
      setImportResult(result);
      if (!result.failed.length) {
        setExpanded(false);
        setSelectedPaths(new Set());
        setSelectedAgents(new Set());
        setCopy(false);
      }
    } catch (err) {
      setImportResult({ imported: [], failed: paths.map((path) => ({ path, error: String(err) })) });
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="mb-4 rounded-xl border border-amber-200 dark:border-amber-800/50 bg-amber-50/70 dark:bg-amber-900/20 overflow-hidden">
      <div className="px-4 py-3 flex items-center gap-3">
        <div className="w-8 h-8 rounded-lg bg-amber-100 dark:bg-amber-900/40 flex items-center justify-center flex-shrink-0">
          <Archive size={16} className="text-amber-600 dark:text-amber-400" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-amber-900 dark:text-amber-200">
            发现旧版技能数据
          </p>
          <p className="text-xs text-amber-700/80 dark:text-amber-300/70 mt-0.5">
            可导入 {importable.length} 个，不兼容 {incompatible.length} 个。原目录不会被删除。
          </p>
        </div>
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1 px-3 py-1.5 text-xs font-medium text-amber-800 dark:text-amber-200 bg-amber-100 dark:bg-amber-900/40 hover:bg-amber-200 dark:hover:bg-amber-900/60 rounded-lg transition-colors"
        >
          {expanded ? '收起' : '导入'}
          <ChevronRight
            size={14}
            className={`transition-transform ${expanded ? 'rotate-90' : ''}`}
          />
        </button>
      </div>

      {expanded && (
        <div className="px-4 pb-4 border-t border-amber-200 dark:border-amber-800/40">
          <div className="mt-3 space-y-3">
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-xs font-medium text-amber-900 dark:text-amber-200">
                  选择要导入的技能
                </label>
                <button
                  onClick={handleSelectAll}
                  className="text-[11px] text-amber-700 dark:text-amber-300 hover:underline"
                >
                  全选可导入项
                </button>
              </div>
              <div className="max-h-40 overflow-y-auto space-y-1.5 rounded-lg border border-amber-200 dark:border-amber-800/40 bg-white dark:bg-gray-800 p-2">
                {skills.map((skill) => (
                  <label
                    key={skill.path}
                    className={`flex items-start gap-2 p-2 rounded-md ${
                      skill.importable
                        ? 'hover:bg-amber-50 dark:hover:bg-amber-900/20 cursor-pointer'
                        : 'opacity-60 cursor-not-allowed'
                    }`}
                  >
                    <input
                      type="checkbox"
                      disabled={!skill.importable}
                      checked={selectedPaths.has(skill.path)}
                      onChange={() => togglePath(skill.path)}
                      className="mt-0.5 rounded border-amber-300 text-amber-600 focus:ring-amber-500"
                    />
                    <div className="flex-1 min-w-0">
                      <p className="text-xs font-medium text-gray-900 dark:text-gray-100 truncate">
                        {skill.name}
                      </p>
                      <p className="text-[10px] text-gray-500 dark:text-gray-400 truncate">
                        {skill.path}
                      </p>
                      {!skill.importable && skill.reason && (
                        <p className="text-[10px] text-red-500 mt-0.5 flex items-center gap-1">
                          <AlertTriangle size={10} />
                          {skill.reason}
                        </p>
                      )}
                    </div>
                  </label>
                ))}
              </div>
            </div>

            <div>
              <label className="text-xs font-medium text-amber-900 dark:text-amber-200 mb-1.5 block">
                目标 Agent
              </label>
              <div className="flex flex-wrap gap-2">
                {agents.map((agent) => (
                  <button
                    key={agent.id}
                    onClick={() => toggleAgent(agent.id)}
                    className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs transition-colors border ${
                      selectedAgents.has(agent.id)
                        ? 'bg-amber-100 dark:bg-amber-900/50 border-amber-300 dark:border-amber-700 text-amber-800 dark:text-amber-200'
                        : 'bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300 hover:border-amber-300'
                    }`}
                  >
                    {selectedAgents.has(agent.id) && <Check size={10} />}
                    {agent.name}
                  </button>
                ))}
                {agents.length === 0 && (
                  <p className="text-xs text-gray-400">暂无可选 agent</p>
                )}
              </div>
            </div>

            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={copy}
                onChange={(e) => setCopy(e.target.checked)}
                className="rounded border-amber-300 text-amber-600 focus:ring-amber-500"
              />
              <span className="text-xs text-amber-900 dark:text-amber-200">复制到目标目录（不移动原文件）</span>
            </label>

            {importResult && (
              <div
                className={`p-2.5 rounded-lg text-xs ${
                  importResult.failed.length > 0
                    ? 'bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800/50 text-red-700 dark:text-red-400'
                    : 'bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800/50 text-green-700 dark:text-green-400'
                }`}
              >
                导入完成：成功 {importResult.imported.length} 个
                {importResult.failed.length > 0 && `，失败 ${importResult.failed.length} 个`}
                {importResult.failed.length > 0 && (
                  <ul className="mt-1 space-y-0.5">
                    {importResult.failed.map((f) => (
                      <li key={f.path} className="truncate">
                        {f.path}: {f.error}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}

            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={handleImport}
                disabled={selectedPaths.size === 0 || selectedAgents.size === 0 || importing}
                className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-amber-600 to-orange-600 hover:from-amber-700 hover:to-orange-700 disabled:opacity-50 text-white text-xs font-medium rounded-lg transition-all"
              >
                {importing ? (
                  <span className="w-3.5 h-3.5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                ) : (
                  <Archive size={13} />
                )}
                导入 {selectedPaths.size > 0 && `(${selectedPaths.size})`}
              </button>
              <button
                onClick={() => setExpanded(false)}
                className="px-4 py-2 bg-white dark:bg-gray-800 border border-amber-200 dark:border-amber-800/40 text-amber-800 dark:text-amber-200 text-xs font-medium rounded-lg hover:bg-amber-50 dark:hover:bg-amber-900/20 transition-colors"
              >
                <X size={13} className="inline mr-1" />
                取消
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
