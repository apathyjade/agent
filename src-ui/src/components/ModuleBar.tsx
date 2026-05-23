import { MessageSquare, BrainCircuit, PlugZap, Workflow, Settings, Sun, Moon, Server, BookOpen, Users } from 'lucide-react';
import { Tooltip } from 'antd';
import { Col } from '@jelper/component';
import { useStore } from '../store';

export function ModuleBar() {
  const { currentView, setCurrentView, sidebarOpen, setSidebarOpen, darkMode, toggleDarkMode } = useStore();

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

  const handleMcpClick = () => {
    setCurrentView('mcp-manager');
    setSidebarOpen(false);
  };

  const handleRuntimeClick = () => {
    setCurrentView('runtime-manager');
    setSidebarOpen(false);
  };

  const handleMemoryClick = () => {
    setCurrentView('memory-manager');
    setSidebarOpen(false);
  };

  const handlePersonaClick = () => {
    setCurrentView('persona-manager');
    setSidebarOpen(false);
  };

  const handleSettingsClick = () => {
    setCurrentView('settings');
    setSidebarOpen(false);
  };

  const handleWorkflowsClick = () => {
    setCurrentView('workflows');
    setSidebarOpen(false);
  };

  const btnClass = (active: boolean) =>
    `w-9 h-9 rounded-lg flex items-center justify-center transition-all relative ${
      active
        ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400'
        : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
    }`;

  const activeIndicator = (active: boolean) =>
    active ? <div className="absolute left-0 top-1/2 -translate-y-1/2 w-[2.5px] h-5 bg-purple-500 rounded-r-full" /> : null;

  return (
    <div className="w-[50px] h-full bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 select-none">
      <Col style={{ height: '100%' }} $align="center">
        {/* Module icons */}
        <Col.Item $scale={1}>
          <div className="flex flex-col items-center gap-1 py-3">
            <Tooltip title="对话" placement="right">
              <button onClick={handleChatClick} className={btnClass(currentView === 'chat')}>
                {activeIndicator(currentView === 'chat')}
                <MessageSquare size={18} />
              </button>
            </Tooltip>
            <Tooltip title="技能管理" placement="right">
              <button onClick={handleSkillsClick} className={btnClass(currentView === 'skill-manager')}>
                {activeIndicator(currentView === 'skill-manager')}
                <BrainCircuit size={18} />
              </button>
            </Tooltip>
            <Tooltip title="MCP 连接" placement="right">
              <button onClick={handleMcpClick} className={btnClass(currentView === 'mcp-manager')}>
                {activeIndicator(currentView === 'mcp-manager')}
                <PlugZap size={18} />
              </button>
            </Tooltip>
            <Tooltip title="记忆系统" placement="right">
              <button onClick={handleMemoryClick} className={btnClass(currentView === 'memory-manager')}>
                {activeIndicator(currentView === 'memory-manager')}
                <BookOpen size={18} />
              </button>
            </Tooltip>
            <Tooltip title="虚拟人管理" placement="right">
              <button onClick={handlePersonaClick} className={btnClass(currentView === 'persona-manager')}>
                {activeIndicator(currentView === 'persona-manager')}
                <Users size={18} />
              </button>
            </Tooltip>
            <Tooltip title="运行时管理" placement="right">
              <button onClick={handleRuntimeClick} className={btnClass(currentView === 'runtime-manager')}>
                {activeIndicator(currentView === 'runtime-manager')}
                <Server size={18} />
              </button>
            </Tooltip>
            <Tooltip title="工作流" placement="right">
              <button onClick={handleWorkflowsClick} className={btnClass(currentView === 'workflows')}>
                {activeIndicator(currentView === 'workflows')}
                <Workflow size={18} />
              </button>
            </Tooltip>
          </div>
        </Col.Item>

        {/* Bottom: Theme + Settings */}
        <Col.Item $fixed>
          <div className="flex flex-col items-center gap-1 pb-3">
            <Tooltip title={darkMode ? '切换浅色模式' : '切换深色模式'} placement="right">
              <button
                onClick={toggleDarkMode}
                className="w-9 h-9 rounded-lg flex items-center justify-center text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-all"
              >
                {darkMode ? <Sun size={18} /> : <Moon size={18} />}
              </button>
            </Tooltip>
            <div className="w-5 h-px bg-gray-200 dark:bg-gray-700 my-0.5" />
            <Tooltip title="设置" placement="right">
              <button
                onClick={handleSettingsClick}
                className={`w-9 h-9 rounded-lg flex items-center justify-center transition-all relative ${
                  currentView === 'settings'
                    ? 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400'
                    : 'text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
                }`}
              >
                {currentView === 'settings' && (
                  <div className="absolute left-0 top-1/2 -translate-y-1/2 w-[2.5px] h-5 bg-purple-500 rounded-r-full" />
                )}
                <Settings size={18} />
              </button>
            </Tooltip>
          </div>
        </Col.Item>
      </Col>
    </div>
  );
}
