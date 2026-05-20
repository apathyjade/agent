import { useState, useEffect } from 'react';
import { Plus, Trash2, MessageSquare, Eraser, X, Sparkles } from 'lucide-react';
import { useStore } from '../store';

export function Sidebar() {
  const {
    conversations,
    currentConversation,
    loading,
    models,
    providers,
    defaultModel,
    fetchConversations,
    fetchProviders,
    createConversation,
    selectConversation,
    deleteConversation,
    updateConversationTitle,
    clearConversation,
    systemPrompts,
    fetchSystemPrompts,
    setSidebarOpen,
  } = useStore();

  const [showNewChat, setShowNewChat] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [selectedModel, setSelectedModel] = useState<string>('');
  const [selectedPrompt, setSelectedPrompt] = useState<string>('');
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');

  useEffect(() => {
    if (showNewChat) {
      fetchProviders();
      fetchSystemPrompts();
      // Auto-select default model when opening new chat dialog
      setSelectedModel(defaultModel || getDefaultModelId() || '');
    }
  }, [showNewChat]);

  const handleCreate = async () => {
    if (!newTitle.trim()) return;
    const modelId = selectedModel || defaultModel || getDefaultModelId();
    if (!modelId) return;
    const prompt = systemPrompts.find(p => p.id === selectedPrompt);
    await createConversation(newTitle, modelId, prompt?.content);
    setNewTitle('');
    setSelectedPrompt('');
    setShowNewChat(false);
    fetchConversations();
  };

  const getDefaultModelId = (): string | null => {
    const enabled = models.filter(m => m.enabled);
    if (enabled.length > 0) return enabled[0].id;
    for (const provider of providers) {
      if (provider.configured && provider.enabled_models.length > 0) {
        return provider.enabled_models[0];
      }
    }
    return null;
  };

  const getAvailableModelCount = (): number => {
    const modelCount = models.filter(m => m.enabled).length;
    if (modelCount > 0) return modelCount;
    return providers.reduce((count, p) => count + (p.configured ? p.enabled_models.length : 0), 0);
  };

  const handleStartEdit = (conv: { id: string; title: string }) => {
    setEditingId(conv.id);
    setEditingTitle(conv.title);
  };

  const handleSaveEdit = async () => {
    if (editingId && editingTitle.trim()) {
      await updateConversationTitle(editingId, editingTitle);
    }
    setEditingId(null);
    setEditingTitle('');
  };

  return (
    <>
      <div className="h-full bg-gray-50 dark:bg-gray-800 border-r border-gray-100 dark:border-gray-700 flex flex-col transition-colors">
        <div className="p-4 border-b border-gray-100 dark:border-gray-700 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center">
              <Sparkles size={16} className="text-white" />
            </div>
            <h1 className="text-lg font-semibold text-gray-900 dark:text-gray-100">Agent</h1>
          </div>
          <button onClick={() => setSidebarOpen(false)} className="p-1 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors">
            <X size={18} className="text-gray-500 dark:text-gray-400" />
          </button>
        </div>

        <div className="p-3">
          <button
            onClick={() => setShowNewChat(true)}
            className="w-full flex items-center justify-center gap-2 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white px-4 py-2.5 rounded-xl transition-all shadow-sm hover:shadow-md"
          >
            <Plus size={16} />
            新对话
          </button>
        </div>

        {showNewChat && (
          <div className="px-3 pb-3 space-y-2">
            <input
              type="text"
              value={newTitle}
              onChange={(e) => setNewTitle(e.target.value)}
              placeholder="对话名称"
              className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 dark:placeholder-gray-400"
            />
            {getAvailableModelCount() > 0 && (
              <div>
                <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block px-1">选择模型</label>
                <select
                  value={selectedModel}
                  onChange={(e) => setSelectedModel(e.target.value)}
                  className="w-full bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100"
                >
                  {models.filter(m => m.enabled).map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.display_name} ({model.provider}){model.is_default ? ' (默认)' : ''}
                    </option>
                  ))}
                </select>
              </div>
            )}
            {getAvailableModelCount() === 0 && (
              <div className="text-xs text-red-500 px-1">
                请先在设置中配置模型提供商
              </div>
            )}
            {systemPrompts.length > 0 && (
              <select
                value={selectedPrompt}
                onChange={(e) => setSelectedPrompt(e.target.value)}
                className="w-full bg-white border border-gray-200 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
              >
                <option value="">无系统提示词</option>
                {systemPrompts.map(p => (
                  <option key={p.id} value={p.id}>{p.name}{p.is_default ? ' (默认)' : ''}</option>
                ))}
              </select>
            )}
            <div className="flex gap-2">
              <button
                onClick={handleCreate}
                disabled={!newTitle.trim() || getAvailableModelCount() === 0 || loading}
                className="flex-1 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2 rounded-lg transition-all text-sm"
              >
                创建
              </button>
              <button
                onClick={() => setShowNewChat(false)}
                className="px-3 py-2 bg-gray-100 hover:bg-gray-200 text-gray-600 rounded-lg transition-all text-sm"
              >
                取消
              </button>
            </div>
          </div>
        )}

        <div className="flex-1 overflow-y-auto px-2">
          <div className="text-xs font-medium text-gray-400 dark:text-gray-500 px-3 py-2">最近对话</div>
          {conversations.map((conv) => (
            <div
              key={conv.id}
              className={`flex items-center gap-2 px-3 py-2.5 rounded-lg mb-1 cursor-pointer group transition-all ${
                currentConversation?.id === conv.id
                  ? 'bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300'
                  : 'hover:bg-gray-100 dark:hover:bg-gray-700/50 text-gray-700 dark:text-gray-300'
              }`}
            >
              <MessageSquare size={16} className="flex-shrink-0 opacity-60" />
              {editingId === conv.id ? (
                <input
                  type="text"
                  value={editingTitle}
                  onChange={(e) => setEditingTitle(e.target.value)}
                  onBlur={handleSaveEdit}
                  onKeyDown={(e) => e.key === 'Enter' && handleSaveEdit()}
                  className="flex-1 bg-white border border-gray-200 rounded px-2 py-1 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500"
                  autoFocus
                />
              ) : (
                <span
                  className="flex-1 truncate text-sm"
                  onClick={() => selectConversation(conv.id)}
                  onDoubleClick={() => handleStartEdit(conv)}
                >
                  {conv.title}
                </span>
              )}
              <div className="hidden group-hover:flex items-center gap-1 flex-shrink-0">
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    selectConversation(conv.id);
                    clearConversation();
                  }}
                  className="p-1 rounded hover:bg-gray-200 text-gray-400 hover:text-yellow-500 transition-colors"
                  title="清空消息"
                >
                  <Eraser size={14} />
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    deleteConversation(conv.id);
                  }}
                  className="p-1 rounded hover:bg-gray-200 text-gray-400 hover:text-red-500 transition-colors"
                  title="删除"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>

      </div>
    </>
  );
}
