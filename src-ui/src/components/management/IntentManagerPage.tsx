import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  Download,
  Info,
  Loader2,
  Pencil,
  Plus,
  Save,
  Target,
  Trash2,
  Upload,
  X,
} from 'lucide-react';
import * as api from '../../api/intent';
import type { IntentDomainInfo, IntentInfo, IntentInput } from '../../api/intent';
import { ManagerPageLayout } from '../common/ManagerPageLayout';

// ── Constants ──

const TOOL_OPTIONS = [
  { value: 'file_system', label: '文件系统' },
  { value: 'code_executor', label: '命令执行' },
  { value: 'web_search', label: '网络搜索' },
  { value: 'calculator', label: '计算器' },
  { value: 'run_workflow', label: '运行工作流' },
];

const PATH_OPTIONS = [
  { value: 'fast', label: '快速' },
  { value: 'deep', label: '深度' },
];

const PATH_LABELS: Record<string, string> = {
  fast: '快速',
  deep: '深度',
};

// ── Helpers ──

function errText(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

function parseExamples(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

function sameList(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

/** Order-insensitive comparison for values that are semantically a set. */
function sameSet(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  const values = new Set(a);
  return b.every((value) => values.has(value));
}

/** Returns false only when the legacy execCommand fallback reports failure. */
async function copyToClipboard(text: string): Promise<boolean> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return true;
  }
  const el = document.createElement('textarea');
  el.value = text;
  el.style.position = 'fixed';
  el.style.opacity = '0';
  document.body.appendChild(el);
  el.select();
  const ok = document.execCommand('copy');
  document.body.removeChild(el);
  return ok;
}

// ── Field primitives ──

const inputClass =
  'w-full px-3 py-2 border border-gray-200 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-sm text-gray-800 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-purple-500';

const labelClass = 'block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1';

// ── Create domain dialog ──

function CreateDomainDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: () => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (open) {
      setName('');
      setDescription('');
      setError('');
    }
  }, [open]);

  if (!open) return null;

  const handleSave = async () => {
    if (!name.trim()) {
      setError('名称不能为空');
      return;
    }
    setSaving(true);
    setError('');
    try {
      await api.createIntentDomain(name.trim(), description);
      await onCreated();
      onClose();
    } catch (err) {
      setError(errText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-md mx-4"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100">新建领域</h2>
          <button
            onClick={onClose}
            aria-label="关闭"
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          >
            <X size={18} />
          </button>
        </div>
        <div className="p-6 space-y-4">
          {error && (
            <div className="flex items-center gap-2 p-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
              <AlertTriangle size={14} /> {error}
            </div>
          )}
          <div>
            <label className={labelClass}>名称 *</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：写作"
              className={inputClass}
            />
          </div>
          <div>
            <label className={labelClass}>描述</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={3}
              placeholder="这个领域负责什么"
              className={`${inputClass} resize-none`}
            />
          </div>
          <p className="text-[11px] text-gray-400 dark:text-gray-500">
            新建的领域不会自动设为默认领域，需要在左侧列表中切换。
          </p>
        </div>
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-gray-200 dark:border-gray-700">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg"
          >
            取消
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg disabled:opacity-50"
          >
            {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
            创建
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Import dialog ──

function ImportDomainDialog({
  open,
  onClose,
  onImported,
}: {
  open: boolean;
  onClose: () => void;
  onImported: () => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [json, setJson] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (open) {
      setName('');
      setJson('');
      setError('');
    }
  }, [open]);

  if (!open) return null;

  const handleImport = async () => {
    if (!name.trim()) {
      setError('领域名称不能为空');
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(json);
    } catch {
      setError('JSON 格式不正确');
      return;
    }
    if (!Array.isArray(parsed)) {
      setError('需要粘贴一个 JSON 数组');
      return;
    }
    if (parsed.length === 0) {
      setError('数组为空，至少需要一个意图');
      return;
    }
    setSaving(true);
    setError('');
    try {
      await api.importIntentDomain(json, name.trim());
      await onImported();
      onClose();
    } catch (err) {
      setError(errText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-2xl mx-4 max-h-[85vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-100">导入领域</h2>
          <button
            onClick={onClose}
            aria-label="关闭"
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          >
            <X size={18} />
          </button>
        </div>
        <div className="p-6 space-y-4 overflow-y-auto">
          {error && (
            <div className="flex items-center gap-2 p-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
              <AlertTriangle size={14} /> {error}
            </div>
          )}
          <div>
            <label className={labelClass}>领域名称 *</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：程序开发（导入）"
              className={inputClass}
            />
          </div>
          <div>
            <label className={labelClass}>意图 JSON 数组 *</label>
            <textarea
              value={json}
              onChange={(e) => setJson(e.target.value)}
              rows={12}
              placeholder={'[\n  { "name": "规划方案", "description": "..." }\n]'}
              className={`${inputClass} resize-none font-mono text-xs`}
            />
            <p className="text-[11px] text-gray-400 dark:text-gray-500 mt-1">
              每条意图的 id 会被忽略并重新生成；重名领域会自动加后缀。
            </p>
          </div>
        </div>
        <div className="flex items-center justify-end gap-3 px-6 py-4 border-t border-gray-200 dark:border-gray-700">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg"
          >
            取消
          </button>
          <button
            onClick={handleImport}
            disabled={saving}
            className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg disabled:opacity-50"
          >
            {saving ? <Loader2 size={14} className="animate-spin" /> : <Upload size={14} />}
            导入
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Intent editor drawer ──

type EditorState = { mode: 'create' } | { mode: 'edit'; intent: IntentInfo };

function IntentEditorDrawer({
  state,
  domainId,
  siblings,
  onClose,
  onSaved,
  onDeleted,
}: {
  state: EditorState;
  domainId: string;
  siblings: IntentInfo[];
  onClose: () => void;
  onSaved: () => Promise<void>;
  onDeleted: () => Promise<void>;
}) {
  const editing = state.mode === 'edit' ? state.intent : null;

  const [name, setName] = useState(editing?.name ?? '');
  const [description, setDescription] = useState(editing?.description ?? '');
  const [examplesText, setExamplesText] = useState((editing?.examples ?? []).join('\n'));
  const [selectedTools, setSelectedTools] = useState<string[]>(editing?.allowed_tools ?? []);
  const [path, setPath] = useState(editing?.path === 'deep' ? 'deep' : 'fast');
  const [threshold, setThreshold] = useState(editing?.confidence_threshold ?? 0.7);
  const [fallback, setFallback] = useState(editing?.fallback_intent ?? '');
  const [appendix, setAppendix] = useState(editing?.system_prompt_appendix ?? '');
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState('');
  const [confirmDelete, setConfirmDelete] = useState(false);

  const fallbackOptions = useMemo(
    () => siblings.filter((item) => item.id !== editing?.id),
    [siblings, editing?.id],
  );

  // Keep tool names that came in through import even though the picker only
  // labels the five built-in tools. Unknown values stay visible and removable.
  const toolOptions = useMemo(() => {
    const known = TOOL_OPTIONS.map((option) => ({ ...option }));
    const extras = (editing?.allowed_tools ?? [])
      .filter((value) => !TOOL_OPTIONS.some((option) => option.value === value))
      .map((value) => ({ value, label: value }));
    return [...known, ...extras];
  }, [editing]);

  const toggleTool = (value: string) => {
    setSelectedTools((prev) =>
      prev.includes(value) ? prev.filter((item) => item !== value) : [...prev, value],
    );
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setFormError('名称不能为空');
      return;
    }
    const normalizedThreshold = Number.isFinite(threshold)
      ? Math.min(1, Math.max(0, threshold))
      : 0.7;
    const examples = parseExamples(examplesText);
    const allowedTools = toolOptions
      .map((option) => option.value)
      .filter((value) => selectedTools.includes(value));

    setSaving(true);
    setFormError('');
    try {
      if (editing) {
        // Sparse update: only send the fields the user actually changed.
        const patch: IntentInput = {};
        if (name.trim() !== editing.name) patch.name = name.trim();
        if (description !== editing.description) patch.description = description;
        if (!sameList(examples, editing.examples)) patch.examples = examples;
        if (!sameSet(allowedTools, editing.allowed_tools)) patch.allowed_tools = allowedTools;
        if (path !== editing.path) patch.path = path;
        if (normalizedThreshold !== editing.confidence_threshold) {
          patch.confidence_threshold = normalizedThreshold;
        }
        if (fallback !== editing.fallback_intent) patch.fallback_intent = fallback;
        if (appendix !== editing.system_prompt_appendix) {
          patch.system_prompt_appendix = appendix;
        }
        if (Object.keys(patch).length > 0) {
          await api.updateIntent(editing.id, patch);
          await onSaved();
        }
      } else {
        await api.createIntent(domainId, {
          name: name.trim(),
          description,
          examples,
          allowed_tools: allowedTools,
          path,
          confidence_threshold: normalizedThreshold,
          fallback_intent: fallback,
          system_prompt_appendix: appendix,
        });
        await onSaved();
      }
      onClose();
    } catch (err) {
      setFormError(errText(err));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!editing) return;
    setSaving(true);
    setFormError('');
    try {
      await api.deleteIntent(editing.id);
      await onDeleted();
      onClose();
    } catch (err) {
      setFormError(errText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-y-0 right-0 w-full max-w-xl bg-white dark:bg-gray-800 shadow-2xl z-50 flex flex-col border-l border-gray-200 dark:border-gray-700 animate-in slide-in-from-right duration-200">
      <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-gray-700">
        <div className="flex items-center gap-2 min-w-0">
          <div className="w-8 h-8 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center flex-shrink-0">
            <Pencil size={15} className="text-purple-600 dark:text-purple-400" />
          </div>
          <div className="min-w-0">
            <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100 truncate">
              {editing ? '编辑意图' : '新建意图'}
            </h2>
            <p className="text-[11px] text-gray-400 dark:text-gray-500 font-mono truncate">
              {editing ? editing.id : '保存后生成 id'}
            </p>
          </div>
        </div>
        <button
          onClick={onClose}
          aria-label="关闭"
          className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
        >
          <X size={18} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-5 space-y-4">
        {formError && (
          <div className="flex items-start gap-2 p-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
            <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" /> <span>{formError}</span>
          </div>
        )}

        <div>
          <label className={labelClass}>名称 *</label>
          <input
            value={name}
            aria-label="名称"
            onChange={(e) => setName(e.target.value)}
            className={inputClass}
          />
        </div>

        <div>
          <label className={labelClass}>描述</label>
          <textarea
            value={description}
            aria-label="描述"
            onChange={(e) => setDescription(e.target.value)}
            rows={3}
            placeholder="分类器的主要依据，写清与相邻意图的区别"
            className={`${inputClass} resize-none`}
          />
        </div>

        <div>
          <label className={labelClass}>示例（每行一条）</label>
          <textarea
            value={examplesText}
            aria-label="示例"
            onChange={(e) => setExamplesText(e.target.value)}
            rows={4}
            placeholder={'帮我做个登录页\n把这个需求拆一下'}
            className={`${inputClass} resize-none`}
          />
        </div>

        <div>
          <label className={labelClass}>工具白名单</label>
          <div className="grid grid-cols-2 gap-2">
            {toolOptions.map((option) => {
              const checked = selectedTools.includes(option.value);
              return (
                <label
                  key={option.value}
                  className={`flex items-center gap-2 px-2.5 py-1.5 rounded-lg border cursor-pointer transition-colors ${
                    checked
                      ? 'border-purple-200 dark:border-purple-800 bg-purple-50 dark:bg-purple-900/20'
                      : 'border-gray-200 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700/50'
                  }`}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    aria-label={`工具 ${option.label}`}
                    onChange={() => toggleTool(option.value)}
                    className="rounded border-gray-300 dark:border-gray-600"
                  />
                  <span className="text-xs text-gray-700 dark:text-gray-200 truncate">
                    {option.label}
                  </span>
                  <code className="text-[10px] text-gray-400 dark:text-gray-500 ml-auto">
                    {option.value}
                  </code>
                </label>
              );
            })}
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className={labelClass}>路径</label>
            <select
              value={path}
              aria-label="路径"
              onChange={(e) => setPath(e.target.value)}
              className={inputClass}
            >
              {PATH_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}（{option.value}）
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className={labelClass}>置信度阈值（0–1）</label>
            <input
              type="number"
              min={0}
              max={1}
              step={0.05}
              value={threshold}
              aria-label="置信度阈值"
              onChange={(e) => setThreshold(Number(e.target.value))}
              className={inputClass}
            />
          </div>
        </div>

        <div>
          <label className={labelClass}>兜底意图</label>
          <select
            value={fallback}
            aria-label="兜底意图"
            onChange={(e) => setFallback(e.target.value)}
            className={inputClass}
          >
            <option value="">无（不跳转）</option>
            {fallbackOptions.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name}
              </option>
            ))}
          </select>
          <p className="text-[11px] text-gray-400 dark:text-gray-500 mt-1">
            分类置信度低于阈值时改走这个意图。
          </p>
        </div>

        <div>
          <label className={labelClass}>系统提示词附录</label>
          <textarea
            value={appendix}
            aria-label="系统提示词附录"
            onChange={(e) => setAppendix(e.target.value)}
            rows={3}
            placeholder="追加到该意图系统提示词末尾的内容"
            className={`${inputClass} resize-none font-mono text-xs`}
          />
        </div>

        <div className="rounded-lg border border-dashed border-gray-200 dark:border-gray-700 p-3 space-y-2">
          <div className="flex items-center gap-1.5 text-xs font-medium text-gray-500 dark:text-gray-400">
            <Info size={12} /> 后续接入字段（当前只保存，不参与路由）
          </div>
          <dl className="grid grid-cols-[90px_1fr] gap-x-2 gap-y-1 text-xs">
            <dt className="text-gray-400 dark:text-gray-500">workflow</dt>
            <dd className="font-mono text-gray-600 dark:text-gray-300 truncate">
              {editing?.workflow || '—'}
            </dd>
            <dt className="text-gray-400 dark:text-gray-500">default_agent</dt>
            <dd className="font-mono text-gray-600 dark:text-gray-300 truncate">
              {editing?.default_agent || '—'}
            </dd>
            <dt className="text-gray-400 dark:text-gray-500">scope</dt>
            <dd className="font-mono text-gray-600 dark:text-gray-300 truncate">
              {editing?.scope || '—'}
            </dd>
          </dl>
          <p className="text-[11px] text-gray-400 dark:text-gray-500">
            这三个字段本次仅存储，驱动 workflow 与子 agent 留到后续版本。
          </p>
        </div>
      </div>

      <div className="px-5 py-3 border-t border-gray-100 dark:border-gray-700 flex items-center justify-between gap-3">
        {editing ? (
          confirmDelete ? (
            <div className="flex items-center gap-2">
              <span className="text-xs text-red-600 dark:text-red-400">确认删除？</span>
              <button
                onClick={handleDelete}
                disabled={saving}
                className="flex items-center gap-1 px-3 py-1.5 text-xs text-white bg-red-500 hover:bg-red-600 rounded-lg disabled:opacity-50"
              >
                {saving && <Loader2 size={11} className="animate-spin" />}
                确认删除
              </button>
              <button
                onClick={() => setConfirmDelete(false)}
                disabled={saving}
                className="px-3 py-1.5 text-xs text-gray-600 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg"
              >
                取消
              </button>
            </div>
          ) : (
            <button
              onClick={() => setConfirmDelete(true)}
              className="flex items-center gap-1.5 px-3 py-2 text-xs text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
            >
              <Trash2 size={13} /> 删除意图
            </button>
          )
        ) : (
          <span />
        )}
        <div className="flex items-center gap-2">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg"
          >
            取消
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg disabled:opacity-50"
          >
            {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
            {editing ? '保存' : '创建'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Main page ──

export function IntentManagerPage() {
  const [domains, setDomains] = useState<IntentDomainInfo[]>([]);
  const [activeDomain, setActiveDomain] = useState<IntentDomainInfo | null>(null);
  const [intents, setIntents] = useState<IntentInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [busyId, setBusyId] = useState<string | null>(null);
  const [createDomainOpen, setCreateDomainOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<IntentDomainInfo | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [copied, setCopied] = useState(false);

  const loadDomainIntents = useCallback(async (domainId: string) => {
    const rows = await api.listIntents(domainId);
    setIntents([...rows].sort((a, b) => a.sort_order - b.sort_order));
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextDomains, nextActive] = await Promise.all([
        api.listIntentDomains(),
        api.getActiveIntentDomain(),
      ]);
      setDomains(nextDomains);
      setActiveDomain(nextActive);
      if (nextActive) {
        await loadDomainIntents(nextActive.id);
      } else {
        setIntents([]);
      }
    } catch (err) {
      setError(`加载意图数据失败：${errText(err)}`);
    } finally {
      setLoading(false);
    }
  }, [loadDomainIntents]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleActivate = async (domain: IntentDomainInfo) => {
    if (activeDomain?.id === domain.id) return;
    setBusyId(domain.id);
    setError('');
    try {
      await api.setActiveIntentDomain(domain.id);
      await refresh();
    } catch (err) {
      setError(`切换领域失败：${errText(err)}`);
    } finally {
      setBusyId(null);
    }
  };

  const handleDeleteDomainClick = (domain: IntentDomainInfo) => {
    if (activeDomain?.id === domain.id) {
      setError(`「${domain.name}」是当前默认领域，不能删除。请先把其他领域设为默认。`);
      return;
    }
    setError('');
    setDeleteTarget(domain);
  };

  const handleDeleteDomain = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setBusyId(target.id);
    setError('');
    try {
      await api.deleteIntentDomain(target.id);
      setDeleteTarget(null);
      await refresh();
    } catch (err) {
      setDeleteTarget(null);
      setError(`删除领域失败：${errText(err)}`);
    } finally {
      setBusyId(null);
    }
  };

  const handleToggle = async (intent: IntentInfo) => {
    setBusyId(intent.id);
    try {
      await api.setIntentEnabled(intent.id, !intent.enabled);
      if (activeDomain) {
        await loadDomainIntents(activeDomain.id);
      }
      setError('');
    } catch (err) {
      setError(`切换启用状态失败：${errText(err)}`);
    } finally {
      setBusyId(null);
    }
  };

  const handleExport = async () => {
    if (!activeDomain) return;
    try {
      const json = await api.exportIntentDomain(activeDomain.id);
      const copiedOk = await copyToClipboard(json);
      if (!copiedOk) {
        setError('已生成导出内容，但复制到剪贴板失败，请检查系统权限后重试。');
        return;
      }
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
      setError('');
    } catch (err) {
      setError(`导出失败：${errText(err)}`);
    }
  };

  const sortedIntents = useMemo(
    () => [...intents].sort((a, b) => a.sort_order - b.sort_order),
    [intents],
  );
  const enabledCount = sortedIntents.filter((intent) => intent.enabled).length;

  return (
    <ManagerPageLayout
      title="意图管理"
      subtitle={activeDomain ? `默认领域：${activeDomain.name}` : '按领域管理意图集合'}
      icon={<Target size={18} />}
      headerActions={
        <>
          <button
            onClick={() => setImportOpen(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-gray-600 dark:text-gray-300 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 rounded-lg transition-colors"
          >
            <Upload size={14} /> 导入
          </button>
          <button
            onClick={handleExport}
            disabled={!activeDomain}
            title={activeDomain ? '复制当前领域的 JSON' : '请先选择一个默认领域'}
            className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-gray-600 dark:text-gray-300 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {copied ? <Check size={14} /> : <Download size={14} />}
            {copied ? '已复制' : '导出'}
          </button>
        </>
      }
    >
      <div className="flex h-full min-h-[420px] flex-col gap-4">
        {error && (
          <div className="flex-shrink-0 flex items-center justify-between gap-2 p-3 text-sm text-red-600 bg-red-50 dark:bg-red-900/20 dark:text-red-400 rounded-lg">
            <span className="flex items-center gap-2">
              <AlertTriangle size={14} /> {error}
            </span>
            <button
              onClick={() => setError('')}
              aria-label="关闭提示"
              className="text-red-400 hover:text-red-600 dark:hover:text-red-300"
            >
              <X size={14} />
            </button>
          </div>
        )}

        <div className="flex min-h-0 flex-1 gap-4">
          {/* ── Domains ── */}
          <aside className="w-60 flex-shrink-0 flex flex-col bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl overflow-hidden">
            <div className="flex items-center justify-between px-4 py-3 border-b border-gray-100 dark:border-gray-700">
              <span className="text-sm font-semibold text-gray-800 dark:text-gray-100">领域</span>
              <button
                onClick={() => setCreateDomainOpen(true)}
                aria-label="新建领域"
                title="新建领域"
                className="p-1 rounded-md text-gray-400 hover:text-purple-500 hover:bg-purple-50 dark:hover:bg-purple-900/20 transition-colors"
              >
                <Plus size={15} />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-2 space-y-1">
              {loading && domains.length === 0 && (
                <div className="flex items-center justify-center py-8 text-gray-400">
                  <Loader2 size={16} className="animate-spin" />
                </div>
              )}
              {!loading && domains.length === 0 && (
                <p className="px-2 py-8 text-center text-xs text-gray-400 dark:text-gray-500">
                  还没有领域，点击右上角加号新建
                </p>
              )}
              {domains.map((domain) => {
                const isActive = activeDomain?.id === domain.id;
                return (
                  <div
                    key={domain.id}
                    className={`group flex items-center gap-1 rounded-lg pr-1 ${
                      isActive
                        ? 'bg-purple-50 dark:bg-purple-900/20'
                        : 'hover:bg-gray-50 dark:hover:bg-gray-700/50'
                    }`}
                  >
                    <button
                      onClick={() => void handleActivate(domain)}
                      disabled={busyId === domain.id}
                      className="flex flex-1 min-w-0 items-center gap-2 px-2.5 py-2 text-left disabled:opacity-60"
                    >
                      <span
                        className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${
                          isActive ? 'bg-purple-500' : 'bg-gray-300 dark:bg-gray-600'
                        }`}
                      />
                      <span
                        className={`truncate text-sm ${
                          isActive
                            ? 'font-medium text-purple-700 dark:text-purple-300'
                            : 'text-gray-700 dark:text-gray-200'
                        }`}
                      >
                        {domain.name}
                      </span>
                      {domain.source === 'builtin' && (
                        <span className="text-[10px] px-1 rounded bg-gray-100 dark:bg-gray-700 text-gray-400 flex-shrink-0">
                          内置
                        </span>
                      )}
                    </button>
                    {busyId === domain.id ? (
                      <Loader2 size={13} className="animate-spin text-purple-500 flex-shrink-0" />
                    ) : (
                      <button
                        onClick={() => handleDeleteDomainClick(domain)}
                        aria-label={`删除领域 ${domain.name}`}
                        title="删除领域"
                        className="p-1 rounded-md text-gray-300 dark:text-gray-600 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors flex-shrink-0"
                      >
                        <Trash2 size={13} />
                      </button>
                    )}
                  </div>
                );
              })}
            </div>

            <div className="px-3 py-2 border-t border-gray-100 dark:border-gray-700 text-[11px] text-gray-400 dark:text-gray-500">
              默认领域不可删除
            </div>
          </aside>

          {/* ── Intents of the active domain ── */}
          <section className="flex-1 min-w-0 flex flex-col bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl overflow-hidden">
            {activeDomain ? (
              <>
                <div className="flex items-center justify-between gap-3 px-5 py-3 border-b border-gray-100 dark:border-gray-700">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <h2 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
                        {activeDomain.name}
                      </h2>
                      <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-emerald-50 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-400">
                        默认
                      </span>
                      <span className="text-[11px] text-gray-400 dark:text-gray-500">
                        {sortedIntents.length} 个意图 · {enabledCount} 个启用
                      </span>
                    </div>
                    {activeDomain.description && (
                      <p className="text-xs text-gray-400 dark:text-gray-500 truncate mt-0.5">
                        {activeDomain.description}
                      </p>
                    )}
                  </div>
                  <button
                    onClick={() => setEditor({ mode: 'create' })}
                    className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg transition-colors flex-shrink-0"
                  >
                    <Plus size={14} /> 新建意图
                  </button>
                </div>

                <div className="flex-1 overflow-y-auto p-3 space-y-1">
                  {loading && sortedIntents.length === 0 && (
                    <div className="flex items-center justify-center py-12 text-gray-400">
                      <Loader2 size={20} className="animate-spin text-purple-500" />
                    </div>
                  )}
                  {!loading && sortedIntents.length === 0 && (
                    <div className="flex flex-col items-center justify-center py-16 text-gray-400 dark:text-gray-500">
                      <Target size={40} className="mb-3 opacity-40" />
                      <p className="text-sm">这个领域还没有意图</p>
                      <p className="text-xs mt-1">点击右上角「新建意图」，或用顶部「导入」粘贴 JSON</p>
                    </div>
                  )}
                  {sortedIntents.map((intent) => {
                    const isLastEnabled = intent.enabled && enabledCount <= 1;
                    const toggleTitle = isLastEnabled
                      ? '领域内至少要保留一个启用的意图，不能停用最后一个'
                      : intent.enabled
                        ? '停用这个意图'
                        : '启用这个意图';
                    return (
                      <div
                        key={intent.id}
                        className="flex items-center gap-2 px-2 py-2 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700/40 transition-colors"
                      >
                        <button
                          onClick={() => setEditor({ mode: 'edit', intent })}
                          className="flex flex-1 min-w-0 items-center gap-2 text-left"
                        >
                          <span
                            className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${
                              intent.enabled ? 'bg-emerald-500' : 'bg-gray-300 dark:bg-gray-600'
                            }`}
                          />
                          <span
                            className={`truncate text-sm ${
                              intent.enabled
                                ? 'text-gray-800 dark:text-gray-100'
                                : 'text-gray-400 dark:text-gray-500 line-through'
                            }`}
                          >
                            {intent.name}
                          </span>
                          <span
                            title={intent.path}
                            className={`text-[10px] px-1.5 py-0.5 rounded-full flex-shrink-0 ${
                              intent.path === 'deep'
                                ? 'bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-300'
                                : 'bg-sky-50 dark:bg-sky-900/30 text-sky-600 dark:text-sky-300'
                            }`}
                          >
                            {PATH_LABELS[intent.path] ?? intent.path}
                          </span>
                          {intent.description && (
                            <span className="hidden md:inline text-[11px] text-gray-400 dark:text-gray-500 truncate">
                              {intent.description}
                            </span>
                          )}
                        </button>
                        {busyId === intent.id ? (
                          <Loader2 size={14} className="animate-spin text-purple-500 flex-shrink-0" />
                        ) : (
                          <button
                            type="button"
                            role="switch"
                            aria-checked={intent.enabled}
                            aria-label={`${intent.name} 启用开关`}
                            title={toggleTitle}
                            disabled={isLastEnabled}
                            onClick={() => void handleToggle(intent)}
                            className={`relative inline-flex items-center h-4 w-8 rounded-full transition-colors flex-shrink-0 disabled:opacity-50 disabled:cursor-not-allowed ${
                              intent.enabled ? 'bg-purple-600' : 'bg-gray-300 dark:bg-gray-600'
                            }`}
                          >
                            <span
                              className={`inline-block h-3 w-3 rounded-full bg-white transition-transform ${
                                intent.enabled ? 'translate-x-[17px]' : 'translate-x-[2px]'
                              }`}
                            />
                          </button>
                        )}
                      </div>
                    );
                  })}
                </div>
              </>
            ) : (
              <div className="flex-1 flex flex-col items-center justify-center px-6 text-gray-400 dark:text-gray-500">
                <Target size={44} className="mb-3 opacity-40" />
                <p className="text-sm font-medium text-gray-500 dark:text-gray-400">没有默认领域</p>
                <p className="text-xs mt-1 text-center">
                  请选择或新建一个领域，然后管理其中的意图
                </p>
                <button
                  onClick={() => setCreateDomainOpen(true)}
                  className="mt-4 flex items-center gap-1.5 px-3 py-1.5 text-sm text-white bg-purple-500 hover:bg-purple-600 rounded-lg transition-colors"
                >
                  <Plus size={14} /> 新建领域
                </button>
              </div>
            )}
          </section>
        </div>
      </div>

      {/* ── Dialogs ── */}
      <CreateDomainDialog
        open={createDomainOpen}
        onClose={() => setCreateDomainOpen(false)}
        onCreated={refresh}
      />

      <ImportDomainDialog
        open={importOpen}
        onClose={() => setImportOpen(false)}
        onImported={refresh}
      />

      {deleteTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
          onClick={() => setDeleteTarget(null)}
        >
          <div
            className="bg-white dark:bg-gray-800 rounded-xl shadow-xl w-full max-w-sm mx-4 p-6"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center text-red-500">
                <AlertTriangle size={20} />
              </div>
              <div>
                <h3 className="font-semibold text-gray-800 dark:text-gray-100">确认删除领域</h3>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  「{deleteTarget.name}」及其中所有意图都会被删除，不可恢复
                </p>
              </div>
            </div>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setDeleteTarget(null)}
                className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg"
              >
                取消
              </button>
              <button
                onClick={() => void handleDeleteDomain()}
                disabled={busyId === deleteTarget.id}
                className="flex items-center gap-2 px-4 py-2 text-sm text-white bg-red-500 hover:bg-red-600 rounded-lg disabled:opacity-50"
              >
                {busyId === deleteTarget.id ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : (
                  <Trash2 size={14} />
                )}
                确认删除
              </button>
            </div>
          </div>
        </div>
      )}

      {editor && activeDomain && (
        <IntentEditorDrawer
          key={editor.mode === 'edit' ? editor.intent.id : 'create'}
          state={editor}
          domainId={activeDomain.id}
          siblings={sortedIntents}
          onClose={() => setEditor(null)}
          onSaved={async () => {
            await loadDomainIntents(activeDomain.id);
          }}
          onDeleted={async () => {
            await loadDomainIntents(activeDomain.id);
          }}
        />
      )}
    </ManagerPageLayout>
  );
}
