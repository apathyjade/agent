import { useState, useEffect } from 'react';
import { X, Trash2, Loader2 } from 'lucide-react';
import * as api from '../api/tauri';
import type { SkillDetail } from '../types';

interface SkillDetailPanelProps {
  skillId: string;
  onClose: () => void;
}

interface SchemaProperty {
  title?: string;
  description?: string;
  type?: string;
  default?: unknown;
  enum?: string[];
}

type SchemaProperties = Record<string, SchemaProperty>;

export function SkillDetailPanel({ skillId, onClose }: SkillDetailPanelProps) {
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [config, setConfig] = useState<Record<string, unknown>>({});
  const [saving, setSaving] = useState(false);
  const [confirmUninstall, setConfirmUninstall] = useState(false);

  useEffect(() => {
    loadDetail();
  }, [skillId]);

  const loadDetail = async () => {
    setLoading(true);
    setError(null);
    try {
      const d = await api.getSkillDetail(skillId);
      setDetail(d);
      setConfig(d.config || {});
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  const handleSaveConfig = async () => {
    if (!detail) return;
    setSaving(true);
    setError(null);
    try {
      await api.configureSkill(skillId, config);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleUninstall = async () => {
    setSaving(true);
    try {
      await api.uninstallSkill(skillId);
      onClose();
    } catch (err) {
      setError(String(err));
      setSaving(false);
    }
  };

  const updateConfigField = (key: string, value: unknown) => {
    setConfig((prev) => ({ ...prev, [key]: value }));
  };

  if (loading) {
    return (
      <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
        <div className="bg-white rounded-2xl p-8 shadow-2xl">
          <Loader2 size={24} className="animate-spin text-purple-600 mx-auto" />
        </div>
      </div>
    );
  }

  if (error && !detail) {
    return (
      <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
        <div className="bg-white rounded-2xl p-6 shadow-2xl">
          <p className="text-sm text-red-600 mb-4">{error}</p>
          <button onClick={onClose} className="px-4 py-2 bg-gray-100 rounded-lg text-sm">关闭</button>
        </div>
      </div>
    );
  }

  if (!detail) return null;

  // Render form fields from config_schema
  const renderConfigFields = () => {
    const schema = detail.config_schema;
    if (!schema) return <p className="text-xs text-gray-400">此 Skill 没有可配置的选项</p>;

    const properties = (schema as Record<string, unknown>).properties as SchemaProperties | undefined;
    if (!properties) return <p className="text-xs text-gray-400">此 Skill 没有可配置的选项</p>;

    return Object.entries(properties).map(([key, prop]) => {
      const value = config[key] ?? prop.default ?? '';

      if (prop.enum) {
        return (
          <div key={key}>
            <label className="text-xs text-gray-500 mb-1 block capitalize">{prop.title || key}</label>
            <select
              value={String(value)}
              onChange={(e) => updateConfigField(key, e.target.value)}
              className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
            >
              {prop.enum.map((opt: string) => (
                <option key={opt} value={opt}>{opt}</option>
              ))}
            </select>
            {prop.description && <p className="text-xs text-gray-400 mt-0.5">{prop.description}</p>}
          </div>
        );
      }

      if (prop.type === 'boolean') {
        return (
          <div key={key} className="flex items-center justify-between">
            <label className="text-sm text-gray-700 capitalize">{prop.title || key}</label>
            <label className="relative inline-flex items-center cursor-pointer">
              <input
                type="checkbox"
                checked={!!value}
                onChange={(e) => updateConfigField(key, e.target.checked)}
                className="sr-only peer"
              />
              <div className="w-11 h-6 bg-gray-200 rounded-full peer peer-checked:after:translate-x-full after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-purple-600 shadow-sm"></div>
            </label>
          </div>
        );
      }

      return (
        <div key={key}>
          <label className="text-xs text-gray-500 mb-1 block capitalize">{prop.title || key}</label>
          <input
            type={prop.type === 'number' ? 'number' : 'text'}
            value={String(value)}
            onChange={(e) => updateConfigField(key, prop.type === 'number' ? Number(e.target.value) : e.target.value)}
            placeholder={prop.description || ''}
            className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
          />
          {prop.description && <p className="text-xs text-gray-400 mt-0.5">{prop.description}</p>}
        </div>
      );
    });
  };

  return (
    <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-800 rounded-2xl w-[520px] max-h-[85vh] flex flex-col shadow-2xl">
        <div className="flex items-center justify-between p-5 border-b border-gray-100 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">{detail.name}</h2>
          <button onClick={onClose} className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors text-gray-400 hover:text-gray-600 dark:hover:text-gray-300">
            <X size={20} />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-5 space-y-4">
          {/* Metadata */}
          <div className="grid grid-cols-2 gap-3 text-sm">
            <div>
              <span className="text-xs text-gray-400 block">ID</span>
              <span className="text-gray-700 dark:text-gray-300">{detail.id}</span>
            </div>
            <div>
              <span className="text-xs text-gray-400 block">版本</span>
              <span className="text-gray-700 dark:text-gray-300">{detail.version}</span>
            </div>
            <div>
              <span className="text-xs text-gray-400 block">作者</span>
              <span className="text-gray-700 dark:text-gray-300">{detail.author || '-'}</span>
            </div>
            <div>
              <span className="text-xs text-gray-400 block">来源</span>
              <span className="text-gray-700 dark:text-gray-300 capitalize">{detail.source}</span>
            </div>
            <div>
              <span className="text-xs text-gray-400 block">执行类型</span>
              <span className="text-gray-700 dark:text-gray-300">{detail.entry_type}</span>
            </div>
            <div>
              <span className="text-xs text-gray-400 block">已启用</span>
              <span className={detail.enabled ? 'text-green-600' : 'text-gray-400'}>{detail.enabled ? '是' : '否'}</span>
            </div>
          </div>

          <div>
            <span className="text-xs text-gray-400 block">描述</span>
            <p className="text-sm text-gray-700 dark:text-gray-300 mt-0.5">{detail.description}</p>
          </div>

          {detail.source_path && (
            <div>
              <span className="text-xs text-gray-400 block">路径</span>
              <p className="text-xs text-gray-500 mt-0.5 break-all">{detail.source_path}</p>
            </div>
          )}

          {/* Config form */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">配置</h3>
            <div className="space-y-3">
              {renderConfigFields()}
            </div>
          </div>

          {error && (
            <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700">
              {error}
            </div>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center justify-between p-5 border-t border-gray-100 dark:border-gray-700">
          {confirmUninstall ? (
            <div className="flex items-center gap-2">
              <span className="text-sm text-red-600">确认卸载？</span>
              <button
                onClick={handleUninstall}
                disabled={saving}
                className="flex items-center gap-1 px-3 py-1.5 bg-red-600 hover:bg-red-700 text-white rounded-lg text-sm transition-colors"
              >
                {saving ? <Loader2 size={12} className="animate-spin" /> : null}
                确认
              </button>
              <button
                onClick={() => setConfirmUninstall(false)}
                className="px-3 py-1.5 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors"
              >
                取消
              </button>
            </div>
          ) : (
            <button
              onClick={() => setConfirmUninstall(true)}
              className="flex items-center gap-1.5 px-3 py-2 text-red-600 hover:bg-red-50 rounded-lg text-sm transition-colors"
              title="卸载此 Skill"
            >
              <Trash2 size={14} />
              卸载
            </button>
          )}

          <div className="flex gap-2">
            <button onClick={onClose} className="px-4 py-2 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors">
              取消
            </button>
            <button
              onClick={handleSaveConfig}
              disabled={saving || !detail.config_schema}
              className="flex items-center gap-2 bg-purple-600 hover:bg-purple-700 disabled:opacity-50 text-white px-4 py-2 rounded-lg text-sm transition-colors"
            >
              {saving && <Loader2 size={14} className="animate-spin" />}
              保存
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
