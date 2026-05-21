import { useState, useEffect } from 'react';
import { Plus, Trash2, MessageSquare, Eraser } from 'lucide-react';
import { Col, Row } from '@jelper/component';
import { Select } from 'antd';
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
    <Col className="h-full bg-white dark:bg-gray-800/70 border-r border-purple-100/50 dark:border-purple-900/30 transition-colors backdrop-blur-sm">
      <Col.Item $fixed>
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
              className="w-full bg-gray-50 dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 placeholder-gray-400"
            />
            {getAvailableModelCount() > 0 && (
              <div>
                <label className="text-xs text-gray-500 dark:text-gray-400 mb-1 block px-1">选择模型</label>
                <Select
                  value={selectedModel || undefined}
                  onChange={(v) => v && setSelectedModel(v)}
                  className="w-full"
                  size="small"
                  options={models.filter(m => m.enabled).map(m => ({
                    value: m.id,
                    label: `${m.display_name} (${m.provider})${m.is_default ? ' (默认)' : ''}`
                  }))}
                />
              </div>
            )}
            {getAvailableModelCount() === 0 && (
              <div className="text-xs text-red-500 dark:text-red-400 px-1">
                请先在设置中配置模型提供商
              </div>
            )}
            {systemPrompts.length > 0 && (
              <Select
                value={selectedPrompt || undefined}
                onChange={setSelectedPrompt}
                className="w-full"
                size="small"
                placeholder="无系统提示词"
                allowClear
                options={[
                  { value: '', label: '无系统提示词' },
                  ...systemPrompts.map(p => ({
                    value: p.id,
                    label: `${p.name}${p.is_default ? ' (默认)' : ''}`
                  }))
                ]}
              />
            )}
            <Row $gap={8}>
              <Row.Item $scale={1}>
                <button
                  onClick={handleCreate}
                  disabled={!newTitle.trim() || getAvailableModelCount() === 0 || loading}
                  className="w-full bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 text-white px-4 py-2 rounded-lg transition-all text-sm"
                >
                  创建
                </button>
              </Row.Item>
              <Row.Item $fixed>
                <button
                  onClick={() => setShowNewChat(false)}
                  className="px-3 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-600 dark:text-gray-300 rounded-lg transition-all text-sm"
                >
                  取消
                </button>
              </Row.Item>
            </Row>
          </div>
        )}
      </Col.Item>

      <Col.Item $scale={1}>
        <div className="overflow-y-auto h-full px-2">
          <div className="text-xs font-medium text-gray-400 dark:text-gray-500 px-3 py-2">最近对话</div>
          {conversations.map((conv) => (
            <div
              key={conv.id}
              className={`flex items-center gap-2 px-3 py-2.5 rounded-lg mb-1 cursor-pointer group transition-all ${
                currentConversation?.id === conv.id
                  ? 'bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300'
                  : 'hover:bg-purple-50/50 dark:hover:bg-gray-700/50 text-gray-700 dark:text-gray-300'
              }`}
            >
              <MessageSquare size={16} className="flex-shrink-0 text-gray-400 dark:text-gray-500" />
              {editingId === conv.id ? (
                  <input
                    type="text"
                    value={editingTitle}
                    onChange={(e) => setEditingTitle(e.target.value)}
                    onBlur={handleSaveEdit}
                    onKeyDown={(e) => e.key === 'Enter' && handleSaveEdit()}
                    className="flex-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded px-2 py-1 text-sm focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100"
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
                    className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 dark:text-gray-500 hover:text-yellow-500 dark:hover:text-yellow-400 transition-colors"
                    title="清空消息"
                  >
                    <Eraser size={14} />
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      deleteConversation(conv.id);
                    }}
                    className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 dark:text-gray-500 hover:text-red-500 dark:hover:text-red-400 transition-colors"
                    title="删除"
                  >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      </Col.Item>
    </Col>
  );
}
