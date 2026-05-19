import { useState, useEffect } from 'react';
import { X, Eye, EyeOff, Check, Wrench, FileText, Cpu, ChevronRight, Star, Trash2, Plus, Sparkles } from 'lucide-react';
import { useStore } from '../store';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export function SettingsModal({ isOpen, onClose }: SettingsModalProps) {
  const [activeTab, setActiveTab] = useState<'providers' | 'tools' | 'prompts'>('providers');
  const {
    providers, fetchProviders, setupProvider, updateProviderConfig, removeProvider,
    tools, fetchTools, toggleTool,
    systemPrompts, fetchSystemPrompts, deleteSystemPrompt, setDefaultSystemPrompt,
    models, fetchModels, defaultModel, setDefaultModel,
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

  useEffect(() => {
    if (isOpen) {
      fetchProviders();
      fetchTools();
      fetchSystemPrompts();
      fetchModels();
    }
  }, [isOpen]);

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
    // 显式刷新以确保 UI 立即反映最新状态
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
      // 显式刷新以确保 UI 立即反映最新状态
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
    if (isEditing) {
      setNewModels(prev => checked ? [...prev, modelId] : prev.filter(m => m !== modelId));
    } else if (showAddForm) {
      setNewModels(prev => checked ? [...prev, modelId] : prev.filter(m => m !== modelId));
    }
  };

  const getProviderColor = (providerId: string) => {
    const colors: Record<string, string> = {
      openai: 'bg-green-100 text-green-700',
      anthropic: 'bg-orange-100 text-orange-700',
      google: 'bg-blue-100 text-blue-700',
      groq: 'bg-yellow-100 text-yellow-700',
      deepseek: 'bg-purple-100 text-purple-700',
      zhipu: 'bg-cyan-100 text-cyan-700',
      moonshot: 'bg-rose-100 text-rose-700',
      siliconflow: 'bg-indigo-100 text-indigo-700',
      ollama: 'bg-gray-100 text-gray-700',
      lmstudio: 'bg-pink-100 text-pink-700',
    };
    return colors[providerId] || 'bg-gray-100 text-gray-700';
  };

  if (!isOpen) return null;

  const tabs = [
    { id: 'providers' as const, icon: <Cpu size={16} />, label: '模型提供商' },
    { id: 'tools' as const, icon: <Wrench size={16} />, label: '工具' },
    { id: 'prompts' as const, icon: <FileText size={16} />, label: '提示词' },
  ];

  return (
    <div className="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-800 rounded-2xl w-[800px] max-h-[85vh] flex flex-col shadow-2xl">
        <div className="flex items-center justify-between p-5 border-b border-gray-100 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">设置</h2>
          <button onClick={onClose} className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors text-gray-400 hover:text-gray-600 dark:hover:text-gray-300">
            <X size={20} />
          </button>
        </div>

        <div className="flex border-b border-gray-100 dark:border-gray-700 px-5">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 px-4 py-3 text-sm font-medium transition-all border-b-2 ${
                activeTab === tab.id
                  ? 'text-purple-600 dark:text-purple-400 border-purple-600 dark:border-purple-400'
                  : 'text-gray-500 dark:text-gray-400 border-transparent hover:text-gray-700 dark:hover:text-gray-300'
              }`}
            >
              {tab.icon}
              {tab.label}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto p-5 dark:text-gray-300">
          {activeTab === 'providers' && (
            <>
              <div className="mb-4 p-4 bg-purple-50 border border-purple-100 rounded-xl">
                <div className="flex items-center gap-2 mb-3">
                  <Sparkles size={16} className="text-purple-600" />
                  <h3 className="text-sm font-medium text-purple-900">默认模型</h3>
                </div>
                <div className="flex items-center gap-3">
                  <select
                    value={defaultModel || ''}
                    onChange={(e) => { if (e.target.value) setDefaultModel(e.target.value); }}
                    className="flex-1 bg-white border border-purple-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
                  >
                    <option value="">选择默认模型...</option>
                    {models.filter(m => m.enabled).map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.display_name} ({model.provider})
                      </option>
                    ))}
                  </select>
                  {defaultModel && (
                    <span className="text-xs bg-purple-100 text-purple-600 px-2 py-1 rounded-full whitespace-nowrap">
                      当前: {models.find(m => m.id === defaultModel)?.display_name || defaultModel}
                    </span>
                  )}
                </div>
                {models.filter(m => m.enabled).length === 0 && (
                  <p className="text-xs text-red-500 mt-2">请先配置模型提供商并启用至少一个模型</p>
                )}
              </div>
              <div className="flex gap-4">
              {/* Provider list */}
              <div className="w-64 space-y-1">
                {providers.map((provider) => (
                  <button
                    key={provider.id}
                    onClick={() => handleSelectProvider(provider)}
                    className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-left transition-all ${
                      selectedProvider === provider.id
                        ? 'bg-purple-50 text-purple-700'
                        : 'hover:bg-gray-50 text-gray-700'
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

              {/* Provider detail */}
              <div className="flex-1">
                {selected && !showAddForm && !isEditing && (
                  <div className="space-y-4">
                    <div className="flex items-center gap-3">
                      <span className={`text-sm px-3 py-1 rounded-full ${getProviderColor(selected.id)}`}>
                        {selected.name}
                      </span>
                      {selected.configured ? (
                        <span className="text-xs bg-green-100 text-green-600 px-2 py-0.5 rounded-full">已配置</span>
                      ) : (
                        <span className="text-xs bg-gray-100 text-gray-500 px-2 py-0.5 rounded-full">未配置</span>
                      )}
                    </div>

                    {selected.configured && (
                      <>
                        <div>
                          <label className="text-xs text-gray-500 mb-1 block">已启用模型</label>
                          <div className="space-y-1">
                            {selected.available_models.map((model) => (
                              <div key={model.id} className="flex items-center gap-2 px-3 py-1.5 rounded bg-gray-50">
                                <Check size={14} className={`flex-shrink-0 ${selected.enabled_models.includes(model.id) ? 'text-green-500' : 'text-gray-300'}`} />
                                <span className="text-sm text-gray-700">{model.name}</span>
                                <span className="text-xs text-gray-400 ml-auto">{model.id}</span>
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
                    <h3 className="text-sm font-medium text-gray-900">配置 {selected.name}</h3>

                    {selected.requires_api_key && (
                      <div>
                        <label className="text-xs text-gray-500 mb-1 block">API Key</label>
                        <div className="relative">
                          <input
                            type={showApiKey ? 'text' : 'password'}
                            value={newApiKey}
                            onChange={(e) => setNewApiKey(e.target.value)}
                            placeholder="sk-..."
                            className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 pr-10 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
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
                      <label className="text-xs text-gray-500 mb-1 block">Base URL (可选)</label>
                      <input
                        type="text"
                        value={newBaseUrl}
                        onChange={(e) => setNewBaseUrl(e.target.value)}
                        placeholder={selected.base_url || '使用默认地址'}
                        className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
                      />
                    </div>

                    <div>
                      <label className="text-xs text-gray-500 mb-1 block">选择模型</label>
                      <div className="space-y-1 max-h-48 overflow-y-auto">
                        {selected.available_models.map((model) => (
                          <label key={model.id} className="flex items-center gap-2 px-3 py-1.5 rounded hover:bg-gray-50 cursor-pointer">
                            <input
                              type="checkbox"
                              checked={newModels.includes(model.id)}
                              onChange={(e) => toggleModel(model.id, e.target.checked)}
                              className="rounded border-gray-300 text-purple-600 focus:ring-purple-500"
                            />
                            <span className="text-sm text-gray-700">{model.name}</span>
                            <span className="text-xs text-gray-400 ml-auto">{model.id}</span>
                          </label>
                        ))}
                      </div>
                    </div>

                    {saveError && (
                      <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700">
                        {saveError}
                      </div>
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
                        onClick={() => {
                          setShowAddForm(false);
                          setSelectedProvider(null);
                          setSaveError(null);
                        }}
                        className="px-4 py-2 bg-gray-100 hover:bg-gray-200 text-gray-700 rounded-lg text-sm transition-colors"
                      >
                        取消
                      </button>
                    </div>
                  </div>
                )}

                {selected && isEditing && (
                  <div className="space-y-3">
                    <h3 className="text-sm font-medium text-gray-900">编辑 {selected.name}</h3>

                    {selected.requires_api_key && (
                      <div>
                        <label className="text-xs text-gray-500 mb-1 block">API Key (留空保持不变)</label>
                        <div className="relative">
                          <input
                            type={showApiKey ? 'text' : 'password'}
                            value={newApiKey}
                            onChange={(e) => setNewApiKey(e.target.value)}
                            placeholder="留空保持不变"
                            className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 pr-10 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
                          />
                          {newApiKey && (
                            <button
                              onClick={() => setShowApiKey(!showApiKey)}
                              className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
                            >
                              {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
                            </button>
                          )}
                        </div>
                      </div>
                    )}

                    <div>
                      <label className="text-xs text-gray-500 mb-1 block">Base URL (可选)</label>
                      <input
                        type="text"
                        value={newBaseUrl}
                        onChange={(e) => setNewBaseUrl(e.target.value)}
                        placeholder="使用默认地址"
                        className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
                      />
                    </div>

                    <div>
                      <label className="text-xs text-gray-500 mb-1 block">选择模型</label>
                      <div className="space-y-1 max-h-48 overflow-y-auto">
                        {selected.available_models.map((model) => (
                          <label key={model.id} className="flex items-center gap-2 px-3 py-1.5 rounded hover:bg-gray-50 cursor-pointer">
                            <input
                              type="checkbox"
                              checked={newModels.includes(model.id)}
                              onChange={(e) => toggleModel(model.id, e.target.checked)}
                              className="rounded border-gray-300 text-purple-600 focus:ring-purple-500"
                            />
                            <span className="text-sm text-gray-700">{model.name}</span>
                            <span className="text-xs text-gray-400 ml-auto">{model.id}</span>
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
                  <div className="flex items-center justify-center h-48 text-gray-400 text-sm">
                    选择左侧的模型提供商进行配置
                  </div>
                )}
              </div>
            </div>
            </>
          )}

          {activeTab === 'tools' && (
            <div className="space-y-3">
              {tools.map((tool) => (
                <div key={tool.name} className="flex items-center gap-4 p-4 bg-gray-50 rounded-xl border border-gray-100">
                  <div className="flex-1">
                    <h4 className="text-sm font-medium text-gray-900 capitalize">{tool.name.replace('_', ' ')}</h4>
                    <p className="text-xs text-gray-500 mt-1">{tool.description}</p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={tool.enabled}
                      onChange={(e) => toggleTool(tool.name, e.target.checked)}
                      className="sr-only peer"
                    />
                    <div className="w-11 h-6 bg-gray-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-purple-600 shadow-sm"></div>
                  </label>
                </div>
              ))}
            </div>
          )}

          {activeTab === 'prompts' && (
            <div className="space-y-4">
              <PromptForm />
              <div className="space-y-2">
                {systemPrompts.map((prompt) => (
                  <div key={prompt.id} className="p-4 bg-gray-50 rounded-xl border border-gray-100 flex items-start gap-3">
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <h4 className="text-sm font-medium text-gray-900">{prompt.name}</h4>
                        {prompt.is_default && (
                          <span className="text-xs bg-purple-100 text-purple-600 px-2 py-0.5 rounded-full">默认</span>
                        )}
                      </div>
                      <p className="text-xs text-gray-500 mt-1 line-clamp-2">{prompt.content}</p>
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
          )}
        </div>
      </div>
    </div>
  );
}

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
    <div className="space-y-3 p-4 bg-gray-50 rounded-xl border border-gray-100">
      <input
        type="text"
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="提示词名称"
        className="w-full bg-white border border-gray-200 rounded-lg px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
      />
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        placeholder="系统提示词内容..."
        rows={3}
        className="w-full bg-white border border-gray-200 rounded-lg px-4 py-2.5 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 resize-none"
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
