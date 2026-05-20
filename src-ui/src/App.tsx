import { useEffect } from 'react';
import { ModuleBar } from './components/ModuleBar';
import { Sidebar } from './components/Sidebar';
import { ChatArea } from './components/ChatArea';
import { WelcomePage } from './components/WelcomePage';
import { SkillManagerPage } from './components/SkillManagerPage';
import { ToastContainer } from './components/Toast';
import { useStore } from './store';
import './styles/global.css';

function App() {
  const fetchConversations = useStore((state) => state.fetchConversations);
  const fetchTools = useStore((state) => state.fetchTools);
  const fetchModels = useStore((state) => state.fetchModels);
  const darkMode = useStore((state) => state.darkMode);
  const currentConversation = useStore((state) => state.currentConversation);
  const currentView = useStore((state) => state.currentView);
  const sidebarOpen = useStore((state) => state.sidebarOpen);

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
    <div className="flex h-screen w-screen bg-white dark:bg-gray-900 transition-colors">
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

      <ToastContainer />
    </div>
  );
}

export default App;
