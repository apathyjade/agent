import { useState } from 'react';
import { X, FolderOpen, Loader2 } from 'lucide-react';
import * as api from '../api/tauri';

interface SkillInstallDialogProps {
  onClose: () => void;
}

export function SkillInstallDialog({ onClose }: SkillInstallDialogProps) {
  const [path, setPath] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  const handleBrowse = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        filters: [{ name: 'Skill 定义', extensions: ['yaml', 'yml'] }],
        multiple: false,
      });
      if (selected) {
        setPath(selected as string);
        setError(null);
      }
    } catch (err) {
      setError('文件选择器不可用: ' + String(err));
    }
  };

  const handleInstall = async () => {
    if (!path.trim()) return;
    setInstalling(true);
    setError(null);
    try {
      await api.installSkillFromPath(path.trim());
      onClose();
    } catch (err) {
      setError(String(err));
      setInstalling(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-800 rounded-2xl w-[480px] max-h-[85vh] flex flex-col shadow-2xl">
        <div className="flex items-center justify-between p-5 border-b border-gray-100 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">安装 Skill</h2>
          <button onClick={onClose} className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors text-gray-400 hover:text-gray-600 dark:hover:text-gray-300">
            <X size={20} />
          </button>
        </div>

        <div className="p-5 space-y-4">
          <div>
            <label className="text-xs text-gray-500 mb-1.5 block">本地 skill.yaml 文件路径</label>
            <div className="flex gap-2">
              <input
                type="text"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="C:\path\to\skill.yaml"
                className="flex-1 bg-white border border-gray-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
              />
              <button
                onClick={handleBrowse}
                className="flex items-center gap-1.5 px-3 py-2 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors"
              >
                <FolderOpen size={14} />
                浏览
              </button>
            </div>
          </div>

          {error && (
            <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700">
              {error}
            </div>
          )}

          <div className="flex gap-2 pt-2">
            <button
              onClick={handleInstall}
              disabled={!path.trim() || installing}
              className="flex-1 flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2.5 rounded-lg text-sm transition-all"
            >
              {installing && <Loader2 size={14} className="animate-spin" />}
              {installing ? '安装中...' : '安装'}
            </button>
            <button
              onClick={onClose}
              className="px-4 py-2 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors"
            >
              取消
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
