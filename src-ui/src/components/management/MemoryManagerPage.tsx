import { useEffect, useState } from 'react';
import {
  Brain,
  Plus,
  Trash2,
  Loader2,
  Search,
  X,
  BookOpen,
  User,
  Star,
  Folder,
  MessageSquare,
  Edit3,
  Save,
  Check,
  AlertTriangle,
  Sparkles,
} from 'lucide-react';
import { useStore } from '../../store';
import { ManagerPageLayout } from '../common/ManagerPageLayout';
import type { MemoryInfo, CreateMemoryParams, UpdateMemoryParams } from '../../types';

const MEMORY_TYPE_CONFIG: Record<string, { label: string; icon: React.ReactNode; color: string }> = {
  fact: {
    label: '事实',
    icon: <BookOpen size={14} />,
    color: 'bg-blue-100 text-blue-600 dark:bg-blue-900/40 dark:text-blue-400',
  },
  preference: {
    label: '偏好',
    icon: <Star size={14} />,
    color: 'bg-amber-100 text-amber-600 dark:bg-amber-900/40 dark:text-amber-400',
  },
  project_context: {
    label: '项目',
    icon: <Folder size={14} />,
    color: 'bg-emerald-100 text-emerald-600 dark:bg-emerald-900/40 dark:text-emerald-400',
  },
  user_info: {
    label: '用户',
    icon: <User size={14} />,
    color: 'bg-purple-100 text-purple-600 dark:bg-purple-900/40 dark:text-purple-400',
  },
  conversation_summary: {
    label: '对话摘要',
    icon: <MessageSquare size={14} />,
    color: 'bg-rose-100 text-rose-600 dark:bg-rose-900/40 dark:text-rose-400',
  },
};

const MEMORY_TYPES = [
  { value: '', label: '全部类型' },
  { value: 'fact', label: '事实' },
  { value: 'preference', label: '偏好' },
  { value: 'project_context', label: '项目上下文' },
  { value: 'user_info', label: '用户信息' },
  { value: 'conversation_summary', label: '对话摘要' },
];

function MemoryCard({
  memory,
  onEdit,
  onDelete,
}: {
  memory: MemoryInfo;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const config = MEMORY_TYPE_CONFIG[memory.memory_type] ?? MEMORY_TYPE_CONFIG.fact;
  const [confirmDelete, setConfirmDelete] = useState(false);

  return (
    <div className="group bg-white dark:bg-gray-800/80 rounded-xl border border-gray-100 dark:border-gray-700/60 hover:border-purple-200 dark:hover:border-purple-700/50 hover:shadow-md transition-all duration-200">
      <div className="p-4">
        {/* Header */}
        <div className="flex items-start gap-3 mb-2">
          <div className="w-9 h-9 rounded-lg bg-purple-50 dark:bg-purple-900/30 flex items-center justify-center flex-shrink-0 text-purple-600 dark:text-purple-400">
            <Brain size={18} />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium ${config.color}`}>
                {config.icon}
                {config.label}
              </span>
              {memory.scope !== 'global' && (
                <span className="text-[11px] text-gray-400 dark:text-gray-500 bg-gray-100 dark:bg-gray-700/60 px-1.5 py-0.5 rounded">
                  {memory.scope}
                </span>
              )}
              {memory.tags && memory.tags.length > 0 && (
                <div className="flex gap-1 flex-wrap">
                  {memory.tags.slice(0, 3).map((tag) => (
                    <span key={tag} className="text-[11px] text-gray-400 dark:text-gray-500 bg-gray-100 dark:bg-gray-700/60 px-1.5 py-0.5 rounded">
                      #{tag}
                    </span>
                  ))}
                </div>
              )}
            </div>
          </div>
          {/* Relevance indicator — powered by Rig semantic search when available */}
          <div className="flex-shrink-0 flex items-center gap-1.5">
            <div className="w-16 h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
              <div
                className="h-full rounded-full bg-gradient-to-r from-purple-400 to-indigo-500 transition-all"
                style={{ width: `${Math.round(memory.relevance * 100)}%` }}
              />
            </div>
            <span className="text-[11px] text-gray-400 w-7 text-right font-mono">{Math.round(memory.relevance * 100)}%</span>
          </div>
        </div>

        {/* Content */}
        <p className="text-sm text-gray-700 dark:text-gray-300 line-clamp-3 whitespace-pre-wrap ml-[48px]">
          {memory.content}
        </p>

        {/* Footer */}
        <div className="flex items-center justify-between mt-3 ml-[48px]">
          <div className="text-[11px] text-gray-400 dark:text-gray-500">
            访问 {memory.access_count} 次 · {new Date(memory.last_accessed_at).toLocaleDateString()}
          </div>
          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
            <button
              onClick={onEdit}
              className="p-1.5 rounded-md text-gray-400 hover:text-purple-600 hover:bg-purple-50 dark:hover:bg-purple-900/30 transition-all"
              title="编辑"
            >
              <Edit3 size={14} />
            </button>
            {confirmDelete ? (
              <div className="flex items-center gap-1">
                <button
                  onClick={() => { onDelete(); setConfirmDelete(false); }}
                  className="p-1.5 rounded-md text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 transition-all"
                  title="确认删除"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setConfirmDelete(false)}
                  className="p-1.5 rounded-md text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 transition-all"
                  title="取消"
                >
                  <X size={14} />
                </button>
              </div>
            ) : (
              <button
                onClick={() => setConfirmDelete(true)}
                className="p-1.5 rounded-md text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 transition-all"
                title="删除"
              >
                <Trash2 size={14} />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function CreateMemoryDialog({ onClose }: { onClose: () => void }) {
  const { createMemory } = useStore();
  const [content, setContent] = useState('');
  const [memoryType, setMemoryType] = useState<string>('fact');
  const [tags, setTags] = useState('');
  const [relevance, setRelevance] = useState(100);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!content.trim()) {
      setError('记忆内容不能为空');
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const params: CreateMemoryParams = {
        content: content.trim(),
        memory_type: memoryType,
        relevance: relevance / 100,
        tags: tags.trim() ? tags.split(',').map((t) => t.trim()).filter(Boolean) : undefined,
      };
      await createMemory(params);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-gray-200 dark:border-gray-700 w-full max-w-lg mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <Brain size={18} className="text-purple-600" />
            <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">新建记忆</h2>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700 transition-all"
          >
            <X size={16} />
          </button>
        </div>

        {/* Form */}
        <div className="p-5 space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">内容 *</label>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              placeholder="Agent 应该记住什么信息？"
              rows={4}
              className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent resize-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">类型</label>
              <select
                value={memoryType}
                onChange={(e) => setMemoryType(e.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-purple-500"
              >
                {MEMORY_TYPES.filter((t) => t.value).map((t) => (
                  <option key={t.value} value={t.value}>{t.label}</option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">相关度</label>
              <div className="flex items-center gap-2">
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={relevance}
                  onChange={(e) => setRelevance(Number(e.target.value))}
                  className="flex-1 accent-purple-500"
                />
                <span className="text-sm text-gray-500 w-8">{relevance}%</span>
              </div>
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">标签（逗号分隔）</label>
            <input
              value={tags}
              onChange={(e) => setTags(e.target.value)}
              placeholder="rust, react, project-x"
              className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent"
            />
          </div>

          {error && (
            <div className="flex items-center gap-2 text-sm text-red-500 bg-red-50 dark:bg-red-900/20 px-3 py-2 rounded-lg">
              <AlertTriangle size={14} />
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-2 px-5 py-4 border-t border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-all"
          >
            取消
          </button>
          <button
            onClick={handleSubmit}
            disabled={submitting}
            className="px-4 py-2 text-sm font-medium text-white bg-purple-600 hover:bg-purple-700 disabled:bg-purple-400 rounded-lg transition-all flex items-center gap-2"
          >
            {submitting && <Loader2 size={14} className="animate-spin" />}
            <Save size={14} />
            保存记忆
          </button>
        </div>
      </div>
    </div>
  );
}

function EditMemoryDialog({
  memory,
  onClose,
}: {
  memory: MemoryInfo;
  onClose: () => void;
}) {
  const { updateMemory } = useStore();
  const [content, setContent] = useState(memory.content);
  const [memoryType, setMemoryType] = useState<string>(memory.memory_type);
  const [tags, setTags] = useState(memory.tags?.join(', ') ?? '');
  const [relevance, setRelevance] = useState(Math.round(memory.relevance * 100));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!content.trim()) {
      setError('记忆内容不能为空');
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const params: UpdateMemoryParams = {
        content: content.trim(),
        memory_type: memoryType,
        relevance: relevance / 100,
        tags: tags.trim() ? tags.split(',').map((t) => t.trim()).filter(Boolean) : [],
      };
      await updateMemory(memory.id, params);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl border border-gray-200 dark:border-gray-700 w-full max-w-lg mx-4 overflow-hidden">
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <Edit3 size={18} className="text-purple-600" />
            <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">编辑记忆</h2>
          </div>
          <button onClick={onClose} className="p-1.5 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700 transition-all">
            <X size={16} />
          </button>
        </div>

        <div className="p-5 space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">内容 *</label>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              rows={4}
              className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent resize-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">类型</label>
              <select
                value={memoryType}
                onChange={(e) => setMemoryType(e.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-purple-500"
              >
                {MEMORY_TYPES.filter((t) => t.value).map((t) => (
                  <option key={t.value} value={t.value}>{t.label}</option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">相关度</label>
              <div className="flex items-center gap-2">
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={relevance}
                  onChange={(e) => setRelevance(Number(e.target.value))}
                  className="flex-1 accent-purple-500"
                />
                <span className="text-sm text-gray-500 w-8">{relevance}%</span>
              </div>
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">标签（逗号分隔）</label>
            <input
              value={tags}
              onChange={(e) => setTags(e.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-purple-500"
            />
          </div>

          {error && (
            <div className="flex items-center gap-2 text-sm text-red-500 bg-red-50 dark:bg-red-900/20 px-3 py-2 rounded-lg">
              <AlertTriangle size={14} />
              {error}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 px-5 py-4 border-t border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50">
          <button onClick={onClose} className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-all">
            取消
          </button>
          <button
            onClick={handleSubmit}
            disabled={submitting}
            className="px-4 py-2 text-sm font-medium text-white bg-purple-600 hover:bg-purple-700 disabled:bg-purple-400 rounded-lg transition-all flex items-center gap-2"
          >
            {submitting && <Loader2 size={14} className="animate-spin" />}
            <Save size={14} />
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

export function MemoryManagerPage() {
  const {
    memories, memoryLoading, memoryError, memorySearchQuery, memoryFilterType,
    fetchMemories, searchMemories, deleteMemory, setMemorySearchQuery, setMemoryFilterType, clearMemoryError,
  } = useStore();

  const [showCreate, setShowCreate] = useState(false);
  const [editingMemory, setEditingMemory] = useState<MemoryInfo | null>(null);
  const [searchInput, setSearchInput] = useState('');

  useEffect(() => {
    fetchMemories();
  }, []);

  const handleSearch = () => {
    if (searchInput.trim()) {
      searchMemories(searchInput.trim(), memoryFilterType || undefined);
    } else {
      fetchMemories();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleSearch();
    }
  };

  const filteredMemories = memorySearchQuery
    ? memories
    : memoryFilterType
      ? memories.filter((m) => m.memory_type === memoryFilterType)
      : memories;

  return (
    <ManagerPageLayout
      title="记忆系统"
      icon={<Brain size={20} />}
      subtitle="管理 Agent 的长期记忆 — Agent 会在对话中自动参考相关记忆"
      headerActions={
        <button
          onClick={() => setShowCreate(true)}
          className="px-4 py-2 text-sm font-medium text-white bg-purple-600 hover:bg-purple-700 rounded-lg transition-all flex items-center gap-2 shadow-sm"
        >
          <Plus size={16} />
          新建记忆
        </button>
      }
    >
      {/* Search & Filter Bar */}
      <div className="flex items-center gap-3 mb-4">
        <div className="relative flex-1 max-w-md">
          <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
          <input
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="搜索记忆内容..."
            className="w-full pl-9 pr-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent"
          />
          {/* Semantic search indicator */}
          {memorySearchQuery && (
            <div className="absolute right-3 top-1/2 -translate-y-1/2 flex items-center gap-1.5">
              <span className="flex items-center gap-1 text-[11px] font-medium text-purple-500 dark:text-purple-400 bg-purple-50 dark:bg-purple-900/30 px-2 py-0.5 rounded-full">
                <Sparkles size={11} />
                语义
              </span>
            </div>
          )}
        </div>
        <select
          value={memoryFilterType}
          onChange={(e) => {
            setMemoryFilterType(e.target.value);
            if (!searchInput.trim()) {
              // Filter will re-render via filteredMemories
            }
          }}
          className="px-3 py-2 rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 text-sm text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-purple-500"
        >
          {MEMORY_TYPES.map((t) => (
            <option key={t.value} value={t.value}>{t.label}</option>
          ))}
        </select>
        {memorySearchQuery && (
          <button
            onClick={() => {
              setSearchInput('');
              setMemorySearchQuery('');
              fetchMemories();
            }}
            className="px-3 py-2 text-sm text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 bg-gray-100 dark:bg-gray-700 rounded-lg transition-all flex items-center gap-1"
          >
            <X size={14} />
            清除
          </button>
        )}
        <button
          onClick={fetchMemories}
          className="p-2 rounded-lg text-gray-400 hover:text-purple-600 hover:bg-purple-50 dark:hover:bg-purple-900/30 transition-all"
          title="刷新"
        >
          <Loader2 size={16} className={memoryLoading ? 'animate-spin text-purple-500' : ''} />
        </button>
      </div>

      {/* Status summary */}
      <div className="flex items-center gap-4 mb-4 text-xs text-gray-500 dark:text-gray-400">
        <span>共 {filteredMemories.length} 条记忆</span>
        {memories.filter((m) => m.memory_type === 'preference').length > 0 && (
          <span className="flex items-center gap-1">
            <Star size={12} /> {memories.filter((m) => m.memory_type === 'preference').length} 偏好
          </span>
        )}
        {memories.filter((m) => m.memory_type === 'user_info').length > 0 && (
          <span className="flex items-center gap-1">
            <User size={12} /> {memories.filter((m) => m.memory_type === 'user_info').length} 用户信息
          </span>
        )}
        {memories.filter((m) => m.memory_type === 'project_context').length > 0 && (
          <span className="flex items-center gap-1">
            <Folder size={12} /> {memories.filter((m) => m.memory_type === 'project_context').length} 项目
          </span>
        )}
      </div>

      {/* Error */}
      {memoryError && (
        <div className="flex items-center gap-2 text-sm text-red-500 bg-red-50 dark:bg-red-900/20 px-4 py-3 rounded-xl mb-4">
          <AlertTriangle size={16} />
          {memoryError}
          <button onClick={clearMemoryError} className="ml-auto p-1 hover:bg-red-100 dark:hover:bg-red-900/40 rounded">
            <X size={14} />
          </button>
        </div>
      )}

      {/* Memory Grid */}
      {memoryLoading && memories.length === 0 ? (
        <div className="flex items-center justify-center py-20">
          <Loader2 size={24} className="animate-spin text-purple-500" />
        </div>
      ) : filteredMemories.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-gray-400">
          <Brain size={40} className="mb-3 opacity-30" />
          <p className="text-sm">还没有记忆</p>
          <p className="text-xs mt-1">点击"新建记忆"添加 Agent 应该记住的信息</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
          {filteredMemories.map((memory) => (
            <MemoryCard
              key={memory.id}
              memory={memory}
              onEdit={() => setEditingMemory(memory)}
              onDelete={() => deleteMemory(memory.id)}
            />
          ))}
        </div>
      )}

      {/* Dialogs */}
      {showCreate && <CreateMemoryDialog onClose={() => setShowCreate(false)} />}
      {editingMemory && (
        <EditMemoryDialog memory={editingMemory} onClose={() => setEditingMemory(null)} />
      )}
    </ManagerPageLayout>
  );
}
