import { LifecycleBadge } from './LifecycleBadge';
import type { RuntimeVersion, VersionLifecycle } from '../../types';

interface VersionTimelineProps {
  versions: RuntimeVersion[];
  currentVersion?: string | null;
  onSelectVersion?: (version: string) => void;
}

// ── Version comparison helpers ──

function parseVersion(v: string): number[] {
  return v.split('.').map(n => {
    const parsed = parseInt(n, 10);
    return isNaN(parsed) ? 0 : parsed;
  });
}

function compareVersions(a: string, b: string): number {
  const aParts = parseVersion(a);
  const bParts = parseVersion(b);
  const len = Math.max(aParts.length, bParts.length);
  for (let i = 0; i < len; i++) {
    const diff = (aParts[i] || 0) - (bParts[i] || 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

// ── Lifecycle heuristics ──

function getVersionLifecycle(
  v: RuntimeVersion,
  allSorted: RuntimeVersion[],
): VersionLifecycle {
  // Newest version is "latest"
  if (allSorted.length > 0 && allSorted[0].version === v.version) return 'latest';
  // Has LTS codename → LTS
  if (v.lts) return 'lts';
  // Stable but not LTS → determine by age
  if (v.is_stable && allSorted.length > 0) {
    const latestParts = parseVersion(allSorted[0].version);
    const thisParts = parseVersion(v.version);
    const majorDiff = (latestParts[0] || 0) - (thisParts[0] || 0);
    if (majorDiff >= 3) return 'eol';
    if (majorDiff >= 2) return 'maintenance';
  }
  return 'active';
}

// ── Dot color per lifecycle ──

const DOT_COLORS: Record<VersionLifecycle, string> = {
  latest:      'bg-green-500',
  lts:         'bg-green-500',
  active:      'bg-blue-500',
  maintenance: 'bg-yellow-500',
  eol:         'bg-red-500',
};

// ── Component ──

export function VersionTimeline({
  versions,
  currentVersion,
  onSelectVersion,
}: VersionTimelineProps) {
  // Sort newest → oldest
  const sorted = [...versions].sort((a, b) => compareVersions(b.version, a.version));

  if (sorted.length === 0) {
    return (
      <div className="p-8 text-center text-gray-400 dark:text-gray-500">
        <p className="text-sm">暂无版本记录</p>
      </div>
    );
  }

  return (
    <div className="relative">
      {/* Vertical timeline line */}
      <div className="absolute left-[11px] top-2 bottom-2 w-0.5 bg-gray-200 dark:bg-gray-600" />

      <div className="space-y-0">
        {sorted.map((v, _idx) => {
          const lifecycle = getVersionLifecycle(v, sorted);
          const isCurrent = currentVersion !== null && currentVersion !== undefined && v.version === currentVersion;
          const dotColor = DOT_COLORS[lifecycle];

          return (
            <div
              key={v.version}
              onClick={() => onSelectVersion?.(v.version)}
              className={`relative flex items-start gap-4 pl-8 pr-3 py-2.5 rounded-lg cursor-pointer transition-colors ${
                isCurrent
                  ? 'bg-purple-50 dark:bg-purple-900/15 border border-purple-200 dark:border-purple-800'
                  : 'hover:bg-gray-50 dark:hover:bg-gray-700/30 border border-transparent'
              } ${onSelectVersion ? 'cursor-pointer' : 'cursor-default'}`}
            >
              {/* Timeline dot */}
              <div className="absolute left-[5px] top-1/2 -translate-y-1/2 z-10">
                <div className={`w-3.5 h-3.5 rounded-full border-2 border-white dark:border-gray-800 ${dotColor}`} />
              </div>

              {/* Version info */}
              <div className="flex-1 min-w-0 flex items-center gap-3">
                <span className="text-sm font-mono font-medium text-gray-900 dark:text-gray-100">
                  {v.version}
                </span>
                {v.lts && (
                  <span className="text-[10px] text-green-600 dark:text-green-400 font-medium">
                    {v.lts}
                  </span>
                )}
                <div className="flex-shrink-0">
                  <LifecycleBadge lifecycle={lifecycle} lts={v.lts} />
                </div>
              </div>

              {/* Date */}
              <div className="flex-shrink-0 text-right">
                {v.release_date ? (
                  <span className="text-xs text-gray-400 dark:text-gray-500">
                    {v.release_date}
                  </span>
                ) : (
                  <span className="text-xs text-gray-300 dark:text-gray-600">日期未知</span>
                )}
              </div>

              {/* Current marker */}
              {isCurrent && (
                <span className="text-[10px] text-purple-600 dark:text-purple-400 font-medium flex-shrink-0">
                  当前使用
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
