import { useEffect, useState } from 'react';
import { X, Loader2, Trash2, Check, AlertTriangle, ExternalLink, RotateCcw } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useStore } from '../../store';
import type { InstalledSkill, SkillScope } from '../../types';
import { CodeBlock } from '../common/CodeBlock';
import { agentIdsFromNames } from './skillUtils';

interface SkillDetailPanelProps {
  skill: InstalledSkill;
  onClose: () => void;
}

export function SkillDetailPanel({ skill, onClose }: SkillDetailPanelProps) {
  const {
    skillContent,
    skillLoading,
    busy,
    skillError,
    fetchContent,
    removeSkill,
    installSkill,
    projectPath,
    agents,
    clearSkillContent,
    clearSkillError,
  } = useStore();

  const [confirmRemove, setConfirmRemove] = useState(false);
  const [localBusy, setLocalBusy] = useState(false);

  const scope: SkillScope = skill.scope === 'project' ? 'project' : 'global';
  const isMissing = skill.status === 'missing';

  useEffect(() => {
    clearSkillError();
    if (!isMissing) {
      fetchContent(skill.name, scope);
    }
    return () => {
      clearSkillContent();
    };
  }, [skill.name, scope, isMissing, fetchContent, clearSkillContent, clearSkillError]);

  const handleRemove = async () => {
    setLocalBusy(true);
    try {
      await removeSkill(skill.name);
      onClose();
    } catch {
      // error handled in store
    } finally {
      setLocalBusy(false);
    }
  };

  const handleReinstall = async () => {
    const source = skill.source_url || skill.source || '';
    if (!source) return;
    setLocalBusy(true);
    try {
      await installSkill({
        source,
        skills: [skill.name],
        agents: agentIdsFromNames(skill.agents, agents),
        scope,
        projectPath,
        copy: false,
      });
      onClose();
    } catch {
      // error handled in store
    } finally {
      setLocalBusy(false);
    }
  };

  const loading = skillLoading || busy === 'content' || busy === 'remove' || busy === 'install' || localBusy;

  const sourceLink = skill.source
    ? `https://skills.sh/${skill.source}`
    : skill.source_url || null;

  return (
    <div className="fixed inset-y-0 right-0 w-full max-w-lg bg-white dark:bg-gray-800 shadow-2xl z-50 flex flex-col border-l border-gray-200 dark:border-gray-700 animate-in slide-in-from-right duration-200">
      <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-gray-700">
        <div className="min-w-0">
          <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100 truncate">
            {skill.name}
          </h2>
          {skill.version && (
            <p className="text-xs text-gray-400 dark:text-gray-500 mt-0.5">
              版本 {skill.version}
            </p>
          )}
        </div>
        <button
          onClick={onClose}
          className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
        >
          <X size={18} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-5 space-y-4">
        {isMissing && (
          <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg flex items-start gap-2 text-sm text-red-700 dark:text-red-400">
            <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />
            <span>该技能记录存在但本地文件缺失，建议重新安装或移除。</span>
          </div>
        )}

        <div>
          <p className="text-sm text-gray-700 dark:text-gray-300 leading-relaxed">
            {skill.description}
          </p>
        </div>

        <div className="flex flex-wrap gap-1.5">
          {skill.agents.map((agent) => (
            <span
              key={agent}
              className="text-[11px] px-2 py-0.5 rounded-full bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 border border-purple-100 dark:border-purple-800"
            >
              {agent}
            </span>
          ))}
        </div>

        {sourceLink && (
          <div>
            <a
              href={sourceLink}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-xs text-purple-600 dark:text-purple-400 hover:underline"
            >
              <ExternalLink size={12} />
              {sourceLink}
            </a>
          </div>
        )}

        {skill.path && (
          <div>
            <span className="text-xs text-gray-400 dark:text-gray-500 block mb-0.5">路径</span>
            <p className="text-xs text-gray-600 dark:text-gray-400 break-all font-mono">
              {skill.path}
            </p>
          </div>
        )}

        {skillError && (
          <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
            <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />
            <span>{skillError}</span>
          </div>
        )}

        {!isMissing && (
          <div className="border-t border-gray-100 dark:border-gray-700 pt-4">
            <h3 className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2">
              SKILL.md
            </h3>
            {loading && !skillContent ? (
              <div className="flex items-center justify-center py-12 text-gray-400 dark:text-gray-500">
                <Loader2 size={20} className="animate-spin text-purple-500 mr-2" />
                加载中...
              </div>
            ) : skillContent ? (
              <div className="prose prose-sm dark:prose-invert max-w-none">
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  components={{
                    pre({ children }) {
                      const child = children as React.ReactElement<{
                        className?: string;
                        children?: React.ReactNode;
                      }>;
                      if (child?.type === 'code' && child?.props?.className) {
                        const match = /language-(\w+)/.exec(String(child.props.className));
                        if (match) {
                          return (
                            <CodeBlock
                              language={match[1]}
                              value={String(child.props.children).replace(/\n$/, '')}
                            />
                          );
                        }
                      }
                      return (
                        <pre className="bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg p-4 overflow-x-auto text-sm text-gray-700 dark:text-gray-300">
                          {children}
                        </pre>
                      );
                    },
                    code({ children, ...props }) {
                      return (
                        <code
                          className="bg-gray-100 dark:bg-gray-700 text-pink-600 dark:text-pink-300 px-1.5 py-0.5 rounded text-sm font-mono"
                          {...props}
                        >
                          {children}
                        </code>
                      );
                    },
                  }}
                >
                  {skillContent}
                </ReactMarkdown>
              </div>
            ) : (
              <p className="text-xs text-gray-400 dark:text-gray-500 py-8 text-center">
                暂无文档内容
              </p>
            )}
          </div>
        )}
      </div>

      <div className="p-5 border-t border-gray-100 dark:border-gray-700 flex items-center justify-between">
        {confirmRemove ? (
          <div className="flex items-center gap-2">
            <span className="text-xs text-red-600 dark:text-red-400">确认移除？</span>
            <button
              onClick={handleRemove}
              disabled={loading}
              className="flex items-center gap-1 px-3 py-1.5 bg-red-600 hover:bg-red-700 disabled:opacity-50 text-white rounded-lg text-xs transition-colors"
            >
              {loading && <Loader2 size={11} className="animate-spin" />}
              <Check size={11} />
              确认
            </button>
            <button
              onClick={() => setConfirmRemove(false)}
              disabled={loading}
              className="px-3 py-1.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-xs transition-colors"
            >
              取消
            </button>
          </div>
        ) : (
          <button
            onClick={() => setConfirmRemove(true)}
            className="flex items-center gap-1.5 px-3 py-2 text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg text-sm transition-colors"
          >
            <Trash2 size={14} />
            移除
          </button>
        )}

        {isMissing ? (
          <button
            onClick={handleReinstall}
            disabled={loading || !skill.source_url && !skill.source}
            className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-sm transition-all"
          >
            {loading ? <Loader2 size={14} className="animate-spin" /> : <RotateCcw size={14} />}
            重新安装
          </button>
        ) : null}
      </div>
    </div>
  );
}
