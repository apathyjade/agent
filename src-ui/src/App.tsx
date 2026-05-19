import { useEffect, useState } from 'react';
import { Sidebar } from './components/Sidebar';
import { ChatArea } from './components/ChatArea';
import { WelcomePage } from './components/WelcomePage';
import { ToastContainer } from './components/Toast';
import { useStore } from './store';
import './styles/global.css';

function App() {
  const fetchConversations = useStore((state) => state.fetchConversations);
  const fetchTools = useStore((state) => state.fetchTools);
  const fetchModels = useStore((state) => state.fetchModels);
  const darkMode = useStore((state) => state.darkMode);
  const currentConversation = useStore((state) => state.currentConversation);
  const [sidebarOpen, setSidebarOpen] = useState(true);

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

  return (
    <div className="flex h-screen w-screen bg-white dark:bg-gray-900 transition-colors">
      {sidebarOpen ? (
        <div className="w-64 flex-shrink-0">
          <Sidebar onClose={() => setSidebarOpen(false)} />
        </div>
      ) : (
        <div className="w-10 flex-shrink-0 border-r border-gray-100 dark:border-gray-700 flex items-start justify-center pt-4 bg-gray-50 dark:bg-gray-800">
          <button
            onClick={() => setSidebarOpen(true)}
            className="p-2 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors"
            title="打开侧边栏"
          >
            <svg className="w-5 h-5 text-gray-600 dark:text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6h16M4 12h16M4 18h16" />
            </svg>
          </button>
        </div>
      )}
      
      <div className="flex-1 flex flex-col min-h-0">
        {currentConversation ? <ChatArea /> : <WelcomePage />}
      </div>

      <ToastContainer />
    </div>
  );
}

export default App;
