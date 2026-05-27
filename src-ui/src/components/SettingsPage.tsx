import { useState, useEffect } from 'react';
import { Eye, EyeOff, Check, FileText, Cpu, MessageSquare, ChevronRight, Star, Trash2, Plus, Sparkles, BrainCircuit, Settings as SettingsIcon, Server, FolderOpen, Globe } from 'lucide-react';
import { Row } from '@jelper/component';
import { Select, Switch, InputNumber } from 'antd';
import { useStore } from '../store';
import * as api from '../api/tauri';
import { ManagerPageLayout } from './ManagerPageLayout';
import { SkillDetailPanel } from './SkillDetailPanel';
import { SkillInstallDialog } from './SkillInstallDialog';
import { DirectoryPicker } from './DirectoryPicker';

export function SettingsPage() {
  const [activeTab, setActiveTab] = useState<'providers' | 'prompts' | 'skills' | 'runtime' | 'conversation'>('providers');
  const {
    providers, fetchProviders, setupProvider, updateProviderConfig, removeProvider,
    skills, fetchSkills, toggleSkill,
    systemPrompts, fetchSystemPrompts, deleteSystemPrompt, setDefaultSystemPrompt,
    models, fetchModels, defaultModel, setDefaultModel,
    installDir, fetchInstallDir, setInstallDir,
    lifecycleConfig, updateLifecycleConfig, fetchLifecycleConfig,
  } = useStore();

  const [selectedProvider, setSelectedProvider] = useState<string | null>(null);
  const [showApiKey, setShowApiKey] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const [, setEditApiKey] = useState('');
  const [, setEditBaseUrl] = useState('');
  const [, setEditModels] = useState<string[]>([]);
  const [isEditing, setIsEditing] = useState(false);

  const [newApiKey, setNewApiKey] = useState('');
  const [newBaseUrl, setNewBaseUrl] = useState('');
  const [newModels, setNewModels] = useState<string[]>([]);
  const [showAddForm, setShowAddForm] = useState(false);

  const [selectedSkillId, setSelectedSkillId] = useState<string | null>(null);
  const [showSkillDetail, setShowSkillDetail] = useState(false);
  const [showSkillInstall, setShowSkillInstall] = useState(false);

  const [editingDir, setEditingDir] = useState(false);
  const [dirValue, setDirValue] = useState('');

  const [proxyUrl, setProxyUrl] = useState('');
  const [savedProxyUrl, setSavedProxyUrl] = useState('');

  const getSetting = async (key: string): Promise<string | null> => {
    try {
      const settings = await api.getSettings();
      return settings[key] || null;
    } catch {
      return null;
    }
  };

  const saveProxyUrl = async () => {
    try {
      await api.updateSettings('download_proxy', proxyUrl);
      setSavedProxyUrl(proxyUrl);
    } catch {
      // ignore
    }
  };

  const loadProxySetting = async () => {
    const val = await getSetting('download_proxy');
    setProxyUrl(val || '');
    setSavedProxyUrl(val || '');
  };

  useEffect(() => {
    fetchProviders();
    fetchSystemPrompts();
    fetchModels();
    fetchSkills();
    fetchInstallDir();
    loadProxySetting();
    fetchLifecycleConfig();
  }, []);

  const selected = providers.find(p => p.id === selectedProvider);

  const handleSelectProvider = (provider: typeof providers[0]) => {
    setSelectedProvider(provider.id);
    setIsEditing(false);
    setShowAddForm(false);
    setSaveError(null);
    if (provider.configured) {
      setEditApiKey(provider.configured ? '••••••••' : '');
      setEditBaseUrl(provider.base_url || '');
      setEditModels(provider.enabled_models);
    }
  };

  const handleSaveEdit = async () => {
    if (!selectedProvider) return;
    await updateProviderConfig({
      provider: selectedProvider,
      apiKey: newApiKey ? newApiKey : undefined,
      baseUrl: newBaseUrl || undefined,
      enabledModels: newModels.length > 0 ? newModels : undefined,
    });
    await fetchProviders();
    await fetchModels();
    setIsEditing(false);
    setNewApiKey('');
  };

  const handleSetupProvider = async () => {
    if (!selectedProvider || newModels.length === 0) return;
    setSaveError(null);
    try {
      await setupProvider({
        provider: selectedProvider,
        apiKey: newApiKey,
        baseUrl: newBaseUrl || undefined,
        enabledModels: newModels,
      });
      await fetchProviders();
      await fetchModels();
      setShowAddForm(false);
      setNewApiKey('');
      setNewBaseUrl('');
      setNewModels([]);
    } catch (err) {
      setSaveError(String(err));
    }
  };

  const handleStartSetup = (provider: typeof providers[0]) => {
    setSelectedProvider(provider.id);
    setShowAddForm(true);
    setIsEditing(false);
    setNewApiKey('');
    setNewBaseUrl(provider.configured ? provider.base_url || '' : '');
    setNewModels(provider.available_models.map(m => m.id));
  };

  const toggleModel = (modelId: string, checked: boolean) => {
    if (isEditing || showAddForm) {
      setNewModels(prev => checked ? [...prev, modelId] : prev.filter(m => m !== modelId));
    }
  };

  const getProviderColor = (providerId: string) => {
    const colors: Record<string, string> = {
      openai: 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400',
      anthropic: 'bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400',
      google: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400',
      groq: 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400',
      deepseek: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400',
      zhipu: 'bg-cyan-100 text-cyan-700 dark:bg-cyan-900/30 dark:text-cyan-400',
      moonshot: 'bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400',
      siliconflow: 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/30 dark:text-indigo-400',
      ollama: 'bg-gray-100 text-gray-700 dark:bg-gray-700/60 dark:text-gray-300',
      lmstudio: 'bg-pink-100 text-pink-700 dark:bg-pink-900/30 dark:text-pink-400',
    };
    return colors[providerId] || 'bg-gray-100 text-gray-700 dark:bg-gray-700/60 dark:text-gray-300';
  };

  // Provider metadata for Rig-backed configuration hints
  const providerHints: Record<string, { hint: string; docs?: string }> = {
    openai: { hint: '使用 OPENAI_API_KEY 环境变量或手动输入密钥' },
    anthropic: { hint: '使用 ANTHROPIC_API_KEY 环境变量或手动输入密钥' },
    google: { hint: '使用 GEMINI_API_KEY 环境变量或手动输入密钥' },
    groq: { hint: '使用 GROQ_API_KEY 环境变量或手动输入密钥' },
    deepseek: { hint: '使用 DEEPSEEK_API_KEY 环境变量或手动输入密钥' },
    zhipu: { hint: '智谱开放平台 API Key，如使用自定义网关请修改 Base URL' },
    moonshot: { hint: '月之暗面开放平台 API Key' },
    siliconflow: { hint: '硅基流动 API Key，支持多种开源模型' },
    ollama: { hint: '本地 Ollama 服务，无需 API Key', docs: 'http://localhost:11434' },
    lmstudio: { hint: '本地 LM Studio 服务，无需 API Key', docs: 'http://localhost:1234' },
    custom: { hint: '自定义 OpenAI 兼容 API，可设置任意 Base URL' },
  };

  const tabs = [
    { id: 'providers' as const, icon: <Cpu size={16} />, label: '模型提供商' },
    { id: 'prompts' as const, icon: <FileText size={16} />, label: '提示词' },
    { id: 'skills' as const, icon: <BrainCircuit size={16} />, label: '技能' },
    { id: 'runtime' as const, icon: <Server size={16} />, label: '运行环境' },
    { id: 'conversation' as const, icon: <MessageSquare size={16} />, label: '对话管理' },
  ];

  const renderTabContent = () => {
    switch (activeTab) {
      case 'providers': return renderProvidersTab();
      case 'skills': return renderSkillsTab();
      case 'prompts': return renderPromptsTab();
      case 'runtime': return renderRuntimeTab();
      case 'conversation': return renderConversationTab();
    }
  };

  // ── Providers Tab ──

  function renderProvidersTab() {
    return (
      <>
        <div className="mb-4 p-4 bg-purple-50 dark:bg-purple-900/10 border border-purple-100 dark:border-purple-800/30 rounded-xl">
          <div className="flex items-center gap-2 mb-3">
            <Sparkles size={16} className="text-purple-600 dark:text-purple-400" />
            <h3 className="text-sm font-medium text-purple-900 dark:text-purple-300">默认模型</h3>
          </div>
          <div className="flex items-center gap-3">
            <Select
              value={defaultModel || undefined}
              onChange={(v) => { if (v) setDefaultModel(v); }}
              className="flex-1"
              placeholder="选择默认模型..."
              options={models.filter(m => m.enabled).map(m => ({
                value: m.id,
                label: `${m.display_name} (${m.provider})`
              }))}
            />
            {defaultModel && (
              <span className="text-xs bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-400 px-2 py-1 rounded-full whitespace-nowrap">
                当前: {models.find(m => m.id === defaultModel)?.display_name || defaultModel}
              </span>
            )}
          </div>
          {models.filter(m => m.enabled).length === 0 && (
            <p className="text-xs text-red-500 mt-2">请先配置模型提供商并启用至少一个模型</p>
          )}
        </div>

        <Row $gap={16}>
          <Row.Item $width={256} $fixed>
            <div className="space-y-1">
              {providers.map((provider) => (
                <button
                  key={provider.id}
                  onClick={() => handleSelectProvider(provider)}
                  className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-left transition-all ${
                    selectedProvider === provider.id
                      ? 'bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300'
                      : 'hover:bg-gray-50 dark:hover:bg-gray-700/50 text-gray-700 dark:text-gray-300'
                  }`}
                >
                  <span className={`text-xs px-2 py-0.5 rounded-full ${getProviderColor(provider.id)}`}>
                    {provider.name}
                  </span>
                  <div className="flex-1" />
                  {provider.configured ? (
                    <Check size={14} className="text-green-500" />
                  ) : (
                    <ChevronRight size={14} className="text-gray-400" />
                  )}
                </button>
              ))}
            </div>
          </Row.Item>

          <Row.Item $scale={1}>
            {selected && !showAddForm && !isEditing && (
              <div className="space-y-4">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className={`text-sm px-3 py-1 rounded-full ${getProviderColor(selected.id)}`}>
                    {selected.name}
                  </span>
                  {selected.configured ? (
                    <span className="text-xs bg-green-100 dark:bg-green-900/40 text-green-600 dark:text-green-400 px-2 py-0.5 rounded-full">已配置</span>
                  ) : (
                    <span className="text-xs bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 px-2 py-0.5 rounded-full">未配置</span>
                  )}
                  {/* Rig backend badge */}
                  <span className="flex items-center gap-1 text-[11px] font-medium text-indigo-500 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-900/30 px-2 py-0.5 rounded-full">
                    <Sparkles size={11} />
                    Rig
                  </span>
                </div>

                {/* Provider-specific hint */}
                {providerHints[selected.id] && (
                  <p className="text-xs text-gray-500 dark:text-gray-400 flex items-start gap-1.5">
                    <span className="text-gray-300 dark:text-gray-600 mt-0.5">ℹ</span>
                    {providerHints[selected.id].hint}
                  </p>
                )}

                {selected.configured && (
                  <>
                    <div>
                      <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">已启用模型</label>
                      <div className="space-y-1">
                        {selected.available_models.map((model) => (
                          <div key={model.id} className="flex items-center gap-2 px-3 py-1.5 rounded bg-gray-50 dark:bg-gray-700/50">
                            <Check size={14} className={`flex-shrink-0 ${selected.enabled_models.includes(model.id) ? 'text-green-500' : 'text-gray-300 dark:text-gray-600'}`} />
                            <span className="text-sm text-gray-700 dark:text-gray-300">{model.name}</span>
                            <span className="text-xs text-gray-400 dark:text-gray-500 ml-auto">{model.id}</span>
                            {model.context_window && (
                              <span className="text-xs text-gray-400">{(model.context_window / 1000).toFixed(0)}K</span>
                            )}
                          </div>
                        ))}
                      </div>
                    </div>
                    <div className="flex gap-2 pt-2">
                      <button
                        onClick={() => {
                          setIsEditing(true);
                          setNewApiKey('');
                          setNewBaseUrl(selected.base_url || '');
                          setNewModels(selected.available_models.map(m => m.id));
                        }}
                        className="flex-1 bg-purple-600 hover:bg-purple-700 text-white px-4 py-2 rounded-lg text-sm transition-colors"
                      >
                        编辑配置
                      </button>
                      <button
                        onClick={() => removeProvider(selected.id)}
                        className="px-4 py-2 bg-red-50 hover:bg-red-100 text-red-600 rounded-lg text-sm transition-colors"
                      >
                        移除
                      </button>
                    </div>
                  </>
                )}

                {!selected.configured && (
                  <button
                    onClick={() => handleStartSetup(selected)}
                    className="w-full bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white px-4 py-2.5 rounded-lg text-sm transition-all"
                  >
                    配置此提供商
                  </button>
                )}
              </div>
            )}

            {selected && showAddForm && (
              <div className="space-y-3">
                <div className="flex items-center gap-2 mb-1">
                  <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">配置 {selected.name}</h3>
                  <span className="flex items-center gap-1 text-[11px] font-medium text-indigo-500 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-900/30 px-2 py-0.5 rounded-full">
                    <Sparkles size={11} />
                    Rig
                  </span>
                </div>

                {/* Provider hint */}
                {providerHints[selected.id] && (
                  <p className="text-xs text-gray-500 dark:text-gray-400">{providerHints[selected.id].hint}</p>
                )}

                {selected.requires_api_key && (
                  <div>
                    <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">API Key</label>
                    <div className="relative">
                      <input
                        type={showApiKey ? 'text' : 'password'}
                        value={newApiKey}
                        onChange={(e) => setNewApiKey(e.target.value)}
                        placeholder="sk-..."
                        className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 pr-10 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400"
                      />
                      <button
                        onClick={() => setShowApiKey(!showApiKey)}
                        className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
                      >
                        {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
                      </button>
                    </div>
                  </div>
                )}
                <div>
                  <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">Base URL</label>
                  <input
                    type="text"
                    value={newBaseUrl}
                    onChange={(e) => setNewBaseUrl(e.target.value)}
                    placeholder={selected.base_url || 'https://api.openai.com/v1/chat/completions'}
                    className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-sm font-mono text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500"
                  />
                  {selected.base_url && (
                    <p className="text-[11px] text-gray-400 dark:text-gray-500 mt-1">
                      默认: <span className="font-mono">{selected.base_url}</span>
                    </p>
                  )}
                </div>
                <div>
                  <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">选择模型</label>
                  <div className="space-y-1 max-h-48 overflow-y-auto">
                    {selected.available_models.map((model) => (
                      <label key={model.id} className="flex items-center gap-2 px-3 py-1.5 rounded hover:bg-gray-50 dark:hover:bg-gray-700/50 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={newModels.includes(model.id)}
                          onChange={(e) => toggleModel(model.id, e.target.checked)}
                          className="rounded border-gray-300 dark:border-gray-600 text-purple-600 focus:ring-purple-500"
                        />
                        <span className="text-sm text-gray-700 dark:text-gray-300">{model.name}</span>
                        <span className="text-xs text-gray-400 dark:text-gray-500 ml-auto">{model.id}</span>
                      </label>
                    ))}
                  </div>
                </div>
                {saveError && (
                  <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700">{saveError}</div>
                )}
                <div className="flex gap-2 pt-2">
                  <button
                    onClick={handleSetupProvider}
                    disabled={selected.requires_api_key && !newApiKey.trim() || newModels.length === 0}
                    className="flex-1 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2 rounded-lg text-sm transition-all"
                  >
                    保存配置
                  </button>
                  <button
                    onClick={() => { setShowAddForm(false); setSelectedProvider(null); setSaveError(null); }}
                    className="px-4 py-2 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors"
                  >
                    取消
                  </button>
                </div>
              </div>
            )}

            {selected && isEditing && (
              <div className="space-y-3">
                <div className="flex items-center gap-2 mb-1">
                  <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">编辑 {selected.name}</h3>
                  <span className="flex items-center gap-1 text-[11px] font-medium text-indigo-500 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-900/30 px-2 py-0.5 rounded-full">
                    <Sparkles size={11} />
                    Rig
                  </span>
                </div>
                {selected.requires_api_key && (
                  <div>
                    <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">API Key (留空保持不变)</label>
                    <div className="relative">
                      <input
                        type={showApiKey ? 'text' : 'password'}
                        value={newApiKey}
                        onChange={(e) => setNewApiKey(e.target.value)}
                        placeholder="留空保持不变"
                        className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 pr-10 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400"
                      />
                      {newApiKey && (
                        <button onClick={() => setShowApiKey(!showApiKey)} className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600">
                          {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
                        </button>
                      )}
                    </div>
                  </div>
                )}
                <div>
                  <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">Base URL</label>
                  <input
                    type="text"
                    value={newBaseUrl}
                    onChange={(e) => setNewBaseUrl(e.target.value)}
                    placeholder={selected.base_url || '使用默认地址'}
                    className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-sm font-mono text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500"
                  />
                  {selected.base_url && (
                    <p className="text-[11px] text-gray-400 dark:text-gray-500 mt-1">
                      默认: <span className="font-mono">{selected.base_url}</span>
                    </p>
                  )}
                </div>
                <div>
                  <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block">选择模型</label>
                  <div className="space-y-1 max-h-48 overflow-y-auto">
                    {selected.available_models.map((model) => (
                      <label key={model.id} className="flex items-center gap-2 px-3 py-1.5 rounded hover:bg-gray-50 dark:hover:bg-gray-700/50 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={newModels.includes(model.id)}
                          onChange={(e) => toggleModel(model.id, e.target.checked)}
                          className="rounded border-gray-300 dark:border-gray-600 text-purple-600 focus:ring-purple-500"
                        />
                        <span className="text-sm text-gray-700 dark:text-gray-300">{model.name}</span>
                        <span className="text-xs text-gray-400 dark:text-gray-500 ml-auto">{model.id}</span>
                      </label>
                    ))}
                  </div>
                </div>
                <div className="flex gap-2 pt-2">
                  <button
                    onClick={handleSaveEdit}
                    disabled={newModels.length === 0}
                    className="flex-1 bg-purple-600 hover:bg-purple-700 disabled:opacity-50 text-white px-4 py-2 rounded-lg text-sm transition-colors"
                  >
                    保存
                  </button>
                  <button
                    onClick={() => setIsEditing(false)}
                    className="px-4 py-2 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors"
                  >
                    取消
                  </button>
                </div>
              </div>
            )}

            {!selected && (
              <div className="flex items-center justify-center h-48 text-gray-400 dark:text-gray-500 text-sm">
                选择左侧的模型提供商进行配置
              </div>
            )}
          </Row.Item>
        </Row>
      </>
    );
  }

  // ── Skills Tab ──

  function renderSkillsTab() {
    return (
      <div className="space-y-3">
        {skills.map((skill) => (
          <div key={skill.id} className="flex items-center gap-4 p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
            <div className="w-9 h-9 rounded-lg bg-purple-100 dark:bg-purple-900/40 flex items-center justify-center flex-shrink-0">
              <BrainCircuit size={18} className="text-purple-600 dark:text-purple-400" />
            </div>
              <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 min-w-0">
                    <h4 className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">{skill.name}</h4>
                    <span className="text-xs text-gray-400 flex-shrink-0 whitespace-nowrap">v{skill.version}</span>
                    {skill.author && (
                      <span className="text-xs bg-gray-200 text-gray-500 px-1.5 py-0.5 rounded flex-shrink-0 whitespace-nowrap">{skill.author}</span>
                    )}
                  </div>
                  <p className="text-xs text-gray-500 mt-0.5 truncate">{skill.description}</p>
                </div>
            <div className="flex items-center gap-2 flex-shrink-0">
              <button
                onClick={() => { setSelectedSkillId(skill.id); setShowSkillDetail(true); }}
                className="p-1.5 rounded-lg hover:bg-gray-200 text-gray-400 hover:text-purple-600 transition-colors"
                title="配置"
              >
                <SettingsIcon size={14} />
              </button>
              <label className="relative inline-flex items-center cursor-pointer">
                <input
                  type="checkbox"
                  checked={skill.enabled}
                  onChange={(e) => toggleSkill(skill.id, e.target.checked)}
                  className="sr-only peer"
                />
                <div className="w-11 h-6 bg-gray-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-purple-600 shadow-sm"></div>
              </label>
            </div>
          </div>
        ))}
        <button
          onClick={() => setShowSkillInstall(true)}
          className="w-full flex items-center justify-center gap-2 py-3 border-2 border-dashed border-gray-200 rounded-xl text-sm text-gray-400 hover:text-purple-600 hover:border-purple-300 transition-colors"
        >
          <Plus size={16} />
          安装 Skill
        </button>
      </div>
    );
  }

  // ── Prompts Tab ──

  function renderPromptsTab() {
    return (
      <div className="space-y-4">
        <PromptForm />
        <div className="space-y-2">
          {systemPrompts.map((prompt) => (
            <div key={prompt.id} className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700 flex items-start gap-3">
              <div className="flex-1">
                <div className="flex items-center gap-2">
                  <h4 className="text-sm font-medium text-gray-900 dark:text-gray-100">{prompt.name}</h4>
                  {prompt.is_default && (
                    <span className="text-xs bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-400 px-2 py-0.5 rounded-full">默认</span>
                  )}
                </div>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 line-clamp-2">{prompt.content}</p>
              </div>
              <div className="flex items-center gap-1">
                {!prompt.is_default && (
                  <button
                    onClick={() => setDefaultSystemPrompt(prompt.id)}
                    className="p-1.5 rounded-lg hover:bg-gray-200 text-gray-400 hover:text-yellow-500 transition-colors"
                    title="设为默认"
                  >
                    <Star size={14} />
                  </button>
                )}
                <button
                  onClick={() => deleteSystemPrompt(prompt.id)}
                  className="p-1.5 rounded-lg hover:bg-gray-200 text-gray-400 hover:text-red-500 transition-colors"
                  title="删除"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // ── Runtime Tab ──

  function renderRuntimeTab() {
    return (
      <div className="space-y-4">
        <div className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2 mb-3">
            <FolderOpen size={16} className="text-gray-400" />
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">运行时安装目录</h3>
          </div>
          {editingDir ? (
            <div className="space-y-2">
              <DirectoryPicker value={dirValue} onChange={setDirValue} placeholder="选择运行时安装目录..." />
              <div className="flex gap-2">
                <button
                  onClick={async () => { await setInstallDir(dirValue); setEditingDir(false); }}
                  className="px-3 py-1.5 bg-purple-600 hover:bg-purple-700 text-white rounded-lg text-xs transition-colors"
                >
                  保存
                </button>
                <button
                  onClick={() => setEditingDir(false)}
                  className="px-3 py-1.5 bg-gray-200 dark:bg-gray-600 hover:bg-gray-300 dark:hover:bg-gray-500 text-gray-700 dark:text-gray-300 rounded-lg text-xs transition-colors"
                >
                  取消
                </button>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-2">
              <span className="flex-1 text-xs font-mono text-gray-500 dark:text-gray-400 truncate">
                {installDir || '加载中...'}
              </span>
              <button
                onClick={() => { setDirValue(installDir); setEditingDir(true); }}
                className="text-xs text-purple-500 hover:text-purple-600 hover:underline"
              >
                修改
              </button>
            </div>
          )}
          <p className="text-[10px] text-gray-400 mt-2">
            所有内置安装的运行时将存储在此目录下，修改后需重新安装已有版本
          </p>
        </div>

        {/* Proxy configuration */}
        <div className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
          <div className="flex items-center gap-2 mb-3">
            <Globe size={16} className="text-gray-400" />
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">网络代理</h3>
          </div>
          <div className="flex items-center gap-3">
            <input
              type="text"
              value={proxyUrl}
              onChange={(e) => setProxyUrl(e.target.value)}
              placeholder="HTTP 代理地址 (可选)"
              className="flex-1 px-3 py-2 bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg text-sm text-gray-900 dark:text-gray-100 placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-purple-500"
            />
            <button
              onClick={saveProxyUrl}
              disabled={proxyUrl === savedProxyUrl}
              className="px-4 py-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white rounded-lg text-sm transition-all"
            >
              保存
            </button>
          </div>
          <p className="text-[11px] text-gray-400 mt-1.5">
            设置后，运行时下载将通过该代理进行。支持 http:// 和 https:// 协议。
          </p>
        </div>
      </div>
    );
  }

  // ── Conversation Tab ──

  function renderConversationTab() {
    return (
      <div className="space-y-4">
        {/* Auto-title */}
        <div className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
          <div className="flex items-center justify-between mb-1">
            <div>
              <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">自动标题生成</h3>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">AI 自动为对话生成标题</p>
            </div>
            <Switch
              checked={lifecycleConfig.auto_title_enabled}
              onChange={(checked) => updateLifecycleConfig({ ...lifecycleConfig, auto_title_enabled: checked })}
            />
          </div>
        </div>

        {/* Auto-summarization */}
        <div className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
          <div className="flex items-center justify-between mb-3">
            <div>
              <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">自动摘要</h3>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">将早期消息压缩为摘要以节省上下文</p>
            </div>
            <Switch
              checked={lifecycleConfig.auto_summarize_enabled}
              onChange={(checked) => updateLifecycleConfig({ ...lifecycleConfig, auto_summarize_enabled: checked })}
            />
          </div>
          {lifecycleConfig.auto_summarize_enabled && (
            <div className="flex items-center justify-between pt-3 border-t border-gray-200 dark:border-gray-600">
              <span className="text-sm text-gray-600 dark:text-gray-400">摘要触发间隔</span>
              <div className="flex items-center gap-2">
                <InputNumber
                  size="small"
                  min={5}
                  max={100}
                  value={lifecycleConfig.summarize_chunk_size}
                  onChange={(val) => val && updateLifecycleConfig({ ...lifecycleConfig, summarize_chunk_size: val })}
                  className="w-20"
                />
                <span className="text-xs text-gray-400">条消息</span>
              </div>
            </div>
          )}
        </div>

        {/* Auto-archive */}
        <div className="p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
          <div className="flex items-center justify-between mb-3">
            <div>
              <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">自动归档</h3>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">将长期未更新的会话自动归档</p>
            </div>
            <Switch
              checked={lifecycleConfig.auto_archive_enabled}
              onChange={(checked) => updateLifecycleConfig({ ...lifecycleConfig, auto_archive_enabled: checked })}
            />
          </div>
          {lifecycleConfig.auto_archive_enabled && (
            <div className="flex items-center justify-between pt-3 border-t border-gray-200 dark:border-gray-600">
              <span className="text-sm text-gray-600 dark:text-gray-400">归档阈值</span>
              <div className="flex items-center gap-2">
                <InputNumber
                  size="small"
                  min={1}
                  max={365}
                  value={lifecycleConfig.archive_after_days}
                  onChange={(val) => val && updateLifecycleConfig({ ...lifecycleConfig, archive_after_days: val })}
                  className="w-20"
                />
                <span className="text-xs text-gray-400">天未活跃</span>
              </div>
            </div>
          )}
        </div>

        <p className="text-[11px] text-gray-400 px-1">
          所有配置更改会自动保存
        </p>
      </div>
    );
  }

  // ── Render ──

  return (
    <ManagerPageLayout
      icon={<SettingsIcon size={20} className="text-white" />}
      title="系统设置"
      subtitle="模型提供商、提示词、技能和运行环境配置"
    >
      <Row style={{ height: '100%' }}>
        {/* Sidebar tabs — vertical */}
        <Row.Item $width={176} $fixed>
          <div style={{ borderRight: '1px solid #e5e7eb', paddingRight: 24 }} className="dark:border-gray-700 h-full">
            <div className="space-y-1">
              {tabs.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`w-full flex items-center gap-2.5 px-3 py-2.5 rounded-lg text-sm font-medium transition-all text-left ${
                    activeTab === tab.id
                      ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 shadow-sm'
                      : 'text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700/50 hover:text-gray-700 dark:hover:text-gray-300'
                  }`}
                >
                  {tab.icon}
                  {tab.label}
                </button>
              ))}
            </div>
          </div>
        </Row.Item>

        {/* Tab content */}
        <Row.Item $scale={1} style={{ paddingLeft: 24 }}>
          {renderTabContent()}
        </Row.Item>
      </Row>

      {/* Overlays */}
      {showSkillDetail && selectedSkillId && (
        <SkillDetailPanel
          skillId={selectedSkillId}
          onClose={() => { setShowSkillDetail(false); setSelectedSkillId(null); fetchSkills(); }}
        />
      )}
      {showSkillInstall && (
        <SkillInstallDialog
          onClose={() => { setShowSkillInstall(false); fetchSkills(); }}
        />
      )}
    </ManagerPageLayout>
  );
}

// ── Prompt Form ──

function PromptForm() {
  const { createSystemPrompt } = useStore();
  const [name, setName] = useState('');
  const [content, setContent] = useState('');

  const handleSubmit = async () => {
    if (!name.trim() || !content.trim()) return;
    await createSystemPrompt(name, content, false);
    setName('');
    setContent('');
  };

  return (
    <div className="space-y-3 p-4 bg-gray-50 dark:bg-gray-700/50 rounded-xl border border-gray-100 dark:border-gray-700">
      <input
        type="text"
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="提示词名称"
        className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400"
      />
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        placeholder="系统提示词内容..."
        rows={3}
        className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400 resize-none"
      />
      <button
        onClick={handleSubmit}
        disabled={!name.trim() || !content.trim()}
        className="w-full flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2.5 rounded-lg transition-all text-sm"
      >
        <Plus size={16} />
        添加提示词
      </button>
    </div>
  );
}
