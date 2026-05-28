import { Sun, Moon } from 'lucide-react';
import { useStore } from '../../store';

export function ThemeToggle() {
  const darkMode = useStore((s) => s.darkMode);
  const toggleDarkMode = useStore((s) => s.toggleDarkMode);

  return (
    <button
      onClick={toggleDarkMode}
      className="flex items-center gap-2 text-gray-600 hover:text-gray-900 dark:text-gray-400 dark:hover:text-gray-100 px-3 py-2 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors text-sm w-full"
      title={darkMode ? '切换到浅色模式' : '切换到深色模式'}
    >
      {darkMode ? <Sun size={16} /> : <Moon size={16} />}
      {darkMode ? '浅色模式' : '深色模式'}
    </button>
  );
}
