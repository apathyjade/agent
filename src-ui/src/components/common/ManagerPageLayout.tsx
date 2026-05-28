import { ArrowLeft } from 'lucide-react';
import { Col } from '@jelper/component';
import { useState } from 'react';
import type { ReactNode } from 'react';

export interface TabConfig {
  key: string;
  label: string;
  icon?: ReactNode;
}

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

  // ── Tabs configuration ──
  /** Tab definitions — when provided, renders a horizontal tab bar in the header */
  tabs?: TabConfig[];
  /** Controlled active tab key (defaults to first tab) */
  activeTab?: string;
  /** Callback when active tab changes */
  onTabChange?: (key: string) => void;
}

export function ManagerPageLayout({
  icon,
  title,
  subtitle,
  headerActions,
  searchBar,
  tabs,
  activeTab: controlledTab,
  onTabChange,
  onBack,
  className = '',
  children,
}: ManagerPageLayoutProps) {
  // Uncontrolled fallback: manage active tab internally
  const [internalTab, setInternalTab] = useState(tabs?.[0]?.key ?? '');
  const isControlled = controlledTab !== undefined;
  const currentTab = isControlled ? controlledTab : internalTab;

  const handleTabChange = (key: string) => {
    if (!isControlled) setInternalTab(key);
    onTabChange?.(key);
  };
  return (
    <div className={`h-full bg-gray-50 dark:bg-gray-900/50 ${className}`}>
      <Col>
        {/* Header */}
        <Col.Item $fixed>
          <div className="bg-white dark:bg-gray-800">
            {/* Title + search section — border-b only when no tabs */}
            <div className={`px-6 py-4 ${tabs?.length ? '' : 'border-b border-gray-100 dark:border-gray-700/60'}`}>
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

            {/* Tab bar — sits flush below padding, its border-b separates header from content */}
            {tabs && tabs.length > 0 && (
              <div className="border-b border-gray-100 dark:border-gray-700/60 px-6">
                <div className="flex items-center gap-2">
                  {tabs.map((tab) => (
                    <button
                      key={tab.key}
                      onClick={() => handleTabChange(tab.key)}
                      className={`flex items-center gap-1.5 px-2.5 py-2 text-xs font-medium transition-all border-b-2 -mb-px ${
                        currentTab === tab.key
                          ? 'border-purple-500 text-gray-900 dark:text-gray-100'
                          : 'border-transparent text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300'
                      }`}
                    >
                      {tab.icon}
                      {tab.label}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </Col.Item>

        {/* Content */}
        <Col.Item $scale={1}>
          <div className="px-6 py-5 overflow-auto h-full">
            {children}
          </div>
        </Col.Item>
      </Col>
    </div>
  );
}
