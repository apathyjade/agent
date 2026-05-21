import { ArrowLeft } from 'lucide-react';
import type { ReactNode } from 'react';

interface ManagerPageLayoutProps {
  /** Icon element displayed in the header gradient badge */
  icon: ReactNode;
  /** Page title */
  title: string;
  /** Subtitle shown below the title */
  subtitle?: string;
  /** Optional action buttons rendered on the right side of the header */
  headerActions?: ReactNode;
  /** Optional search / filter bar rendered below the header top row */
  searchBar?: ReactNode;
  /** When provided, renders a back button on the left of the header */
  onBack?: () => void;
  /** Additional classes applied to the outer container */
  className?: string;
  /** Main scrollable content */
  children: ReactNode;
}

/**
 * Shared layout component for management pages (Skill, MCP, Workflow, etc.).
 * Follows the same header style as SkillManagerPage.
 */
export function ManagerPageLayout({
  icon,
  title,
  subtitle,
  headerActions,
  searchBar,
  onBack,
  className = '',
  children,
}: ManagerPageLayoutProps) {
  return (
    <div className={`h-full flex flex-col bg-gray-50 dark:bg-gray-900/50 ${className}`}>
      {/* Header */}
      <div className="bg-white dark:bg-gray-800 border-b border-gray-100 dark:border-gray-700/60 px-6 py-4 flex-shrink-0">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            {onBack && (
              <button
                onClick={onBack}
                className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors -ml-1"
                title="返回"
              >
                <ArrowLeft size={18} />
              </button>
            )}
            <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-purple-600 to-indigo-600 flex items-center justify-center shadow-sm flex-shrink-0">
              {icon}
            </div>
            <div>
              <h1 className="text-lg font-bold text-gray-900 dark:text-gray-100">{title}</h1>
              {subtitle && (
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{subtitle}</p>
              )}
            </div>
          </div>

          {headerActions && (
            <div className="flex items-center gap-2">{headerActions}</div>
          )}
        </div>

        {searchBar && <div className="mt-4">{searchBar}</div>}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-6 py-5">{children}</div>
    </div>
  );
}
