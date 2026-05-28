import { useState, useEffect, useCallback } from 'react';
import { Plus, Trash2, MessageSquare, Archive, RotateCcw, Kanban, ChevronDown, ChevronRight, MoreHorizontal } from 'lucide-react';
import { Popconfirm, Dropdown } from 'antd';
import { Col } from '@jelper/component';
import { useStore } from '../../store';
import * as mgmt from '../../api/management';
import type { Project, Session } from '../../types';

export function Sidebar() {
  const {
    sessions,
    currentSession,
    selectSession,
    deleteSession,
    updateSessionTitle,
    archiveSession,
    unarchiveSession,
  } = useStore();

  // ── Local state ──
  const [projects, setProjects] = useState<Project[]>([]);
  const [expandedProjectIds, setExpandedProjectIds] = useState<Set<string>>(new Set());
  const [activeProjectId, _setActiveProjectId] = useState<string | null>(null);
  const setActiveProjectId = useCallback((id: string | null) => {
    _setActiveProjectId(id);
    if (id) {
      // Only expand the active project, collapse all others
      setExpandedProjectIds(new Set([id]));
    }
  }, []);

  // Session editing
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');

  // Delete confirmation
  const [activePopconfirmId, setActivePopconfirmId] = useState<string | null>(null);

  // Archive filter
  const [showArchived, setShowArchived] = useState(false);

  // ── Load projects ──
  // Load projects on mount
  useEffect(() => {
    (async () => {
      try {
        const list = await mgmt.listProjects();
        setProjects(list);
        // Auto-activate the most recent project
        if (list.length > 0) {
          const sorted = [...list].sort((a, b) =>
            new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime()
          );
          const latest = sorted[0];
          _setActiveProjectId(latest.id);
          setExpandedProjectIds(new Set([latest.id]));
          const store = useStore.getState();
          const recent = store.sessions
            .filter((s: Session) => s.project_id === latest.id && !s.archived)
            .sort((a: Session, b: Session) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())[0];
          if (recent) {
            store.selectSession(recent.id);
          } else {
            store.setPendingProjectId(latest.id);
          }
        }
      } catch {
        // Backend command not yet available
      }
    })();
  }, []);

  // ── Session grouping ──
  const filteredSessions = sessions.filter((s) =>
    showArchived ? s.archived : !s.archived
  );

  const sessionsByProject = new Map<string | null, Session[]>();
  for (const sess of filteredSessions) {
    const pid = sess.project_id ?? null;
    if (!sessionsByProject.has(pid)) {
      sessionsByProject.set(pid, []);
    }
    sessionsByProject.get(pid)!.push(sess);
  }
  // Sort sessions within each project: most recent first
  for (const [, sessList] of sessionsByProject) {
    sessList.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime());
  }

  const projectMap = new Map(projects.map((p) => [p.id, p]));

  // Active project first, then rest sorted by last updated (most recent first)
  const projectIds = [...projects].sort((a, b) => {
    if (a.id === activeProjectId) return -1;
    if (b.id === activeProjectId) return 1;
    return new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime();
  }).map((p) => p.id);

  const hasSessionsWithoutProject = sessionsByProject.has(null) && sessionsByProject.get(null)!.length > 0;

  // ── Handlers ──

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

  const handleNewChat = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: '打开项目目录' });
      if (!selected) return;
      const folderPath = selected as string;

      // Check if a project with this path already exists
      const existing = projects.find((p) => p.path === folderPath);
      if (existing) {
        setActiveProjectId(existing.id);
        const recent = getMostRecentSession(existing.id);
        if (recent) {
          useStore.getState().selectSession(recent.id);
        } else {
          useStore.getState().setPendingProjectId(existing.id);
          useStore.getState().setCurrentView('chat');
          useStore.getState().newChat();
        }
        return;
      }

      const folderName = folderPath.replace(/[\\/]$/, '').split(/[\\/]/).pop() || 'New Project';
      const project = await mgmt.createProject(folderName, folderPath);
      setProjects((prev) => [project, ...prev]);
      setActiveProjectId(project.id);
      useStore.getState().setPendingProjectId(project.id);
      useStore.getState().setCurrentView('chat');
      useStore.getState().newChat();
    } catch {
      // Not running in Tauri
    }
  };

  const handleCreateSession = async (projectId?: string) => {
    if (projectId) setActiveProjectId(projectId);
    // Set pending project so WelcomePage can create session under it
    useStore.getState().setPendingProjectId(projectId ?? null);
    // Navigate to welcome page
    useStore.getState().setCurrentView('chat');
    useStore.getState().newChat();
  };

  // Helper: get most recent session for a project (or null)
  const getMostRecentSession = useCallback((projectId: string): Session | undefined => {
    return sessions
      .filter((s) => s.project_id === projectId && !s.archived)
      .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())[0];
  }, [sessions]);

  const handleDeleteProject = async (projectId: string) => {
    try {
      await mgmt.deleteProject(projectId);
      setProjects((prev) => prev.filter((p) => p.id !== projectId));
    } catch (err) {
      console.error('Failed to delete project:', err);
    }
  };

  const toggleProjectExpand = (projectId: string) => {
    setExpandedProjectIds((prev) => {
      const next = new Set(prev);
      if (next.has(projectId)) {
        next.delete(projectId);
      } else {
        next.add(projectId);
      }
      return next;
    });
  };

  // ── Render helpers ──

  const renderSessionItem = (sess: Session) => (
    <div
      key={sess.id}
      className={`flex items-center gap-2 px-3 py-2 rounded-lg mb-1 cursor-pointer group transition-all ml-5 ${
        currentSession?.id === sess.id
          ? 'bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300'
          : 'hover:bg-purple-50/50 dark:hover:bg-gray-700/50 text-gray-700 dark:text-gray-300'
      }`}
    >
      {sess.mode === 'autonomous' || (sess as any).mode === 'autonomous' ? (
        <span className="w-2 h-2 rounded-full bg-purple-500 flex-shrink-0" title="自主模式" />
      ) : (
        <MessageSquare size={14} className="flex-shrink-0 text-gray-400 dark:text-gray-500" />
      )}
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
          title="确认删除此会话？"
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
  );

  const renderProject = (project: Project) => {
    const isExpanded = expandedProjectIds.has(project.id);
    const projectSessions = sessionsByProject.get(project.id) ?? [];

    return (
      <div key={project.id} className="mb-1">
        {/* Project header */}
        <div
          className={`flex items-center gap-2 px-3 py-2 rounded-lg cursor-pointer group transition-all ${
            activeProjectId === project.id
              ? 'bg-purple-50/80 dark:bg-purple-900/20 ring-1 ring-purple-200 dark:ring-purple-800'
              : 'hover:bg-purple-50/50 dark:hover:bg-gray-700/50'
          }`}
          onClick={() => { setActiveProjectId(project.id); toggleProjectExpand(project.id); }}
        >
          <button className="p-0.5 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 dark:text-gray-500 transition-colors flex-shrink-0">
            {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </button>
          <Kanban size={16} className={`flex-shrink-0 ${isExpanded ? 'text-purple-500' : 'text-purple-400'}`} />
          <div className="flex-1 min-w-0">
            <span className="text-sm font-medium text-gray-800 dark:text-gray-200 truncate block">
              {project.name}
            </span>
            <span className="text-[10px] text-gray-400 dark:text-gray-500 truncate block">
              {project.path}
            </span>
          </div>
          <Dropdown
            menu={{
              items: [
                {
                  key: 'new-session',
                  icon: <MessageSquare size={14} />,
                  label: '新建会话',
                  onClick: (e) => { e.domEvent.stopPropagation(); handleCreateSession(project.id); },
                },
                { type: 'divider' },
                {
                  key: 'delete',
                  icon: <Trash2 size={14} />,
                  label: '删除项目',
                  danger: true,
                  onClick: (e) => { e.domEvent.stopPropagation(); handleDeleteProject(project.id); },
                },
              ],
              style: { minWidth: 140 },
            }}
            trigger={['click']}
            placement="bottomRight"
          >
            <button
              onClick={(e) => e.stopPropagation()}
              className="p-1 rounded hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 transition-all flex-shrink-0"
            >
              <MoreHorizontal size={16} />
            </button>
          </Dropdown>
        </div>

        {/* Sessions nested under project */}
        {isExpanded && projectSessions.map(renderSessionItem)}
      </div>
    );
  };

  const hasAnyContent = projectIds.length > 0 || hasSessionsWithoutProject;
  const emptyLabel = showArchived ? '没有已归档的项目' : '暂无项目';

  return (
    <>
      <style>{`
        @keyframes projectSlideIn {
          from { opacity: 0.5; transform: translateY(-10px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        .animate-project-active {
          animation: projectSlideIn 0.25s ease-out;
        }
      `}</style>
    <Col className="h-full bg-white dark:bg-gray-800/70 border-r border-purple-100/50 dark:border-purple-900/30 transition-colors backdrop-blur-sm">
      <Col.Item $scale={1}>
        <div className="overflow-y-auto h-full px-2 flex flex-col">
          {/* Filter toggle */}
          <div className="flex items-center gap-1 px-3 py-2">
            <button
              onClick={() => setShowArchived(false)}
              className={`text-xs px-2.5 py-1 rounded-lg font-medium transition-all ${
                !showArchived
                  ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300'
                  : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-400'
              }`}
            >
              项目
            </button>
            <button
              onClick={() => setShowArchived(true)}
              className={`text-xs px-2.5 py-1 rounded-lg font-medium transition-all ${
                showArchived
                  ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300'
                  : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-400'
              }`}
            >
              已归档
            </button>
          </div>

          {/* Empty state */}
          {!hasAnyContent && (
            <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
              <Kanban size={32} className="text-gray-300 dark:text-gray-600 mb-3" />
              <p className="text-sm text-gray-400 dark:text-gray-500 mb-1">{emptyLabel}</p>
              <p className="text-xs text-gray-300 dark:text-gray-600">
                点击下方按钮打开项目
              </p>
            </div>
          )}

          {/* Active project at top */}
          {activeProjectId && projectMap.has(activeProjectId) && (
            <div key={activeProjectId} className="mb-2 animate-project-active">
              {renderProject(projectMap.get(activeProjectId)!)}
            </div>
          )}

          {/* Spacer to push inactive projects to bottom */}
          {projectIds.filter((pid) => pid !== activeProjectId).length > 0 && (
            <div className="flex-1 min-h-[8px]" />
          )}

          {/* All other projects at bottom (collapsed) */}
          {projectIds.filter((pid) => pid !== activeProjectId).map((pid) => {
            const p = projectMap.get(pid)!;
            return renderProject(p);
          })}

          {/* Uncategorized sessions */}
          {hasSessionsWithoutProject && (
            <div className="mb-1">
              <div className="flex items-center gap-2 px-3 py-2 rounded-lg">
                <Kanban size={14} className="text-gray-400 flex-shrink-0" />
                <span className="text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                  未分类
                </span>
              </div>
              {sessionsByProject.get(null)!.map(renderSessionItem)}
            </div>
          )}
        </div>
      </Col.Item>

      {/* Bottom bar */}
      <Col.Item $fixed>
        <div className="p-3 border-t border-purple-100/50 dark:border-purple-900/30 space-y-2">
          <button
            onClick={handleNewChat}
            className="w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-xl transition-all shadow-sm bg-gradient-to-r from-purple-600 to-indigo-600 hover:from-purple-700 hover:to-indigo-700 text-white"
          >
            <Plus size={16} />
            打开项目
          </button>
        </div>
      </Col.Item>


    </Col>
    </>
  );
}
