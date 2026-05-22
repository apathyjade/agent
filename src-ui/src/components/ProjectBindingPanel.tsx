import { useEffect, useState, useCallback } from 'react';
import { FolderOpen, Plus, Trash2, RefreshCw, CheckCircle2, AlertTriangle, Folder, Loader2, X } from 'lucide-react';
import { useStore } from '../store';
import type { ProjectScanResult } from '../types';

const RUNTIME_EMOJI: Record<string, string> = {
  node: '⚡', python: '🐍', docker: '🐳', uv: '🦀', go: '🔷',
  rust: '🦀', java: '☕', deno: '🦕', bun: '🥟', ruby: '💎',
};

export function ProjectBindingPanel() {
  const {
    projectBindings, projectBindingLoading,
    fetchProjectBindings, addProjectBinding, removeProjectBinding,
    syncProjectBinding, scanProjectBinding, runtimes,
  } = useStore();
  const [showAddForm, setShowAddForm] = useState(false);
  const [pathInput, setPathInput] = useState('');
  const [scanResult, setScanResult] = useState<ProjectScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [syncingId, setSyncingId] = useState<string | null>(null);

  useEffect(() => {
    fetchProjectBindings();
  }, [fetchProjectBindings]);

  const handleScan = useCallback(async () => {
    if (!pathInput.trim()) return;
    setScanning(true);
    const result = await scanProjectBinding(pathInput.trim());
    setScanResult(result);
    setScanning(false);
  }, [pathInput, scanProjectBinding]);

  const handleAdd = useCallback(async () => {
    if (!pathInput.trim()) return;
    await addProjectBinding(pathInput.trim());
    setShowAddForm(false);
    setPathInput('');
    setScanResult(null);
  }, [pathInput, addProjectBinding]);

  const handleSync = useCallback(async (id: string) => {
    setSyncingId(id);
    await syncProjectBinding(id);
    setSyncingId(null);
  }, [syncProjectBinding]);

  const getVersionStatus = (rt: string, _spec: string) => {
    const runtime = runtimes.find(r => r.runtime_type === rt);
    if (!runtime?.available) return { icon: '❌', color: 'text-red-500', text: '\u672A\u5B89\u88C5' };
    return { icon: '✅', color: 'text-green-500', text: runtime.version || '\u672A\u77E5' };
  };

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            {'\u7ED1\u5B9A\u9879\u76EE\u6587\u4EF6\u5939\u540E\uFF0C\u81EA\u52A8\u68C0\u6D4B\u8FD0\u884C\u65F6\u7248\u672C\u9700\u6C42'}
          </p>
        </div>
        <button
          onClick={() => setShowAddForm(!showAddForm)}
          className="flex items-center gap-1.5 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-xs transition-all"
        >
          <Plus size={14} />
          {'\u6DFB\u52A0\u9879\u76EE'}
        </button>
      </div>

      {/* Loading state */}
      {projectBindingLoading && (
        <div className="flex items-center justify-center py-4 text-gray-400">
          <Loader2 size={16} className="animate-spin mr-2" />
          <span className="text-sm">加载中...</span>
        </div>
      )}

      {/* Project list - empty state */}
      {!projectBindingLoading && projectBindings.length === 0 && !showAddForm && (
        <div className="p-8 text-center text-gray-400 dark:text-gray-500">
          <FolderOpen size={32} className="mx-auto mb-2 opacity-50" />
          <p className="text-sm">暂无绑定项目</p>
          <p className="text-xs mt-1">点击「添加项目」绑定项目文件夹</p>
        </div>
      )}

      {/* Project list */}
      {!projectBindingLoading && projectBindings.length > 0 && (
        <div className="space-y-3">
          {projectBindings.map((project) => (
            <div key={project.id} className="p-4 bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <Folder size={16} className="text-purple-500" />
                  <span className="text-sm font-medium text-gray-900 dark:text-gray-100">{project.name}</span>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => handleSync(project.id)}
                    disabled={syncingId === project.id}
                    className="flex items-center gap-1 text-xs text-purple-500 hover:text-purple-600 transition-colors"
                  >
                    <RefreshCw size={12} className={syncingId === project.id ? 'animate-spin' : ''} />
                    同步
                  </button>
                  <button
                    onClick={() => removeProjectBinding(project.id)}
                    className="p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-red-500 transition-colors"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>

              <p className="text-[11px] text-gray-400 dark:text-gray-500 mb-2 truncate">{project.path}</p>

              {project.requirements.length > 0 ? (
                <div className="space-y-1">
                  {project.requirements.map((req, idx) => {
                    const status = getVersionStatus(req.runtime_type, req.version_spec);
                    return (
                      <div key={`${req.runtime_type}-${idx}`} className="flex items-center gap-3 px-2.5 py-1.5 rounded-lg bg-gray-50 dark:bg-gray-700/50 text-xs">
                        <span>{RUNTIME_EMOJI[req.runtime_type] || '⚙️'}</span>
                        <span className="text-gray-900 dark:text-gray-100 font-medium min-w-[60px]">{req.runtime_type.toUpperCase()}</span>
                        <span className="text-gray-500 dark:text-gray-400">{req.version_spec}</span>
                        <span className="text-gray-300 dark:text-gray-600">→</span>
                        <span className={status.color}>{status.text}</span>
                        <span className="ml-auto">{status.icon}</span>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <p className="text-xs text-gray-400">未检测到运行时需求</p>
              )}
            </div>
          ))}
        </div>
      )}

      {/* Add project form */}
      {showAddForm && (
        <div className="p-4 bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700">
          <div className="flex items-center justify-between mb-3">
            <h4 className="text-sm font-medium text-gray-900 dark:text-gray-100">{'\u6DFB\u52A0\u9879\u76EE'}</h4>
            <button onClick={() => { setShowAddForm(false); setScanResult(null); setPathInput(''); }} className="text-gray-400 hover:text-gray-600">
              <X size={16} />
            </button>
          </div>

          <div className="flex gap-2 mb-3">
            <input
              type="text"
              value={pathInput}
              onChange={(e) => setPathInput(e.target.value)}
              placeholder={'\u8F93\u5165\u9879\u76EE\u6587\u4EF6\u5939\u8DEF\u5F84...'}
              className="flex-1 px-3 py-2 bg-gray-100 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500"
            />
            <button
              onClick={handleScan}
              disabled={scanning || !pathInput.trim()}
              className="flex items-center gap-1 px-3 py-2 bg-gray-200 dark:bg-gray-600 hover:bg-gray-300 dark:hover:bg-gray-500 disabled:opacity-50 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
            >
              {scanning ? <Loader2 size={14} className="animate-spin" /> : <FolderOpen size={14} />}
              {'\u626B\u63CF'}
            </button>
          </div>

          {scanResult && (
            <div className="mb-3 p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
              <p className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2">{'\u626B\u63CF\u7ED3\u679C'}</p>
              {scanResult.requirements.length > 0 ? (
                <div className="space-y-1">
                  {scanResult.requirements.map((req, idx) => (
                    <div key={idx} className="flex items-center gap-2 text-xs">
                      <CheckCircle2 size={12} className="text-green-500" />
                      <span className="text-gray-700 dark:text-gray-300">
                        {'\u68C0\u6D4B\u5230'} {req.source_file} → {req.runtime_type.toUpperCase()} {req.version_spec}
                      </span>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs text-gray-400">{'\u672A\u68C0\u6D4B\u5230\u8FD0\u884C\u65F6\u914D\u7F6E\u6587\u4EF6'}</p>
              )}
              {scanResult.errors.length > 0 && (
                <div className="mt-2 space-y-1">
                  {scanResult.errors.map((err, idx) => (
                    <div key={idx} className="flex items-center gap-2 text-xs text-red-500">
                      <AlertTriangle size={12} />
                      {err}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          <div className="flex justify-end gap-2">
            <button onClick={() => { setShowAddForm(false); setScanResult(null); setPathInput(''); }} className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200">
              {'\u53D6\u6D88'}
            </button>
            <button
              onClick={handleAdd}
              disabled={!scanResult || scanResult.requirements.length === 0}
              className="px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-sm font-medium transition-all"
            >
              {'\u786E\u8BA4\u6DFB\u52A0'}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
