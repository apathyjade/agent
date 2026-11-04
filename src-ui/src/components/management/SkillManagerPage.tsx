import { useEffect, useMemo, useState } from 'react';
import {
  BrainCircuit,
  Puzzle,
  Plus,
  RefreshCw,
  Search,
  X,
  Loader2,
  AlertTriangle,
  Check,
  Trash2,
  Eye,
  RotateCcw,
  ExternalLink,
  Globe,
  FolderKanban,
  Download,
  BadgeCheck,
} from 'lucide-react';
import { useStore } from '../../store';
import { ManagerPageLayout } from '../common/ManagerPageLayout';
import { SkillDetailPanel } from './SkillDetailPanel';
import { InstallSkillDrawer } from './InstallSkillDrawer';
import { LegacySkillsBanner } from './LegacySkillsBanner';
import { UpdatePanel } from './UpdatePanel';
import { filterInstalled, isInstalled, agentIdsFromNames } from './skillUtils';
import { listProjects } from '../../api/management';
import type { InstalledSkill, MarketSkill, Project, SkillScope } from '../../types';

type TabKey = 'installed' | 'market' | 'updates';

function SkillIcon() {
  return <Puzzle size={20} />;
}

function InstalledSkillCard({
  skill,
  onView,
  onRemove,
  onReinstall,
}: {
  skill: InstalledSkill;
  onView: () => void;
  onRemove: () => void;
  onReinstall: () => void;
}) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const isMissing = skill.status === 'missing';

  return (
    <div
      className={`group rounded-xl border transition-all duration-200 ${
        isMissing
          ? 'bg-red-50/50 dark:bg-red-900/10 border-red-200 dark:border-red-800/50'
          : 'bg-white dark:bg-gray-800/80 border-gray-100 dark:border-gray-700/60 hover:border-purple-200 dark:hover:border-purple-700/50 hover:shadow-md'
      }`}
    >
      <div className="p-4 pb-3 flex items-start gap-3">
        <div
          className={`w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0 transition-transform ${
            isMissing
              ? 'bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400'
              : 'bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 group-hover:scale-105'
          }`}
        >
          <SkillIcon />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <h3 className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
              {skill.name}
            </h3>
            {skill.version && (
              <span className="text-xs text-gray-400 dark:text-gray-500 flex-shrink-0 font-mono">
                v{skill.version}
              </span>
            )}
            {isMissing && (
              <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 flex-shrink-0">
                缺失
              </span>
            )}
          </div>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-2 leading-relaxed">
            {skill.description}
          </p>
          <div className="flex flex-wrap gap-1.5 mt-2">
            {skill.agents.map((agent) => (
              <span
                key={agent}
                className="text-[10px] px-1.5 py-0.5 rounded-md bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
              >
                {agent}
              </span>
            ))}
          </div>
        </div>
      </div>

      <div className="px-4 py-2.5 border-t border-gray-50 dark:border-gray-700/30 flex items-center justify-between">
        <div className="min-w-0 flex flex-col">
          {skill.source && (
            <a
              href={`https://skills.sh/${skill.source}`}
              target="_blank"
              rel="noreferrer"
              onClick={(e) => e.stopPropagation()}
              className="inline-flex items-center gap-1 text-[10px] text-purple-600 dark:text-purple-400 hover:underline truncate max-w-[200px]"
            >
              <ExternalLink size={10} />
              {skill.source}
            </a>
          )}
          {skill.path && (
            <p className="text-[10px] text-gray-400 dark:text-gray-500 truncate max-w-[200px]">
              {skill.path}
            </p>
          )}
        </div>

        <div className="flex items-center gap-1 flex-shrink-0">
          {isMissing ? (
            <>
              <button
                onClick={onReinstall}
                className="flex items-center gap-1 px-2 py-1 text-xs font-medium text-purple-600 dark:text-purple-400 hover:bg-purple-50 dark:hover:bg-purple-900/30 rounded-md transition-colors"
              >
                <RotateCcw size={12} />
                重新安装
              </button>
              <button
                onClick={onRemove}
                className="flex items-center gap-1 px-2 py-1 text-xs font-medium text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-md transition-colors"
              >
                <Trash2 size={12} />
                移除
              </button>
            </>
          ) : confirmRemove ? (
            <div className="flex items-center gap-1">
              <button
                onClick={onRemove}
                className="p-1 rounded-md bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 hover:bg-red-200 dark:hover:bg-red-900/60 transition-colors"
                title="确认移除"
              >
                <Check size={13} />
              </button>
              <button
                onClick={() => setConfirmRemove(false)}
                className="p-1 rounded-md bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
                title="取消"
              >
                <X size={13} />
              </button>
            </div>
          ) : (
            <>
              <button
                onClick={() => setConfirmRemove(true)}
                className="p-1.5 rounded-md text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 transition-colors opacity-0 group-hover:opacity-100"
                title="移除"
              >
                <Trash2 size={13} />
              </button>
              <button
                onClick={onView}
                className="flex items-center gap-1 px-2 py-1 text-xs font-medium text-purple-600 dark:text-purple-400 hover:bg-purple-50 dark:hover:bg-purple-900/30 rounded-md transition-colors"
              >
                <Eye size={12} />
                查看
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function MarketSkillCard({
  skill,
  installing,
  installed,
  onInstall,
}: {
  skill: MarketSkill;
  installing: boolean;
  installed: boolean;
  onInstall: () => void;
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
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-2 leading-relaxed">
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

export function SkillManagerPage() {
  const {
    cliStatus,
    agents,
    scope,
    projectPath,
    installed,
    legacySkills,
    marketSkills,
    skillLoading,
    busy,
    skillError,
    fetchEnv,
    fetchAgents,
    fetchInstalled,
    fetchLegacy,
    fetchMarketTopSkills,
    searchMarketSkills,
    installSkill,
    removeSkill,
    setScope,
    setProjectPath,
    clearSkillError,
    importLegacy,
  } = useStore();

  const [activeTab, setActiveTab] = useState<TabKey>('installed');
  const [searchQuery, setSearchQuery] = useState('');
  const [agentFilter, setAgentFilter] = useState<string | null>(null);
  const [selectedSkill, setSelectedSkill] = useState<InstalledSkill | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [drawerInitialSource, setDrawerInitialSource] = useState<string | undefined>(undefined);
  const [projects, setProjects] = useState<Project[]>([]);
  const [marketSearch, setMarketSearch] = useState('');

  useEffect(() => {
    fetchEnv();
    fetchAgents();
    fetchInstalled();
    fetchLegacy();
    listProjects().then(setProjects).catch(() => setProjects([]));
  }, [fetchEnv, fetchAgents, fetchInstalled, fetchLegacy]);

  useEffect(() => {
    if (activeTab === 'market' && marketSkills.length === 0) {
      fetchMarketTopSkills();
    }
  }, [activeTab, marketSkills.length, fetchMarketTopSkills]);

  const availableAgents = useMemo(() => {
    const set = new Set<string>();
    installed.forEach((s) => s.agents?.forEach((a) => set.add(a)));
    return Array.from(set).sort();
  }, [installed]);

  const filteredInstalled = useMemo(
    () => filterInstalled(installed, searchQuery, agentFilter),
    [installed, searchQuery, agentFilter]
  );

  const handleRefresh = () => {
    clearSkillError();
    fetchEnv();
    fetchAgents();
    fetchInstalled();
    fetchLegacy();
    if (activeTab === 'market') fetchMarketTopSkills();
    listProjects().then(setProjects).catch(() => setProjects([]));
  };

  const handleScopeChange = (next: SkillScope) => {
    setScope(next);
  };

  const handleProjectChange = (path: string) => {
    setProjectPath(path || null);
  };

  const handleMarketSearch = async (q: string) => {
    setMarketSearch(q);
    if (q.trim()) {
      await searchMarketSkills(q.trim());
    } else {
      await fetchMarketTopSkills();
    }
  };

  const handleInstallMarket = (skill: MarketSkill) => {
    setDrawerInitialSource(skill.source);
    setDrawerOpen(true);
  };

  const handleRemove = async (skill: InstalledSkill) => {
    try {
      await removeSkill(skill.name);
      if (selectedSkill?.name === skill.name) setSelectedSkill(null);
    } catch {
      // error handled in store
    }
  };

  const handleReinstall = async (skill: InstalledSkill) => {
    const source = skill.source_url || skill.source || '';
    if (!source) return;
    try {
      await installSkill({
        source,
        skills: [skill.name],
        agents: agentIdsFromNames(skill.agents, agents),
        scope: skill.scope === 'project' ? 'project' : 'global',
        projectPath,
        copy: false,
      });
      if (selectedSkill?.name === skill.name) setSelectedSkill(null);
    } catch {
      // error handled in store
    }
  };

  const isCliUnavailable = cliStatus?.available === false;

  const tabs = [
    { key: 'installed' as TabKey, label: '已安装' },
    { key: 'market' as TabKey, label: '市场' },
    { key: 'updates' as TabKey, label: '更新' },
  ];

  const renderInstalledTab = () => {
    if (isCliUnavailable) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-center px-6 py-12">
          <div className="w-14 h-14 rounded-2xl bg-red-50 dark:bg-red-900/20 flex items-center justify-center mb-4">
            <AlertTriangle size={28} className="text-red-500" />
          </div>
          <h3 className="text-base font-medium text-gray-900 dark:text-gray-100 mb-1">
            技能 CLI 不可用
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400 max-w-md mb-4">
            {cliStatus?.error || '无法检测到 skills 命令。请确认已安装 Node ≥ 22.20 并且 npx 可用。'}
          </p>
          <button
            onClick={handleRefresh}
            className="flex items-center gap-1.5 px-4 py-2 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 text-gray-700 dark:text-gray-200 rounded-lg text-sm transition-colors"
          >
            <RefreshCw size={14} />
            重新检测
          </button>
        </div>
      );
    }

    if (scope === 'project' && !projectPath) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-center px-6 py-12">
          <div className="w-14 h-14 rounded-2xl bg-purple-50 dark:bg-purple-900/20 flex items-center justify-center mb-4">
            <FolderKanban size={28} className="text-purple-500" />
          </div>
          <h3 className="text-base font-medium text-gray-900 dark:text-gray-100 mb-1">
            选择项目
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            切换到「项目」范围后，请选择一个项目以查看该项目的技能。
          </p>
        </div>
      );
    }

    if (skillLoading && installed.length === 0) {
      return (
        <div className="flex items-center justify-center h-full">
          <Loader2 size={28} className="animate-spin text-purple-500" />
        </div>
      );
    }

    if (filteredInstalled.length === 0) {
      return (
        <div className="flex flex-col items-center justify-center h-full text-center px-6 py-12">
          <Puzzle size={36} className="text-gray-300 dark:text-gray-600 mb-3" />
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {searchQuery || agentFilter ? '没有匹配的技能' : '暂无已安装技能'}
          </p>
        </div>
      );
    }

    return (
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
        {filteredInstalled.map((skill) => (
          <InstalledSkillCard
            key={`${skill.name}-${skill.scope}`}
            skill={skill}
            onView={() => setSelectedSkill(skill)}
            onRemove={() => handleRemove(skill)}
            onReinstall={() => handleReinstall(skill)}
          />
        ))}
      </div>
    );
  };

  const renderMarketTab = () => {
    const marketLoading = busy === 'market';
    return (
      <div className="space-y-4">
        <div className="relative">
          <Search
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 dark:text-gray-500"
          />
          <input
            type="text"
            value={marketSearch}
            onChange={(e) => setMarketSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleMarketSearch(marketSearch);
            }}
            placeholder="搜索 skills.sh 市场..."
            className="w-full bg-gray-50 dark:bg-gray-900/50 border border-gray-200 dark:border-gray-600 rounded-lg pl-9 pr-8 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
          />
          {marketSearch && (
            <button
              onClick={() => handleMarketSearch('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 p-0.5 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            >
              <X size={13} />
            </button>
          )}
        </div>

        {marketLoading ? (
          <div className="flex items-center justify-center h-64">
            <Loader2 size={28} className="animate-spin text-amber-500" />
          </div>
        ) : marketSkills.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-64 text-center">
            <Globe size={36} className="text-gray-300 dark:text-gray-600 mb-3" />
            <p className="text-sm text-gray-500 dark:text-gray-400">
              {marketSearch ? '未找到匹配的社区技能' : '暂无市场数据'}
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
            {marketSkills.map((skill) => (
              <MarketSkillCard
                key={skill.id}
                skill={skill}
                installing={false}
                installed={isInstalled(skill.name, installed)}
                onInstall={() => handleInstallMarket(skill)}
              />
            ))}
          </div>
        )}
      </div>
    );
  };

  return (
    <ManagerPageLayout
      icon={<BrainCircuit size={20} className="text-white" />}
      title="技能管理"
      subtitle="管理本机已安装的技能"
      tabs={tabs}
      activeTab={activeTab}
      onTabChange={(key) => setActiveTab(key as TabKey)}
      headerActions={
        <>
          <button
            onClick={handleRefresh}
            disabled={skillLoading || busy === 'market'}
            className="flex items-center gap-1.5 px-3 py-2 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 hover:border-purple-300 dark:hover:border-purple-700 text-gray-700 dark:text-gray-200 rounded-lg text-sm transition-colors disabled:opacity-50"
            title="刷新"
          >
            <RefreshCw size={14} className={skillLoading || busy === 'market' ? 'animate-spin' : ''} />
            刷新
          </button>
          <button
            onClick={() => setDrawerOpen(true)}
            className="flex items-center gap-1.5 px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white rounded-lg text-sm transition-all shadow-sm hover:shadow-md font-medium"
          >
            <Plus size={15} />
            安装技能
          </button>
        </>
      }
      searchBar={
        activeTab === 'installed' ? (
          <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-3">
            <div className="relative flex-1 max-w-md">
              <Search
                size={14}
                className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 dark:text-gray-500"
              />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="搜索技能名称、描述或 agent..."
                className="w-full bg-gray-50 dark:bg-gray-900/50 border border-gray-200 dark:border-gray-600 rounded-lg pl-9 pr-8 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-500"
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

            <div className="flex items-center gap-2">
              <div className="flex items-center gap-1 p-1 bg-gray-100 dark:bg-gray-800 rounded-lg">
                <button
                  onClick={() => handleScopeChange('global')}
                  className={`flex items-center gap-1 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                    scope === 'global'
                      ? 'bg-white dark:bg-gray-700 text-purple-700 dark:text-purple-300 shadow-sm'
                      : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                  }`}
                >
                  <Globe size={12} />
                  全局
                </button>
                <button
                  onClick={() => handleScopeChange('project')}
                  className={`flex items-center gap-1 px-3 py-1.5 rounded-md text-xs font-medium transition-all ${
                    scope === 'project'
                      ? 'bg-white dark:bg-gray-700 text-purple-700 dark:text-purple-300 shadow-sm'
                      : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                  }`}
                >
                  <FolderKanban size={12} />
                  项目
                </button>
              </div>

              {scope === 'project' && (
                <select
                  value={projectPath || ''}
                  onChange={(e) => handleProjectChange(e.target.value)}
                  className="bg-gray-50 dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-xs focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100"
                >
                  <option value="">选择项目...</option>
                  {projects.map((p) => (
                    <option key={p.id} value={p.path}>
                      {p.name}
                    </option>
                  ))}
                </select>
              )}
            </div>

            {availableAgents.length > 0 && (
              <div className="flex items-center gap-1.5 flex-wrap">
                <button
                  onClick={() => setAgentFilter(null)}
                  className={`px-2.5 py-1 rounded-full text-xs transition-colors border ${
                    agentFilter === null
                      ? 'bg-purple-100 dark:bg-purple-900/50 border-purple-300 dark:border-purple-700 text-purple-800 dark:text-purple-200'
                      : 'bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300 hover:border-purple-300'
                  }`}
                >
                  全部
                </button>
                {availableAgents.map((agent) => (
                  <button
                    key={agent}
                    onClick={() => setAgentFilter(agentFilter === agent ? null : agent)}
                    className={`px-2.5 py-1 rounded-full text-xs transition-colors border ${
                      agentFilter === agent
                        ? 'bg-purple-100 dark:bg-purple-900/50 border-purple-300 dark:border-purple-700 text-purple-800 dark:text-purple-200'
                        : 'bg-white dark:bg-gray-800 border-gray-200 dark:border-gray-600 text-gray-600 dark:text-gray-300 hover:border-purple-300'
                    }`}
                  >
                    {agent}
                  </button>
                ))}
              </div>
            )}
          </div>
        ) : undefined
      }
    >
      {skillError && (
        <div className="mb-4 p-3 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg text-sm text-red-700 dark:text-red-400 flex items-start gap-2">
          <AlertTriangle size={14} className="flex-shrink-0 mt-0.5" />
          <span>{skillError}</span>
        </div>
      )}
      {activeTab === 'installed' && (
        <>
          {legacySkills.length > 0 && (
            <LegacySkillsBanner
              skills={legacySkills}
              agents={agents}
              onImport={importLegacy}
            />
          )}
          {renderInstalledTab()}
        </>
      )}
      {activeTab === 'market' && renderMarketTab()}
      {activeTab === 'updates' && <UpdatePanel />}

      {drawerOpen && (
        <InstallSkillDrawer
          open={drawerOpen}
          initialSource={drawerInitialSource}
          onClose={() => {
            setDrawerOpen(false);
            setDrawerInitialSource(undefined);
          }}
        />
      )}

      {selectedSkill && (
        <SkillDetailPanel skill={selectedSkill} onClose={() => setSelectedSkill(null)} />
      )}
    </ManagerPageLayout>
  );
}
