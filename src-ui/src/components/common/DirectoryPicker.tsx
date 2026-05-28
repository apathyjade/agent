import { FolderOpen } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';

interface DirectoryPickerProps {
  value: string;
  onChange: (path: string) => void;
  placeholder?: string;
  disabled?: boolean;
}

/**
 * Directory picker that opens the system's native folder selection dialog.
 * Wraps Tauri's `@tauri-apps/plugin-dialog` `open()` with directory filter.
 */
export function DirectoryPicker({ value, onChange, placeholder, disabled }: DirectoryPickerProps) {
  const handleBrowse = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: '选择目录',
    });
    if (selected && typeof selected === 'string') {
      onChange(selected);
    }
  };

  return (
    <div className="flex gap-2">
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder || '选择目录...'}
        disabled={disabled}
        className="flex-1 bg-white dark:bg-gray-900 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-1.5 text-xs font-mono focus:outline-none focus:ring-2 focus:ring-purple-500 dark:text-gray-100 disabled:opacity-50"
      />
      <button
        onClick={handleBrowse}
        disabled={disabled}
        className="flex items-center gap-1 px-3 py-1.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-600 dark:text-gray-300 rounded-lg text-xs transition-colors disabled:opacity-50"
        title="浏览..."
      >
        <FolderOpen size={14} />
        浏览
      </button>
    </div>
  );
}
