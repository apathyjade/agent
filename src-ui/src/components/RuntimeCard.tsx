import { useState, useRef, useEffect } from 'react';
import { Server, CheckCircle2, Loader2, Download, Trash2, Star, FolderOpen, ChevronDown, ExternalLink } from 'lucide-react';
import type { RuntimeInfo, VersionManager } from '../types';
import { useStore } from '../store';

interface RuntimeCardProps {
  info: RuntimeInfo;
  isInstalling: boolean;
  onInstall: () => void;
  onSwitch: (version: string) => void;
  onUninstall: (version: string) => void;
  onOpenDir?: (version: string) => void;
  availableManagers?: VersionManager[];
  activeManager?: string;
  onSelectManager?: (managerId: string) => void;
}

const RUNTIME_EMOJI: Record<string, string> = {
  node: '⚡', python: '🐍', docker: '🐳', uv: '🦀', go: '🔷',
  rust: '🦀', java: '☕', deno: '🦕', bun: '🥟', ruby: '💎',
};

function ManagerSelector({
  managers,
  activeManager,
  onSelect,
}: {
  managers: VersionManager[];
  activeManager?: string;
  onSelect: (managerId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const { installManagerTool } = useStore();
  const [installing, setInstalling] = useState<string | null>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const active = managers.find((m) => m.id === activeManager) || managers.find((m) => m.id === 'built-in');

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={(e) => { e.stopPropagation(); setOpen(!open); }}
        className="flex items-center gap-1 px-2 py-1 text-[11px] rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-600 dark:text-gray-300 transition-colors"
      >
        <span>{active?.display_name || '管理器'}</span>
        <ChevronDown size={10} className={`transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && (
        <div className="absolute right-0 top-full mt-1 w-64 bg-white dark:bg-gray-800 rounded-xl shadow-lg border border-gray-200 dark:border-gray-700 z-50 py-1 max-h-64 overflow-y-auto">
          {managers.map((m) => {
            const isActive = m.id === (activeManager || 'built-in');
            return (
              <button
                key={m.id}
                onClick={(e) => {
                  e.stopPropagation();
                  if (m.installed) {
                    onSelect(m.id);
                    setOpen(false);
                  }
                }}
                className={`w-full flex items-start gap-2 px-3 py-2 text-left text-xs transition-colors ${
                  isActive
                    ? 'bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-300'
                    : 'hover:bg-gray-50 dark:hover:bg-gray-700/50 text-gray-700 dark:text-gray-300'
                } ${!m.installed && !m.can_install ? 'opacity-60' : ''}`}
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="font-medium truncate">{m.display_name}</span>
                    {m.recommended && (
                      <span className="text-[9px] px-1 py-0.5 rounded-full bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 flex-shrink-0">
                        推荐
                      </span>
                    )}
                    {isActive && (
                      <span className="text-[9px] text-purple-500 flex-shrink-0">当前</span>
                    )}
                  </div>
                  {m.installed && m.version && (
                    <p className="text-[10px] text-gray-400 dark:text-gray-500 mt-0.5 font-mono">{m.version}</p>
                  )}
                  {!m.installed && (
                    <p className="text-[10px] text-gray-400 dark:text-gray-500 mt-0.5">
                      {m.can_install ? '点击安装' : '未安装'}
                    </p>
                  )}
                </div>
                {!m.installed && m.can_install && (
                  <button
                    onClick={async (e) => {
                      e.stopPropagation();
                      if (m.install_url) {
                        setInstalling(m.id);
                        try {
                          await installManagerTool(m.id, m.install_url);
                          setInstalling(null);
                          setOpen(false);
                        } catch {
                          setInstalling(null);
                        }
                      }
                    }}
                    disabled={installing === m.id}
                    className="flex-shrink-0 px-2 py-0.5 bg-purple-600 hover:bg-purple-700 disabled:opacity-50 text-white rounded text-[10px] transition-colors"
                  >
                    {installing === m.id ? '安装中...' : '安装'}
                  </button>
                )}
                {!m.installed && !m.can_install && m.install_guide && (
                  <a
                    href={m.install_guide}
                    target="_blank"
                    rel="noopener noreferrer"
                    onClick={(e) => e.stopPropagation()}
                    className="flex-shrink-0 text-purple-500 hover:text-purple-600"
                  >
                    <ExternalLink size={12} />
                  </a>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

export function RuntimeCard({
  info, isInstalling,
  onInstall, onSwitch, onUninstall, onOpenDir,
  availableManagers, activeManager, onSelectManager,
}: RuntimeCardProps) {
  return (
    <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
      {/* Header */}
      <div className="flex items-center gap-4 p-4">
        <div className={`w-9 h-9 rounded-lg flex items-center justify-center text-lg ${
          isInstalling
            ? 'bg-purple-100 dark:bg-purple-900/30'
            : info.available
              ? 'bg-green-100 dark:bg-green-900/30'
              : 'bg-gray-100 dark:bg-gray-700'
        }`}>
          {isInstalling ? (
            <Loader2 size={18} className="animate-spin text-purple-500" />
          ) : info.available ? (
            <CheckCircle2 size={18} className="text-green-500" />
          ) : (
            <Server size={18} className="text-gray-400" />
          )}
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
              {RUNTIME_EMOJI[info.runtime_type] || '⚙️'} {info.display_name}
            </span>
            {info.available && info.version && (
              <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                {info.version}
              </span>
            )}
          </div>
          {info.available && info.executable_path && (
            <p className="text-[11px] text-gray-400 dark:text-gray-500 truncate mt-0.5">
              {info.executable_path}
            </p>
          )}
          {!info.available && (
            <p className="text-xs text-red-500 dark:text-red-400 mt-0.5">
              {info.error || '未安装'}
            </p>
          )}
        </div>

        <div className="flex items-center gap-2">
          {availableManagers && onSelectManager && (
            <ManagerSelector
              managers={availableManagers}
              activeManager={activeManager}
              onSelect={onSelectManager}
            />
          )}
          <button
            onClick={onInstall}
            disabled={isInstalling}
            className="flex items-center gap-1 px-3 py-1.5 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-xs transition-all"
          >
            <Download size={12} />
            {isInstalling ? '安装中...' : '安装'}
          </button>
        </div>
      </div>

      {/* Installed versions */}
      <div className="px-4 pb-4 border-t border-gray-100 dark:border-gray-700 pt-3 space-y-3">
        <div>
          <h4 className="text-[10px] font-medium text-gray-400 uppercase tracking-wider mb-2">
            已安装版本
          </h4>
          {info.installed_versions && info.installed_versions.length > 0 ? (
            <div className="space-y-1">
              {info.installed_versions.map(v => (
                <div key={v.version} className={`flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs ${
                  v.is_active
                    ? 'bg-purple-50 dark:bg-purple-900/20 border border-purple-200 dark:border-purple-700'
                    : 'bg-gray-50 dark:bg-gray-700/50'
                }`}>
                  <span className="flex-1 font-mono text-gray-700 dark:text-gray-300">{v.version}</span>
                  {v.is_active && (
                    <span className="text-[10px] text-purple-600 dark:text-purple-400 flex items-center gap-0.5">
                      <Star size={10} /> 当前
                    </span>
                  )}
                  {!v.is_active && (
                    <button onClick={() => onSwitch(v.version)} className="text-purple-500 hover:text-purple-600 hover:underline">
                      切换
                    </button>
                  )}
                  {onOpenDir && (
                    <button onClick={() => onOpenDir(v.version)} className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-blue-500 transition-colors" title="打开所在目录">
                      <FolderOpen size={12} />
                    </button>
                  )}
                  <button onClick={() => onUninstall(v.version)} className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-400 hover:text-red-500 transition-colors" title="卸载">
                    <Trash2 size={12} />
                  </button>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-gray-400 py-1">暂无已安装版本</p>
          )}
        </div>
      </div>
    </div>
  );
}
