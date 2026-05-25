import { useEffect, useRef } from 'react';
import { Minus, Square, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Col, Row } from '@jelper/component';
import { ModuleBar } from './components/ModuleBar';
import { Sidebar } from './components/Sidebar';
import { ChatArea } from './components/ChatArea';
import { WelcomePage } from './components/WelcomePage';
import { SkillManagerPage } from './components/SkillManagerPage';
import { McpManagerPage } from './components/McpManagerPage';
import { MemoryManagerPage } from './components/MemoryManagerPage';
import { PersonaManagerPage } from './components/PersonaManagerPage';
import { RuntimeManagerPage } from './components/RuntimeManagerPage';
import { SettingsPage } from './components/SettingsPage';
import { WorkflowManagerPage } from './components/WorkflowManagerPage';
import { ToastContainer } from './components/Toast';
import { useStore } from './store';
import { setWindowPosition } from './api/tauri';
import './styles/global.css';

function App() {
  const fetchSessions = useStore((state) => state.fetchSessions);
  const fetchTools = useStore((state) => state.fetchTools);
  const fetchModels = useStore((state) => state.fetchModels);
  const darkMode = useStore((state) => state.darkMode);
  const currentSession = useStore((state) => state.currentSession);
  const currentView = useStore((state) => state.currentView);
  const sidebarOpen = useStore((state) => state.sidebarOpen);

  let appWindow: ReturnType<typeof getCurrentWindow> | null = null;
  try {
    appWindow = getCurrentWindow();
  } catch {
    // Not running in Tauri (e.g. browser dev)
  }
  const isDragging = useRef(false);
  const dragStart = useRef({ x: 0, y: 0, winX: 0, winY: 0 });
  const rafId = useRef(0);

  useEffect(() => {
    if (!appWindow) return;
    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      cancelAnimationFrame(rafId.current);
      rafId.current = requestAnimationFrame(() => {
        const dx = e.screenX - dragStart.current.x;
        const dy = e.screenY - dragStart.current.y;
        setWindowPosition(
          dragStart.current.winX + dx,
          dragStart.current.winY + dy,
        );
      });
    };
    const handleMouseUp = () => {
      isDragging.current = false;
      cancelAnimationFrame(rafId.current);
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      cancelAnimationFrame(rafId.current);
    };
  }, [appWindow]);

  const handleTopbarMouseDown = async (e: React.MouseEvent) => {
    if (!appWindow) return;
    if ((e.target as HTMLElement).closest('[data-window-control]')) return;
    try {
      const pos = await appWindow.outerPosition();
      dragStart.current = { x: e.screenX, y: e.screenY, winX: pos.x, winY: pos.y };
      isDragging.current = true;
    } catch {
      // Not in Tauri
    }
  };

  useEffect(() => {
    fetchSessions();
    fetchTools();
    fetchModels();
  }, [fetchSessions, fetchTools, fetchModels]);

  // Init dark mode on mount
  useEffect(() => {
    if (darkMode) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, []);

  const renderMainContent = () => {
    if (currentView === 'skill-manager') {
      return <SkillManagerPage />;
    }
    if (currentView === 'mcp-manager') {
      return <McpManagerPage />;
    }
    if (currentView === 'memory-manager') {
      return <MemoryManagerPage />;
    }
    if (currentView === 'persona-manager') {
      return <PersonaManagerPage />;
    }
    if (currentView === 'runtime-manager') {
      return <RuntimeManagerPage />;
    }
    if (currentView === 'settings') {
      return <SettingsPage />;
    }
    if (currentView === 'workflows') {
      return <WorkflowManagerPage />;
    }
    return currentSession ? <ChatArea /> : <WelcomePage />;
  };

  return (
    <Col>
      {/* Top Bar — always visible, draggable for frameless window */}
      <Col.Item $fixed>
        <div
          onMouseDown={handleTopbarMouseDown}
          className="h-11 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 flex items-center gap-3 px-4 select-none"
        >
          <div className="w-6 h-6 rounded-lg bg-gradient-to-br from-purple-500 to-indigo-500 flex items-center justify-center flex-shrink-0 shadow-sm">
            <span className="text-white text-[9px] font-bold">A</span>
          </div>
          <span className="text-sm font-semibold text-gray-800 dark:text-gray-200">Agent</span>
          <div className="flex-1" />
          <span className="text-[11px] text-gray-400 dark:text-gray-500">
            {currentView === 'skill-manager' ? '技能管理' :
             currentView === 'mcp-manager' ? 'MCP 连接' :
             currentView === 'memory-manager' ? '记忆系统' :
             currentView === 'persona-manager' ? '虚拟人管理' :
             currentView === 'runtime-manager' ? '运行时管理' :
             currentView === 'settings' ? '系统设置' :
             currentView === 'workflows' ? '工作流' : ''}
          </span>
          {/* Window controls */}
          <div data-window-control className="flex items-center h-full -mr-2">
            <button
              onClick={() => appWindow?.minimize()}
              className="w-11 h-full flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            >
              <Minus size={14} />
            </button>
            <button
              onClick={() => appWindow?.toggleMaximize()}
              className="w-11 h-full flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            >
              <Square size={12} />
            </button>
            <button
              onClick={() => appWindow?.close()}
              className="w-11 h-full flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-white hover:bg-red-500 transition-colors"
            >
              <X size={16} />
            </button>
          </div>
        </div>
      </Col.Item>

      {/* Main Row — Col.RowItem is both a Col item and a Row container (no extra <Row>) */}
      <Col.RowItem $scale={1}>
        <Row.Item $fixed>
          <ModuleBar />
        </Row.Item>

        {currentView === 'chat' && sidebarOpen && (
          <Row.Item $width={256} $fixed>
            <Sidebar />
          </Row.Item>
        )}

        <Row.Item $scale={1}>
          {renderMainContent()}
        </Row.Item>
      </Col.RowItem>

      <ToastContainer />
    </Col>
  );
}

export default App;
