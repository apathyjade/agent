import { useEffect, useState } from 'react';
import {
  Users,
  Plus,
  Trash2,
  Loader2,
  Edit3,
  Save,
  X,
  AlertTriangle,
  User,
  Star,
  Terminal,
  Shield,
  Link2,
} from 'lucide-react';
import * as api from '../api/tauri';
import { useStore } from '../store';
import { ManagerPageLayout } from './ManagerPageLayout';
import type { PersonaInfo } from '../types';

const PERSONA_ICONS: Record<string, { icon: React.ReactNode; color: string }> = {
  Dev: { icon: <Terminal size={20} />, color: 'text-blue-500' },
  通用开发者: { icon: <User size={20} />, color: 'text-blue-500' },
  Arch: { icon: <Star size={20} />, color: 'text-amber-500' },
  系统架构师: { icon: <Star size={20} />, color: 'text-amber-500' },
  QA: { icon: <Shield size={20} />, color: 'text-emerald-500' },
  质量与安全工程师: { icon: <Shield size={20} />, color: 'text-emerald-500' },
};

function getPersonaIcon(p: PersonaInfo) {
  if (PERSONA_ICONS[p.name]) return PERSONA_ICONS[p.name];
  if (PERSONA_ICONS[p.title]) return PERSONA_ICONS[p.title];
  return { icon: <Users size={20} />, color: 'text-purple-500' };
}

const RESPONSE_STYLE_LABELS: Record<string, string> = {
  concise: '简洁',
  verbose: '详细',
  academic: '学术',
};

function CreatePersonaDialog({ open, onClose, onCreated }: {
  open: boolean;
  onClose: () => void;
  onCreated: (p: PersonaInfo) => void;
}) {
  const fetchPersonas = useStore((state) => state.fetchPersonas);
  const [name, setName] = useState('');
  const [title, setTitle] = useState('');
  const [emoji, setEmoji] = useState('🧑‍💻');
  const [description, setDescription] = useState('');
  const [systemPrompt, setSystemPrompt] = useState('');
  const [temperature, setTemperature] = useState(0.3);
  const [responseStyle, setResponseStyle] = useState('concise');
  const [isDefault, setIsDefault] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  const reset = () => {
    setName('');
    setTitle('');
    setEmoji('🧑‍💻');
    setDescription('');
    setSystemPrompt('');
    setTemperature(0.3);
    setResponseStyle('concise');
    setIsDefault(false);
    setError('');
  };

  if (!open) return null;

  const handleSave = async () => {
    if (!name.trim()) { setError('名称不能为空'); return; }
    if (!systemPrompt.trim()) { setError('系统提示词不能为空'); return; }
    setSaving(true);
    setError('');
    try {
      const p = await api.createPersona({
        name: name.trim(),
        title: title.trim() || undefined,
        emoji: emoji || undefined,
        description: description.trim() || undefined,
        system_prompt: systemPrompt,
        temperature,
        response_style: responseStyle,
        is_default: isDefault || undefined,
      });
      await fetchPersonas();
      onCreated(p);
      reset();
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-lg mx-4 max-h-[85vh] overflow-y-auto" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100">创建虚拟人</h2>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"><X size={18} /></button>
        </div>
        <div className="p-6 space-y-4">
          {error && (
            <div className="flex items-center gap-2 p-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
              <AlertTriangle size={14} /> {error}
            </div>
          )}
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">名称 *</label>
              <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Alex"
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm" />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">头衔</label>
              <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="资深 Rust 开发者"
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm" />
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">描述</label>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} placeholder="专注系统编程和性能优化" rows={2}
              className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm resize-none" />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">系统提示词 *</label>
            <textarea value={systemPrompt} onChange={(e) => setSystemPrompt(e.target.value)}
              placeholder="You are Alex, a senior Rust backend developer..."
              rows={5}
              className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm resize-none font-mono" />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">温度 ({temperature.toFixed(1)})</label>
              <input type="range" min="0" max="2" step="0.1" value={temperature}
                onChange={(e) => setTemperature(parseFloat(e.target.value))}
                className="w-full" />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">回复风格</label>
              <select value={responseStyle} onChange={(e) => setResponseStyle(e.target.value)}
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm">
                {Object.entries(RESPONSE_STYLE_LABELS).map(([k, v]) => (
                  <option key={k} value={k}>{v}</option>
                ))}
              </select>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <input type="checkbox" id="is-default-create" checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)}
              className="rounded border-gray-300 dark:border-gray-600" />
            <label htmlFor="is-default-create" className="text-sm text-gray-600 dark:text-gray-300">设为默认虚拟人</label>
          </div>
        </div>
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-gray-200 dark:border-gray-700">
          <button onClick={onClose} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg">取消</button>
          <button onClick={handleSave} disabled={saving}
            className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg disabled:opacity-50">
            {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
            创建
          </button>
        </div>
      </div>
    </div>
  );
}

function EditPersonaDialog({ open, persona, onClose, onUpdated }: {
  open: boolean;
  persona: PersonaInfo | null;
  onClose: () => void;
  onUpdated: () => void;
}) {
  const fetchPersonas = useStore((state) => state.fetchPersonas);
  const [name, setName] = useState('');
  const [title, setTitle] = useState('');
  const [emoji, setEmoji] = useState('');
  const [description, setDescription] = useState('');
  const [systemPrompt, setSystemPrompt] = useState('');
  const [temperature, setTemperature] = useState(0.3);
  const [responseStyle, setResponseStyle] = useState('concise');
  const [isDefault, setIsDefault] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (persona) {
      setName(persona.name);
      setTitle(persona.title);
      setEmoji(persona.emoji);
      setDescription(persona.description);
      setSystemPrompt(persona.system_prompt);
      setTemperature(persona.temperature);
      setResponseStyle(persona.response_style);
      setIsDefault(persona.is_default);
      setError('');
    }
  }, [persona]);

  if (!open || !persona) return null;

  const handleSave = async () => {
    if (!name.trim()) { setError('名称不能为空'); return; }
    if (!systemPrompt.trim()) { setError('系统提示词不能为空'); return; }
    setSaving(true);
    setError('');
    try {
      await api.updatePersona(persona.id, {
        name: name.trim(),
        title: title.trim() || undefined,
        emoji: emoji || undefined,
        description: description.trim() || undefined,
        system_prompt: systemPrompt,
        temperature,
        response_style: responseStyle,
        is_default: isDefault || undefined,
      });
      await fetchPersonas();
      onUpdated();
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-lg mx-4 max-h-[85vh] overflow-y-auto" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100">编辑虚拟人</h2>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"><X size={18} /></button>
        </div>
        <div className="p-6 space-y-4">
          {error && (
            <div className="flex items-center gap-2 p-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
              <AlertTriangle size={14} /> {error}
            </div>
          )}
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">名称 *</label>
              <input value={name} onChange={(e) => setName(e.target.value)}
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm" />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">头衔</label>
              <input value={title} onChange={(e) => setTitle(e.target.value)}
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm" />
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">描述</label>
            <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={2}
              className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm resize-none" />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">系统提示词 *</label>
            <textarea value={systemPrompt} onChange={(e) => setSystemPrompt(e.target.value)} rows={5}
              className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm resize-none font-mono" />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">温度 ({temperature.toFixed(1)})</label>
              <input type="range" min="0" max="2" step="0.1" value={temperature}
                onChange={(e) => setTemperature(parseFloat(e.target.value))} className="w-full" />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-600 dark:text-gray-300 mb-1">回复风格</label>
              <select value={responseStyle} onChange={(e) => setResponseStyle(e.target.value)}
                className="w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm">
                {Object.entries(RESPONSE_STYLE_LABELS).map(([k, v]) => (
                  <option key={k} value={k}>{v}</option>
                ))}
              </select>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <input type="checkbox" id="is-default-edit" checked={isDefault}
              onChange={(e) => setIsDefault(e.target.checked)}
              className="rounded border-gray-300 dark:border-gray-600" />
            <label htmlFor="is-default-edit" className="text-sm text-gray-600 dark:text-gray-300">设为默认虚拟人</label>
          </div>
        </div>
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-gray-200 dark:border-gray-700">
          <button onClick={onClose} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg">取消</button>
          <button onClick={handleSave} disabled={saving}
            className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg disabled:opacity-50">
            {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Link Memory Dialog ──

function LinkMemoryDialog({ persona, onClose }: { persona: PersonaInfo | null; onClose: () => void }) {
  const [allMemories, setAllMemories] = useState<{ id: string; content: string; memory_type: string }[]>([]);
  const [linkedIds, setLinkedIds] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [search, setSearch] = useState('');

  useEffect(() => {
    if (!persona) return;
    (async () => {
      setLoading(true);
      try {
        const [all, linked] = await Promise.all([
          api.listMemories(),
          api.getPersonaMemories(persona.id),
        ]);
        setAllMemories(all.map((m) => ({ id: m.id, content: m.content, memory_type: m.memory_type })));
        setLinkedIds(new Set(linked.map((m) => m.id)));
      } catch (err) {
        console.error('Failed to load memories', err);
      } finally {
        setLoading(false);
      }
    })();
  }, [persona]);

  if (!persona) return null;

  const toggle = (id: string) => {
    const next = new Set(linkedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setLinkedIds(next);
  };

  const handleSave = async () => {
    if (!persona) return;
    setSaving(true);
    try {
      // Get original linked IDs
      const original = new Set((await api.getPersonaMemories(persona.id)).map((m) => m.id));
      // Compute diff
      const toAdd = [...linkedIds].filter((id) => !original.has(id));
      const toRemove = [...original].filter((id) => !linkedIds.has(id));
      // Apply changes
      await Promise.all([
        ...toAdd.map((id) => api.linkMemoryToPersona(persona.id, id)),
        ...toRemove.map((id) => api.unlinkMemoryFromPersona(persona.id, id)),
      ]);
      onClose();
    } catch (err) {
      console.error('Failed to update memory links', err);
    } finally {
      setSaving(false);
    }
  };

  const filtered = allMemories.filter((m) =>
    !search || m.content.toLowerCase().includes(search.toLowerCase()) || m.memory_type.includes(search)
  );

  const MEMORY_TYPE_LABEL: Record<string, string> = {
    fact: '事实', preference: '偏好', project_context: '项目', user_info: '用户', conversation_summary: '对话摘要',
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-lg mx-4 max-h-[80vh] flex flex-col" onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100">
            关联记忆 — {persona.emoji} {persona.name}
          </h2>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"><X size={18} /></button>
        </div>

        {/* Search */}
        <div className="px-6 py-3 border-b border-gray-100 dark:border-gray-700">
          <input value={search} onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索记忆..."
            className="w-full px-3 py-1.5 text-sm border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700" />
        </div>

        {/* List */}
        <div className="flex-1 overflow-y-auto px-6 py-3 space-y-1 min-h-[200px]">
          {loading ? (
            <div className="flex items-center justify-center py-10"><Loader2 size={24} className="animate-spin text-purple-500" /></div>
          ) : filtered.length === 0 ? (
            <p className="text-center text-sm text-gray-400 dark:text-gray-500 py-10">
              {allMemories.length === 0 ? '还没有记忆，先去「记忆系统」创建' : '无匹配结果'}
            </p>
          ) : (
            filtered.map((m) => {
              const checked = linkedIds.has(m.id);
              return (
                <label key={m.id}
                  className={`flex items-start gap-3 p-3 rounded-lg cursor-pointer transition-colors ${
                    checked
                      ? 'bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800'
                      : 'hover:bg-gray-50 dark:hover:bg-gray-700/50 border border-transparent'
                  }`}
                >
                  <input type="checkbox" checked={checked} onChange={() => toggle(m.id)}
                    className="mt-0.5 rounded border-gray-300 dark:border-gray-600 text-blue-500" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm text-gray-700 dark:text-gray-200 line-clamp-2">{m.content}</p>
                    <span className="text-xs text-gray-400 dark:text-gray-500 mt-0.5 block">
                      {MEMORY_TYPE_LABEL[m.memory_type] || m.memory_type}
                    </span>
                  </div>
                </label>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-gray-200 dark:border-gray-700">
          <span className="text-xs text-gray-400">
            {linkedIds.size} / {allMemories.length} 条已关联
          </span>
          <div className="flex items-center gap-3">
            <button onClick={onClose} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg">取消</button>
            <button onClick={handleSave} disabled={saving}
              className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-blue-500 hover:bg-blue-600 rounded-lg disabled:opacity-50">
              {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
              保存关联
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function PersonaManagerPage() {
  const { personas, personaLoading, personaError, fetchPersonas, deletePersona } = useStore();
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<PersonaInfo | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [linkingPersona, setLinkingPersona] = useState<PersonaInfo | null>(null);

  useEffect(() => {
    fetchPersonas();
  }, [fetchPersonas]);

  return (
    <ManagerPageLayout
      title="虚拟人管理"
      icon={<Users size={18} />}
      headerActions={
        <button onClick={() => setCreating(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg transition-colors">
          <Plus size={14} /> 新建虚拟人
        </button>
      }
    >
      {/* Error state */}
      {personaError && (
        <div className="flex items-center gap-2 p-3 mb-4 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
          <AlertTriangle size={14} /> {personaError}
        </div>
      )}

      {/* Loading */}
      {personaLoading && personas.length === 0 && (
        <div className="flex items-center justify-center py-20">
          <Loader2 size={32} className="animate-spin text-purple-500" />
        </div>
      )}

      {/* Empty state */}
      {!personaLoading && personas.length === 0 && (
        <div className="flex flex-col items-center justify-center py-20 text-gray-400 dark:text-gray-500">
          <Users size={48} className="mb-3 opacity-50" />
          <p className="text-sm">还没有创建虚拟人</p>
          <p className="text-xs mt-1">点击右上角「新建虚拟人」创建一个</p>
        </div>
      )}

      {/* Persona cards grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
        {personas.map((p) => {
          const iconCfg = getPersonaIcon(p);
          return (
            <div key={p.id} className="group relative bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl p-5 hover:shadow-md transition-shadow">
              {/* Default badge */}
              {p.is_default && (
                <div className="absolute top-3 right-3 flex items-center gap-1 px-2 py-0.5 text-xs text-amber-600 bg-amber-50 dark:bg-amber-900/30 dark:text-amber-400 rounded-full">
                  <Star size={10} /> 默认
                </div>
              )}

              {/* Header: icon + name */}
              <div className="flex items-start gap-3 mb-3">
                <div className={`w-10 h-10 rounded-lg bg-gray-100 dark:bg-gray-700 flex items-center justify-center ${iconCfg.color}`}>
                  {iconCfg.icon}
                </div>
                <div className="flex-1 min-w-0">
                  <h3 className="text-base font-semibold text-gray-800 dark:text-gray-100 truncate">{p.name}</h3>
                  <p className="text-xs text-gray-400 dark:text-gray-500 truncate">{p.title}</p>
                </div>
              </div>

              {/* Description */}
              {p.description && (
                <p className="text-xs text-gray-500 dark:text-gray-400 mb-3 line-clamp-2">{p.description}</p>
              )}

              {/* System prompt preview */}
              <div className="mb-3 p-2 bg-gray-50 dark:bg-gray-900/50 rounded-lg">
                <p className="text-xs text-gray-400 dark:text-gray-500 line-clamp-2 font-mono">{p.system_prompt}</p>
              </div>

              {/* Meta info */}
              <div className="flex items-center gap-3 text-xs text-gray-400 dark:text-gray-500">
                <span>温度 {p.temperature.toFixed(1)}</span>
                <span>{RESPONSE_STYLE_LABELS[p.response_style] || p.response_style}</span>
              </div>

              {/* Actions */}
              <div className="flex items-center gap-2 mt-4 pt-3 border-t border-gray-100 dark:border-gray-700">
                <button onClick={() => setEditing(p)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs text-gray-500 hover:text-purple-500 hover:bg-purple-50 dark:hover:bg-purple-900/20 rounded-lg transition-colors">
                  <Edit3 size={12} /> 编辑
                </button>
                <button onClick={() => setLinkingPersona(p)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs text-gray-500 hover:text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/20 rounded-lg transition-colors">
                  <Link2 size={12} /> 关联记忆
                </button>
                <button onClick={() => setDeleting(p.id)}
                  className="flex items-center gap-1 px-2.5 py-1 text-xs text-gray-500 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors">
                  <Trash2 size={12} /> 删除
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* Dialogs */}
      <CreatePersonaDialog open={creating} onClose={() => setCreating(false)} onCreated={() => setCreating(false)} />
      <EditPersonaDialog open={!!editing} persona={editing} onClose={() => setEditing(null)} onUpdated={() => {}} />

      {/* Link Memory Dialog */}
      <LinkMemoryDialog persona={linkingPersona} onClose={() => setLinkingPersona(null)} />

      {/* Delete confirmation */}
      {deleting && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={() => setDeleting(null)}>
          <div className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-sm mx-4 p-6" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center text-red-500">
                <AlertTriangle size={20} />
              </div>
              <div>
                <h3 className="font-semibold text-gray-800 dark:text-gray-100">确认删除</h3>
                <p className="text-sm text-gray-500 dark:text-gray-400">此操作不可恢复</p>
              </div>
            </div>
            <div className="flex justify-end gap-3">
              <button onClick={() => setDeleting(null)} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg">取消</button>
              <button onClick={async () => {
                await deletePersona(deleting);
                setDeleting(null);
              }} className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-red-500 hover:bg-red-600 rounded-lg">
                <Trash2 size={14} /> 删除
              </button>
            </div>
          </div>
        </div>
      )}
    </ManagerPageLayout>
  );
}
