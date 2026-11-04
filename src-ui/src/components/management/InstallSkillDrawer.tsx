import { useState, useEffect } from 'react';
import {
  X,
  Search,
  Check,
  Loader2,
  AlertCircle,
  Package,
  GitBranch,
  FolderKanban,
  Copy,
} from 'lucide-react';
import { useStore } from '../../store';
import { previewSkillSource } from '../../api/management';
import type { SkillScope, RepoSkillPreview, AgentOption } from '../../types';

interface InstallSkillDrawerProps {
  open: boolean;
  onClose: () => void;
  initialSource?: string;
}

export function InstallSkillDrawer({ open, onClose, initialSource }: InstallSkillDrawerProps) {
  const {
    agents,
    scope: storeScope,
    projectPath,
    installSkill,
    setScope,
    busy,
    skillError,
    clearSkillError,
  } = useStore();

  const [source, setSource] = useState('');
  const [preview, setPreview] = useState<RepoSkillPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [selectedSkills, setSelectedSkills] = useState<Set<string>>(new Set());
  const [selectedAgents, setSelectedAgents] = useState<Set<string>>(new Set());
  const [customAgent, setCustomAgent] = useState('');
  const [scope, setScopeState] = useState<SkillScope>(storeScope);
  const [copy, setCopy] = useState(false);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    if (open) {
      setScopeState(storeScope);
      if (initialSource) {
        setSource(initialSource);
        // Trigger preview on next tick after state flush
        setTimeout(() => previewSource(initialSource), 0);
      }
    }
  }, [open, storeScope, initialSource]);

  const isBusy = busy === 'install' || installing;

  const reset = () => {
    setSource('');
    setPreview(null);
    setPreviewError(null);
    setSelectedSkills(new Set());
    setSelectedAgents(new Set());
    setCustomAgent('');
    setCopy(false);
    clearSkillError();
  };

  const handleClose = () => {
    reset();
    onClose();
  };

  const previewSource = async (value?: string) => {
    const s = (value ?? source).trim();
    if (!s) return;
    setPreviewLoading(true);
    setPreviewError(null);
    setPreview(null);
    setSelectedSkills(new Set());
    try {
      const result = await previewSkillSource(s);
      setPreview(result);
    } catch (err) {
      setPreviewError(String(err));
    } finally {
      setPreviewLoading(false);
    }
  };

  const toggleSkill = (name: string) => {
    setSelectedSkills((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
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

  const addCustomAgent = () => {
    const id = customAgent.trim();
    if (!id) return;
    setSelectedAgents((prev) => new Set(prev).add(id));
    setCustomAgent('');
  };

  const handleInstall = async () => {
    const s = source.trim();
    if (!s) return;
    const agentIds = Array.from(selectedAgents);
    if (agentIds.length === 0) return;
    const skills = preview && preview.skills.length > 0 ? Array.from(selectedSkills) : [];
    setInstalling(true);
    try {
      await installSkill({
        source: s,
        skills,
        agents: agentIds,
        scope,
        projectPath,
        copy,
      });
      if (scope !== storeScope) {
        await setScope(scope);
      }
      handleClose();
    } catch {
      // error handled in store
    } finally {
      setInstalling(false);
    }
  };

  const projectDisabled = scope === 'project' && !projectPath;
  const canInstall = Boolean(
    source.trim() &&
      selectedAgents.size > 0 &&
      !isBusy &&
      !projectDisabled &&
      (!preview || preview.skills.length === 0 || selectedSkills.size > 0)
  );

  if (!open) return null;

  return (
    <div className="fixed inset-y-0 right-0 w-full max-w-md bg-white dark:bg-gray-800 shadow-2xl z-50 flex flex-col border-l border-gray-200 dark:border-gray-700 animate-in slide-in-from-right duration-200">
      <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-gray-700">
        <div className="flex items-center gap-2">
          <div className="w-8 h-8 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center">
            <Package size={16} className="text-purple-600 dark:text-purple-400" />
          </div>
          <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">安装技能</h2>
        </div>
        <button
          onClick={handleClose}
          className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
        >
          <X size={18} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-5 space-y-5">
        {/* Source */}
        <div>
          <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">
            来源（GitHub owner/repo 或完整 URL）
          </label>
          <div className="flex gap-2">
            <input
              type="text"
              value={source}
              onChange={(e) => {
                setSource(e.target.value);
                setPreviewError(null);
                clearSkillError();
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') previewSource();
              }}
              placeholder="例如：vercel-labs/agent-skills"
              className="flex-1 bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
            />
            <button
              onClick={() => previewSource()}
              disabled={!source.trim() || previewLoading || isBusy}
              className="flex items-center gap-1.5 px-3 py-2 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 text-gray-700 dark:text-gray-200 rounded-lg text-sm transition-colors disabled:opacity-50"
            >
              {previewLoading ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <Search size={13} />
              )}
              预览
            </button>
          </div>
        </div>

        {/* Preview skills */}
        {preview && (
          <div>
            <div className="flex items-center justify-between mb-1.5">
              <label className="text-xs font-medium text-gray-500 dark:text-gray-400">
                可选技能
              </label>
              {preview.skills.length > 0 && (
                <button
                  onClick={() =>
                    setSelectedSkills(
                      selectedSkills.size === preview.skills.length
                        ? new Set()
                        : new Set(preview.skills.map((s) => s.name))
                    )
                  }
                  className="text-[11px] text-purple-600 dark:text-purple-400 hover:underline"
                >
                  {selectedSkills.size === preview.skills.length ? '取消全选' : '全选'}
                </button>
              )}
            </div>
            {preview.skills.length === 0 ? (
              <div className="p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-lg text-xs text-amber-700 dark:text-amber-400">
                未能解析出技能列表。继续安装将包含该仓库中的全部技能。
              </div>
            ) : (
              <div className="space-y-1.5 max-h-56 overflow-y-auto">
                {preview.skills.map((skill) => (
                  <label
                    key={skill.name}
                    className="flex items-start gap-2 p-2.5 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 hover:border-purple-300 dark:hover:border-purple-700 cursor-pointer transition-colors"
                  >
                    <input
                      type="checkbox"
                      checked={selectedSkills.has(skill.name)}
                      onChange={() => toggleSkill(skill.name)}
                      className="mt-0.5 rounded border-gray-300 text-purple-600 focus:ring-purple-500"
                    />
                    <div className="flex-1 min-w-0">
                      <p className="text-xs font-medium text-gray-900 dark:text-gray-100">
                        {skill.name}
                      </p>
                      {skill.description && (
                        <p className="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5 line-clamp-2">
                          {skill.description}
                        </p>
                      )}
                    </div>
                  </label>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Agents */}
        <div>
          <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">
            目标 Agent
          </label>
          <div className="flex flex-wrap gap-2 mb-2">
            {agents.map((agent: AgentOption) => (
              <button
                key={agent.id}
                onClick={() => toggleAgent(agent.id)}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs transition-colors border ${
                  selectedAgents.has(agent.id)
                    ? 'bg-purple-100 dark:bg-purple-900/50 border-purple-300 dark:border-purple-700 text-purple-800 dark:text-purple-200'
                    : 'bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300 hover:border-purple-300'
                }`}
              >
                {selectedAgents.has(agent.id) && <Check size={10} />}
                {agent.name}
              </button>
            ))}
          </div>
          <div className="flex gap-2">
            <input
              type="text"
              value={customAgent}
              onChange={(e) => setCustomAgent(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') addCustomAgent();
              }}
              placeholder="自定义 agent id"
              className="flex-1 bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-1.5 text-xs focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
            />
            <button
              onClick={addCustomAgent}
              disabled={!customAgent.trim()}
              className="px-3 py-1.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-xs transition-colors disabled:opacity-50"
            >
              添加
            </button>
          </div>
          {Array.from(selectedAgents).length > 0 && (
            <div className="flex flex-wrap gap-1.5 mt-2">
              {Array.from(selectedAgents).map((id) => (
                <span
                  key={id}
                  className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 border border-purple-200 dark:border-purple-800"
                >
                  {id}
                  <button
                    onClick={() => toggleAgent(id)}
                    className="hover:text-purple-900 dark:hover:text-purple-100"
                  >
                    <X size={10} />
                  </button>
                </span>
              ))}
            </div>
          )}
        </div>

        {/* Scope */}
        <div>
          <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">
            安装范围
          </label>
          <div className="flex items-center gap-2 p-1 bg-gray-100 dark:bg-gray-800 rounded-lg">
            <button
              onClick={() => setScopeState('global')}
              className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                scope === 'global'
                  ? 'bg-white dark:bg-gray-700 text-purple-700 dark:text-purple-300 shadow-sm'
                  : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
              }`}
            >
              <GitBranch size={13} />
              全局
            </button>
            <button
              onClick={() => setScopeState('project')}
              className={`flex-1 flex items-center justify-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                scope === 'project'
                  ? 'bg-white dark:bg-gray-700 text-purple-700 dark:text-purple-300 shadow-sm'
                  : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
              }`}
            >
              <FolderKanban size={13} />
              项目
            </button>
          </div>
          {scope === 'project' && !projectPath && (
            <p className="mt-1.5 text-[11px] text-amber-600 dark:text-amber-400 flex items-center gap-1">
              <AlertCircle size={11} />
              请先选择一个项目
            </p>
          )}
        </div>

        {/* Copy toggle */}
        <label className="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={copy}
            onChange={(e) => setCopy(e.target.checked)}
            className="rounded border-gray-300 text-purple-600 focus:ring-purple-500"
          />
          <span className="text-xs text-gray-600 dark:text-gray-300 flex items-center gap-1">
            <Copy size={12} />
            复制到目标目录
          </span>
        </label>

        {(previewError || skillError) && (
          <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span>{previewError || skillError}</span>
          </div>
        )}
      </div>

      <div className="p-5 border-t border-gray-100 dark:border-gray-700 flex items-center gap-2">
        <button
          onClick={handleInstall}
          disabled={!canInstall}
          className="flex-1 flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2.5 rounded-lg text-sm transition-all font-medium"
        >
          {isBusy ? (
            <>
              <Loader2 size={14} className="animate-spin" />
              安装中...
            </>
          ) : (
            <>
              <Package size={14} />
              安装
            </>
          )}
        </button>
        <button
          onClick={handleClose}
          className="px-4 py-2.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
        >
          取消
        </button>
      </div>
    </div>
  );
}
