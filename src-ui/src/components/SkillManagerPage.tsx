import { useEffect, useState } from 'react';
import {
  BrainCircuit,
  Package,
  Plus,
  Settings,
  Trash2,
  Loader2,
  RefreshCw,
  Search,
  Puzzle,
  X,
  FolderOpen,
  Check,
  AlertTriangle,
  ScanLine,
  Download,
  BadgeCheck,
} from 'lucide-react';
import { useStore } from '../store';
import { ManagerPageLayout } from './ManagerPageLayout';
import { SkillDetailPanel } from './SkillDetailPanel';
import type { SkillInfo, MarketSkill, ReconcileResult } from '../types';

type FilterType = 'installed' | 'market';

function SkillIcon() {
  return <Puzzle size={20} />;
}

function SkillCard({
  skill,
  onConfigure,
  onToggle,
  onUninstall,
}: {
  skill: SkillInfo;
  onConfigure: () => void;
  onToggle: (enabled: boolean) => void;
  onUninstall: () => void;
}) {
  const [confirmDelete, setConfirmDelete] = useState(false);

  const sourceLabel = skill.source === 'local' ? '本地' : '注册表';
  const sourceColors: Record<string, string> = {
    local: 'bg-green-100 text-green-600 dark:bg-green-900/40 dark:text-green-400',
    registry: 'bg-purple-100 text-purple-600 dark:bg-purple-900/40 dark:text-purple-400',
  };

  return (
    <div className="group bg-white dark:bg-gray-800/80 rounded-xl border border-gray-100 dark:border-gray-700/60 hover:border-purple-200 dark:hover:border-purple-700/50 hover:shadow-md transition-all duration-200">
      {/* Card Header */}
      <div className="p-4 pb-3 flex items-start gap-3">
        <div className="w-10 h-10 rounded-lg bg-purple-50 dark:bg-purple-900/30 flex items-center justify-center flex-shrink-0 text-purple-600 dark:text-purple-400 group-hover:scale-105 transition-transform">
          <SkillIcon />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
              {skill.name}
            </h3>
            <span className="text-xs text-gray-400 dark:text-gray-500 flex-shrink-0 font-mono">
              v{skill.version}
            </span>
            {skill.author && (
              <span className="text-xs bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 px-1.5 py-0.5 rounded flex-shrink-0">
                {skill.author}
              </span>
            )}
          </div>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-2 leading-relaxed">
            {skill.description}
          </p>
          {/* Tags */}
          {skill.tags && skill.tags.length > 0 && (
            <div className="flex items-center gap-1.5 mt-2 flex-wrap">
              {skill.tags.slice(0, 3).map((tag) => (
                <span
                  key={tag}
                  className="text-[10px] bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 px-1.5 py-0.5 rounded-md"
                >
                  {tag}
                </span>
              ))}
              {skill.tags.length > 3 && (
                <span className="text-[10px] text-gray-400">+{skill.tags.length - 3}</span>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Card Footer */}
      <div className="px-4 py-2.5 border-t border-gray-50 dark:border-gray-700/30 flex items-center justify-between">
        <span
          className={`text-[10px] font-medium px-2 py-0.5 rounded-full ${sourceColors[skill.source] || 'bg-gray-100 text-gray-600'}`}
        >
          {sourceLabel}
        </span>

        <div className="flex items-center gap-1">
          {confirmDelete ? (
            <div className="flex items-center gap-1">
              <button
                onClick={onUninstall}
                className="p-1 rounded-md bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 hover:bg-red-200 dark:hover:bg-red-900/60 transition-colors"
                title="确认卸载"
              >
                <Check size={13} />
              </button>
              <button
                onClick={() => setConfirmDelete(false)}
                className="p-1 rounded-md bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
                title="取消"
              >
                <X size={13} />
              </button>
            </div>
          ) : (
            <button
              onClick={() => setConfirmDelete(true)}
              className="p-1.5 rounded-md text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 transition-colors opacity-0 group-hover:opacity-100"
              title="卸载"
            >
              <Trash2 size={13} />
            </button>
          )}

          <button
            onClick={onConfigure}
            className="p-1.5 rounded-md text-gray-400 hover:text-purple-600 hover:bg-purple-50 dark:hover:bg-purple-900/30 transition-colors"
            title="详情与配置"
          >
            <Settings size={13} />
          </button>

          <label
            className="relative inline-flex items-center cursor-pointer ml-1"
            onClick={(e) => e.stopPropagation()}
          >
            <input
              type="checkbox"
              checked={skill.enabled}
              onChange={(e) => onToggle(e.target.checked)}
              className="sr-only peer"
            />
            <div className="w-9 h-5 bg-gray-200 dark:bg-gray-600 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-purple-600 shadow-sm"></div>
          </label>
        </div>
      </div>
    </div>
  );
}
function MarketSkillCard({
  skill,
  onInstall,
  installing,
  installed,
}: {
  skill: MarketSkill;
  onInstall: () => void;
  installing: boolean;
  installed: boolean;
}) {
  return (
    <div className="group bg-white dark:bg-gray-800/80 rounded-xl border border-gray-100 dark:border-gray-700/60 hover:border-purple-200 dark:hover:border-purple-700/50 hover:shadow-md transition-all duration-200">
      <div className="p-4 pb-3 flex items-start gap-3">
        <div className="w-10 h-10 rounded-lg bg-amber-50 dark:bg-amber-900/30 flex items-center justify-center flex-shrink-0 text-amber-600 dark:text-amber-400 group-hover:scale-105 transition-transform">
          <Download size={20} />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
              {skill.name}
            </h3>
            <span className="text-[10px] bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded-full truncate max-w-[180px]">
              {skill.source}
            </span>
          </div>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-1 leading-relaxed">
            {skill.description}
          </p>
          <div className="flex items-center gap-1.5 mt-2 flex-wrap">
            <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-full bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 flex items-center gap-1">
              <Download size={10} />
              {skill.installs.toLocaleString()}
            </span>
          </div>
        </div>
      </div>

      <div className="px-4 py-2.5 border-t border-gray-50 dark:border-gray-700/30 flex items-center justify-end">
        {installed ? (
          <span className="flex items-center gap-1 text-[11px] text-green-600 dark:text-green-400 font-medium">
            <BadgeCheck size={13} />
            已安装
          </span>
        ) : (
          <button
            onClick={onInstall}
            disabled={installing}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-gradient-to-r from-amber-600 to-orange-600 hover:from-amber-700 hover:to-orange-700 disabled:opacity-50 text-white text-xs font-medium rounded-lg transition-all"
          >
            {installing ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <Download size={12} />
            )}
            {installing ? '安装中...' : '安装'}
          </button>
        )}
      </div>
    </div>
  );
}

function InstallDialog({ onClose }: { onClose: () => void }) {
  const { installSkill, skillLoading, skillError, clearSkillError } = useStore();
  const [path, setPath] = useState('');

  const handleBrowse = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        filters: [{ name: 'Skill 定义', extensions: ['yaml', 'yml'] }],
        multiple: false,
      });
      if (selected) {
        setPath(selected as string);
        clearSkillError();
      }
    } catch (err) {
      clearSkillError();
    }
  };

  const handleInstall = async () => {
    if (!path.trim()) return;
    try {
      await installSkill(path.trim());
      onClose();
    } catch {
      // error is set in store via installSkill
    }
  };

  return (
    <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-800 rounded-2xl w-[480px] shadow-2xl animate-in fade-in zoom-in-95 duration-200">
        <div className="flex items-center justify-between p-5 border-b border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center">
              <Package size={16} className="text-purple-600 dark:text-purple-400" />
            </div>
            <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">安装 Skill</h2>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          >
            <X size={18} />
          </button>
        </div>

        <div className="p-5 space-y-4">
          <div>
            <label className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5 block">
              skill.yaml 文件路径
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={path}
                onChange={(e) => {
                  setPath(e.target.value);
                  clearSkillError();
                }}
                placeholder="C:\path\to\skill.yaml 或 /home/user/skill.yaml"
                className="flex-1 bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
              />
              <button
                onClick={handleBrowse}
                className="flex items-center gap-1.5 px-3 py-2.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
              >
                <FolderOpen size={14} />
                浏览
              </button>
            </div>
            <p className="text-[11px] text-gray-400 dark:text-gray-500 mt-1.5">
              选择一个包含 skill.yaml 定义文件的目录或直接选择 yaml 文件
            </p>
          </div>

          {skillError && (
            <div className="p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
              <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />
              <span>{skillError}</span>
            </div>
          )}

          <div className="flex gap-2 pt-2">
            <button
              onClick={handleInstall}
              disabled={!path.trim() || skillLoading}
              className="flex-1 flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2.5 rounded-lg text-sm transition-all font-medium"
            >
              {skillLoading ? (
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
              onClick={onClose}
              className="px-4 py-2.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg text-sm transition-colors"
            >
              取消
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function SkillManagerPage() {
  const {
    skills,
    fetchSkills,
    toggleSkill,
    uninstallSkill,
    skillLoading,
    installDialogOpen,
    setInstallDialogOpen,
    addToast,
    reconciling,
    reconcileSkills,
    marketSkills,
    marketLoading,
    marketSearching,
    fetchMarketTopSkills,
    searchMarketSkills,
    installMarketSkill,
  } = useStore();

  const [filter, setFilter] = useState<FilterType>('installed');
  const [searchQuery, setSearchQuery] = useState('');
  const [configureSkillId, setConfigureSkillId] = useState<string | null>(null);
  const [showDetailPanel, setShowDetailPanel] = useState(false);
  const [marketSearchQuery, setMarketSearchQuery] = useState('');
  const [installingMarketId, setInstallingMarketId] = useState<string | null>(null);

  useEffect(() => {
    fetchSkills();
  }, [fetchSkills]);

  // Fetch market top skills when switching to market tab
  useEffect(() => {
    if (filter === 'market' && marketSkills.length === 0 && !marketLoading) {
      fetchMarketTopSkills();
    }
  }, [filter, marketSkills.length, marketLoading, fetchMarketTopSkills]);

  const filteredSkills = skills.filter((s) => {
    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      return (
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.id.toLowerCase().includes(q) ||
        (s.tags && s.tags.some((t) => t.toLowerCase().includes(q)))
      );
    }
    return true;
  });

  const handleMarketSearch = async (query: string) => {
    setMarketSearchQuery(query);
    if (query.trim()) {
      await searchMarketSkills(query.trim());
    } else {
      await fetchMarketTopSkills();
    }
  };

  const handleInstallMarket = async (skill: MarketSkill) => {
    setInstallingMarketId(skill.id);
    try {
      await installMarketSkill(skill.source);
      addToast('success', `${skill.name} 已成功安装`);
    } catch (err) {
      addToast('error', `安装 ${skill.name} 失败: ${String(err)}`);
    } finally {
      setInstallingMarketId(null);
    }
  };

  const skillCounts = {
    installed: skills.length,
    market: 0,
  };

  const handleToggle = async (skill: SkillInfo, enabled: boolean) => {
    try {
      await toggleSkill(skill.id, enabled);
      addToast(
        enabled ? 'success' : 'warning',
        `${skill.name} 已${enabled ? '启用' : '禁用'}`
      );
    } catch {
      addToast('error', `切换 ${skill.name} 状态失败`);
    }
  };

  const handleUninstall = async (skill: SkillInfo) => {
    try {
      await uninstallSkill(skill.id);
      addToast('success', `${skill.name} 已卸载`);
    } catch {
      addToast('error', `卸载 ${skill.name} 失败`);
    }
  };

  const handleConfigure = (skill: SkillInfo) => {
    setConfigureSkillId(skill.id);
    setShowDetailPanel(true);
  };

  const handleScan = async () => {
    try {
      const result: ReconcileResult = await reconcileSkills();
      const msgs: string[] = [];
      if (result.added.length > 0) msgs.push(`新增 ${result.added.length} 个技能`);
      if (result.removed.length > 0) msgs.push(`移除 ${result.removed.length} 个失效记录`);
      const msg = msgs.length > 0 ? `同步完成：${msgs.join('，')}` : '数据一致，无需同步';
      addToast('success', msg);
    } catch {
      addToast('error', '扫描同步失败');
    }
  };

  const filterTabs: { key: FilterType; label: string; count: number }[] = [
    { key: 'installed', label: '已安装', count: skillCounts.installed },
    { key: 'market', label: '市场', count: 0 },
  ];

  return (
    <ManagerPageLayout
      icon={<BrainCircuit size={20} className="text-white" />}
      title="技能管理"
      subtitle={`管理本机已安装的 ${skills.length} 个技能`}
      headerActions={
        <>
          <button
            onClick={handleScan}
            disabled={reconciling}
            className="flex items-center gap-1.5 px-3 py-2 bg-indigo-50 dark:bg-indigo-900/30 hover:bg-indigo-100 dark:hover:bg-indigo-900/50 text-indigo-600 dark:text-indigo-400 rounded-lg text-sm transition-colors disabled:opacity-50 border border-indigo-200 dark:border-indigo-800/50"
            title="扫描同步本地技能数据，纠正磁盘与数据库不一致"
          >
            {reconciling ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <ScanLine size={14} />
            )}
            <span className="hidden sm:inline">扫描</span>
          </button>
          <button
            onClick={() => setInstallDialogOpen(true)}
            className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-sm transition-all shadow-sm hover:shadow-md font-medium"
          >
            <Plus size={15} />
            安装 Skill
          </button>
        </>
      }
      searchBar={
        <div className="flex items-center gap-3">
          <div className="relative flex-1 max-w-md">
            <Search
              size={14}
              className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 dark:text-gray-500"
            />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="搜索技能名称、描述或标签..."
              className="w-full bg-gray-50 dark:bg-gray-900/50 border border-gray-200 dark:border-gray-600 rounded-lg pl-9 pr-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery('')}
                className="absolute right-2 top-1/2 -translate-y-1/2 p-0.5 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
              >
                <X size={13} />
              </button>
            )}
          </div>

          <div className="flex items-center gap-1 bg-gray-100 dark:bg-gray-800 rounded-lg p-0.5">
            {filterTabs.map((tab) => (
              <button
                key={tab.key}
                onClick={() => setFilter(tab.key)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                  filter === tab.key
                    ? 'bg-white dark:bg-gray-700 text-purple-700 dark:text-purple-300 shadow-sm'
                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                }`}
              >
                {tab.label}
                {tab.key !== 'market' && (
                  <span
                    className={`text-[10px] px-1.5 py-0.5 rounded-full ${
                      filter === tab.key
                        ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-400'
                        : 'bg-gray-200 dark:bg-gray-600 text-gray-500 dark:text-gray-400'
                    }`}
                  >
                    {tab.count}
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>
      }
    >
        {filter === 'market' ? (
          /* ── Marketplace (skills.sh) ── */
          <div>
            <div className="flex items-center justify-between mb-4">
              <div>
                <p className="text-sm font-medium text-gray-700 dark:text-gray-300">skills.sh 市场</p>
                <p className="text-xs text-gray-400 dark:text-gray-500 mt-0.5">
                  从 skills.sh 市场浏览和安装社区贡献的技能
                </p>
              </div>
              <button
                onClick={() => fetchMarketTopSkills()}
                disabled={marketLoading}
                className="flex items-center gap-1.5 px-3 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-600 dark:text-gray-300 rounded-lg text-sm transition-colors disabled:opacity-50"
                title="刷新"
              >
                <RefreshCw size={14} className={marketLoading ? 'animate-spin' : ''} />
                刷新
              </button>
            </div>

            {/* Market Search */}
            <div className="relative mb-4">
              <Search
                size={14}
                className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 dark:text-gray-500"
              />
              <input
                type="text"
                value={marketSearchQuery}
                onChange={(e) => {
                  setMarketSearchQuery(e.target.value);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    handleMarketSearch(marketSearchQuery);
                  }
                }}
                placeholder="搜索 skills.sh 市场..."
                className="w-full bg-gray-50 dark:bg-gray-900/50 border border-gray-200 dark:border-gray-600 rounded-lg pl-9 pr-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
              />
              {marketSearchQuery && (
                <button
                  onClick={() => handleMarketSearch('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-0.5 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                >
                  <X size={13} />
                </button>
              )}
            </div>

            {/* Market Skills Grid */}
            {marketLoading || marketSearching ? (
              <div className="flex items-center justify-center h-64">
                <div className="flex flex-col items-center gap-3 text-gray-400 dark:text-gray-500">
                  <Loader2 size={28} className="animate-spin text-amber-500" />
                  <span className="text-sm">
                    {marketSearching ? '搜索中...' : '加载市场数据...'}
                  </span>
                </div>
              </div>
            ) : marketSkills.length === 0 ? (
              <div className="flex items-center justify-center h-64">
                <div className="flex flex-col items-center gap-3 text-gray-400 dark:text-gray-500">
                  <Download size={36} className="opacity-40" />
                  <div className="text-center">
                    <p className="text-sm font-medium text-gray-500 dark:text-gray-400">
                      {marketSearchQuery ? '未找到匹配的社区技能' : '暂无市场数据'}
                    </p>
                    <p className="text-xs mt-1">
                      {marketSearchQuery ? '尝试其他关键词' : '请检查网络或稍后重试'}
                    </p>
                  </div>
                </div>
              </div>
            ) : (
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
                {marketSkills.map((skill) => (
                  <MarketSkillCard
                    key={skill.id}
                    skill={skill}
                    onInstall={() => handleInstallMarket(skill)}
                    installing={installingMarketId === skill.id}
                    installed={skills.some((s) => s.name === skill.name)}
                  />
                ))}
              </div>
            )}
          </div>
        ) : skillLoading && skills.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="flex flex-col items-center gap-3 text-gray-400 dark:text-gray-500">
              <Loader2 size={28} className="animate-spin text-purple-500" />
              <span className="text-sm">加载中...</span>
            </div>
          </div>
        ) : filteredSkills.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="flex flex-col items-center gap-3 text-gray-400 dark:text-gray-500">
              <Puzzle size={36} className="opacity-40" />
              {searchQuery ? (
                <div className="text-center">
                  <p className="text-sm font-medium text-gray-500 dark:text-gray-400">未找到匹配的技能</p>
                  <p className="text-xs mt-1">尝试其他关键词或清除搜索</p>
                </div>
              ) : (
                <div className="text-center">
                  <p className="text-sm font-medium text-gray-500 dark:text-gray-400">暂无技能</p>
                  <p className="text-xs mt-1">点击「安装 Skill」按钮添加新技能</p>
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
            {filteredSkills.map((skill) => (
              <SkillCard
                key={skill.id}
                skill={skill}
                onConfigure={() => handleConfigure(skill)}
                onToggle={(enabled) => handleToggle(skill, enabled)}
                onUninstall={() => handleUninstall(skill)}
              />
            ))}
          </div>
        )}

      {installDialogOpen && (
        <InstallDialog onClose={() => setInstallDialogOpen(false)} />
      )}

      {showDetailPanel && configureSkillId && (
        <SkillDetailPanel
          skillId={configureSkillId}
          onClose={() => {
            setShowDetailPanel(false);
            setConfigureSkillId(null);
            fetchSkills();
          }}
        />
      )}
    </ManagerPageLayout>
  );
}
