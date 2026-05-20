import { MessageSquare, BrainCircuit, Settings, Sun, Moon } from 'lucide-react';
import { useStore } from '../store';
import { SettingsModal } from './SettingsModal';
import { useState } from 'react';

export function ModuleBar() {
  const { currentView, setCurrentView, sidebarOpen, setSidebarOpen, darkMode, toggleDarkMode } = useStore();
  const [showSettings, setShowSettings] = useState(false);

  const handleChatClick = () => {
    if (currentView === 'chat') {
      setSidebarOpen(!sidebarOpen);
    } else {
      setCurrentView('chat');
      setSidebarOpen(true);
    }
  };

  const handleSkillsClick = () => {
    setCurrentView('skill-manager');
    setSidebarOpen(false);
  };

  return (
    <>
      <div className="w-[50px] flex-shrink-0 bg-white dark:bg-gray-800 flex flex-col items-center py-3 border-r border-gray-200 dark:border-gray-700 select-none">

        {/* Module icons */}
        <div className="flex-1 flex flex-col items-center gap-1">
          <button
            onClick={handleChatClick}
            className={`w-9 h-9 rounded-lg flex items-center justify-center transition-all relative ${
              currentView === 'chat'
                ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400'
                : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
            }`}
            title="对话"
          >
            {currentView === 'chat' && (
              <div className="absolute left-0 top-1/2 -translate-y-1/2 w-[2.5px] h-5 bg-purple-500 rounded-r-full" />
            )}
            <MessageSquare size={18} />
          </button>

          <button
            onClick={handleSkillsClick}
            className={`w-9 h-9 rounded-lg flex items-center justify-center transition-all relative ${
              currentView === 'skill-manager'
                ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400'
                : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
            }`}
            title="技能管理"
          >
            {currentView === 'skill-manager' && (
              <div className="absolute left-0 top-1/2 -translate-y-1/2 w-[2.5px] h-5 bg-purple-500 rounded-r-full" />
            )}
            <BrainCircuit size={18} />
          </button>
        </div>

        {/* Bottom: Theme + Settings */}
        <button
          onClick={toggleDarkMode}
          className="w-9 h-9 rounded-lg flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-all"
          title={darkMode ? '切换浅色模式' : '切换深色模式'}
        >
          {darkMode ? <Sun size={18} /> : <Moon size={18} />}
        </button>
        <div className="w-5 h-px bg-gray-200 dark:bg-gray-700 my-1.5" />
        <button
          onClick={() => setShowSettings(true)}
          className="w-9 h-9 rounded-lg flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-all"
          title="设置"
        >
          <Settings size={18} />
        </button>
      </div>

      <SettingsModal isOpen={showSettings} onClose={() => setShowSettings(false)} />
    </>
  );
}
