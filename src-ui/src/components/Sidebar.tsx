import { useState } from 'react';
import { Plus, Trash2, MessageSquare, Archive, RotateCcw } from 'lucide-react';
import { Popconfirm } from 'antd';
import { Col } from '@jelper/component';
import { useStore } from '../store';

export function Sidebar() {
  const {
    sessions,
    currentSession,
    selectSession,
    deleteSession,
    updateSessionTitle,
    newChat,
    archiveSession,
    unarchiveSession,
  } = useStore();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');
  const [activePopconfirmId, setActivePopconfirmId] = useState<string | null>(null);
  const [showArchived, setShowArchived] = useState(false);

  const archivedSessions = sessions.filter(s => s.archived);
  const activeSessions = sessions.filter(s => !s.archived);
  const displaySessions = showArchived ? archivedSessions : activeSessions;

  const handleStartEdit = (sess: { id: string; title: string }) => {
    setEditingId(sess.id);
    setEditingTitle(sess.title);
  };

  const handleSaveEdit = async () => {
    if (editingId && editingTitle.trim()) {
      await updateSessionTitle(editingId, editingTitle);
    }
    setEditingId(null);
    setEditingTitle('');
  };

  return (
    <Col className="h-full bg-white dark:bg-gray-800/70 border-r border-purple-100/50 dark:border-purple-900/30 transition-colors backdrop-blur-sm">
      <Col.Item $scale={1}>
        <div className="overflow-y-auto h-full px-2">
          <div className="flex items-center gap-1 px-3 py-2">
            <button
              onClick={() => setShowArchived(false)}
              className={`text-xs px-2.5 py-1 rounded-lg font-medium transition-all ${
                !showArchived
                  ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300'
                  : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-400'
              }`}
            >
              对话
            </button>
            <button
              onClick={() => setShowArchived(true)}
              className={`text-xs px-2.5 py-1 rounded-lg font-medium transition-all ${
                showArchived
                  ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300'
                  : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-400'
              }`}
            >
              已归档 {archivedSessions.length > 0 && `(${archivedSessions.length})`}
            </button>
          </div>
          {displaySessions.length === 0 && (
            <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
              <MessageSquare size={32} className="text-gray-300 dark:text-gray-600 mb-3" />
              <p className="text-sm text-gray-400 dark:text-gray-500 mb-1">
                {showArchived ? '没有已归档的对话' : '暂无对话'}
              </p>
              <p className="text-xs text-gray-300 dark:text-gray-600">
                {showArchived ? '对话会在30天不活跃后自动归档' : '在右侧输入，开启新对话'}
              </p>
            </div>
          )}
          {displaySessions.map((sess) => (
            <div
              key={sess.id}
              className={`flex items-center gap-2 px-3 py-2.5 rounded-lg mb-1 cursor-pointer group transition-all ${
                currentSession?.id === sess.id
                  ? 'bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300'
                  : 'hover:bg-purple-50/50 dark:hover:bg-gray-700/50 text-gray-700 dark:text-gray-300'
              }`}
            >
              <MessageSquare size={16} className="flex-shrink-0 text-gray-400 dark:text-gray-500" />
              {editingId === sess.id ? (
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
                  onClick={() => selectSession(sess.id)}
                  onDoubleClick={() => handleStartEdit(sess)}
                >
                  {sess.title}
                </span>
              )}
              <div
                className={`flex items-center gap-1 flex-shrink-0 transition-opacity ${
                  activePopconfirmId === sess.id || currentSession?.id === sess.id
                    ? 'opacity-100'
                    : 'opacity-0 group-hover:opacity-100'
                }`}
                onClick={(e) => e.stopPropagation()}
              >
                <Popconfirm
                  title="确认删除此对话？"
                  onConfirm={() => deleteSession(sess.id)}
                  onOpenChange={(visible) => setActivePopconfirmId(visible ? sess.id : null)}
                  okText="确认"
                  cancelText="取消"
                  placement="right"
                  okButtonProps={{ danger: true, size: 'small' }}
                  cancelButtonProps={{ size: 'small' }}
                >
                  <button
                    className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 dark:text-gray-500 hover:text-red-500 dark:hover:text-red-400 transition-colors"
                    title="删除"
                  >
                    <Trash2 size={14} />
                  </button>
                </Popconfirm>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    if (showArchived) {
                      unarchiveSession(sess.id);
                    } else {
                      archiveSession(sess.id);
                    }
                  }}
                  className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 hover:text-amber-500 dark:hover:text-amber-400 transition-colors"
                  title={showArchived ? '恢复' : '归档'}
                >
                  {showArchived ? <RotateCcw size={14} /> : <Archive size={14} />}
                </button>
              </div>
            </div>
          ))}
        </div>
      </Col.Item>

      <Col.Item $fixed>
        <div className="p-3 border-t border-purple-100/50 dark:border-purple-900/30">
          <button
            onClick={newChat}
            disabled={!currentSession}
            className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl transition-all shadow-sm disabled:cursor-not-allowed disabled:opacity-40 bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white disabled:from-gray-300 disabled:to-gray-300 dark:disabled:from-gray-600 dark:disabled:to-gray-600 disabled:text-gray-500 dark:disabled:text-gray-400"
          >
            <Plus size={16} />
            新对话
          </button>
        </div>
      </Col.Item>
    </Col>
  );
}
