import { useEffect, useRef } from 'react';
import { Minus, Square, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ModuleBar } from './components/ModuleBar';
import { Sidebar } from './components/Sidebar';
import { ChatArea } from './components/ChatArea';
import { WelcomePage } from './components/WelcomePage';
import { SkillManagerPage } from './components/SkillManagerPage';
import { ToastContainer } from './components/Toast';
import { useStore } from './store';
import { setWindowPosition } from './api/tauri';
import './styles/global.css';

function App() {
  const fetchConversations = useStore((state) => state.fetchConversations);
  const fetchTools = useStore((state) => state.fetchTools);
  const fetchModels = useStore((state) => state.fetchModels);
  const darkMode = useStore((state) => state.darkMode);
  const currentConversation = useStore((state) => state.currentConversation);
  const currentView = useStore((state) => state.currentView);
  const sidebarOpen = useStore((state) => state.sidebarOpen);

  const appWindow = getCurrentWindow();
  const isDragging = useRef(false);
  const dragStart = useRef({ x: 0, y: 0, winX: 0, winY: 0 });
  const rafId = useRef(0);

  useEffect(() => {
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
  }, []);

  const handleTopbarMouseDown = async (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('[data-window-control]')) return;
    const pos = await appWindow.outerPosition();
    dragStart.current = { x: e.screenX, y: e.screenY, winX: pos.x, winY: pos.y };
    isDragging.current = true;
  };

  useEffect(() => {
    fetchConversations();
    fetchTools();
    fetchModels();
  }, [fetchConversations, fetchTools, fetchModels]);

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
    return currentConversation ? <ChatArea /> : <WelcomePage />;
  };

  return (
    <div className="flex flex-col h-screen w-screen bg-white dark:bg-gray-900 transition-colors">
      {/* Top Bar — always visible, draggable for frameless window */}
      <div
        onMouseDown={handleTopbarMouseDown}
        className="h-11 flex-shrink-0 bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 flex items-center gap-3 px-4 z-20 select-none"
      >
        <div className="w-6 h-6 rounded-lg bg-gradient-to-br from-purple-500 to-indigo-500 flex items-center justify-center flex-shrink-0 shadow-sm">
          <span className="text-white text-[9px] font-bold">A</span>
        </div>
        <span className="text-sm font-semibold text-gray-800 dark:text-gray-200">Agent</span>
        {currentConversation && (
          <>
            <span className="text-gray-300 dark:text-gray-600">/</span>
            <span className="text-sm text-gray-500 dark:text-gray-400 truncate max-w-[200px]">
              {currentConversation.title}
            </span>
          </>
        )}
        <div className="flex-1" />
        <span className="text-[11px] text-gray-400 dark:text-gray-500">
          {currentView === 'skill-manager' ? '技能管理' : ''}
        </span>
        {/* Window controls */}
        <div data-window-control className="flex items-center h-full -mr-2">
          <button
            onClick={() => appWindow.minimize()}
            className="w-11 h-full flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          >
            <Minus size={14} />
          </button>
          <button
            onClick={() => appWindow.toggleMaximize()}
            className="w-11 h-full flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          >
            <Square size={12} />
          </button>
          <button
            onClick={() => appWindow.close()}
            className="w-11 h-full flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-white hover:bg-red-500 transition-colors"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Main Row */}
      <div className="flex flex-1 min-h-0">
        {/* Module Bar — always visible, far left */}
        <ModuleBar />

        {/* Contextual Sidebar — only for chat module */}
        {currentView === 'chat' && sidebarOpen && (
          <div className="w-64 flex-shrink-0">
            <Sidebar />
          </div>
        )}

        {/* Main Content */}
        <div className="flex-1 flex flex-col min-h-0">
          {renderMainContent()}
        </div>
      </div>

      <ToastContainer />
    </div>
  );
}

export default App;
