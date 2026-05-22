import { type VersionLifecycle } from '../types';

interface LifecycleBadgeProps {
  lifecycle: VersionLifecycle;
  lts?: string | null;
  size?: 'sm' | 'md';
}

const LIFECYCLE_CONFIG: Record<VersionLifecycle, { label: string; emoji: string; className: string }> = {
  latest:       { label: '最新',   emoji: '🆕', className: 'text-blue-600 dark:text-blue-400 bg-blue-100 dark:bg-blue-900/30' },
  lts:          { label: 'LTS',    emoji: '✅', className: 'text-green-600 dark:text-green-400 bg-green-100 dark:bg-green-900/30' },
  active:       { label: '活跃',   emoji: '🟢', className: 'text-green-600 dark:text-green-400 bg-green-100 dark:bg-green-900/30' },
  maintenance:  { label: '维护期', emoji: '🟡', className: 'text-yellow-600 dark:text-yellow-400 bg-yellow-100 dark:bg-yellow-900/30' },
  eol:          { label: '已停止', emoji: '🔴', className: 'text-red-600 dark:text-red-400 bg-red-100 dark:bg-red-900/30' },
};

export function LifecycleBadge({ lifecycle, lts, size = 'sm' }: LifecycleBadgeProps) {
  const cfg = LIFECYCLE_CONFIG[lifecycle];
  const sizeClass = size === 'sm' ? 'text-[10px] px-1.5 py-0.5' : 'text-xs px-2 py-1';

  return (
    <span className={`inline-flex items-center gap-1 rounded-full font-medium ${sizeClass} ${cfg.className}`}>
      <span>{cfg.emoji}</span>
      <span>{lifecycle === 'lts' && lts ? `LTS ${lts}` : cfg.label}</span>
    </span>
  );
}
