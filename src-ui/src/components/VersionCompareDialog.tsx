import { X, ArrowUpCircle, ArrowDownCircle, Minus } from 'lucide-react';
import { type RuntimeVersion, type VersionLifecycle } from '../types';
import { LifecycleBadge } from './LifecycleBadge';

interface VersionCompareDialogProps {
  versions: [RuntimeVersion, RuntimeVersion];
  onClose: () => void;
}

interface RowDef {
  label: string;
  key: string;
  format: (v: RuntimeVersion) => string;
}

const ROWS: RowDef[] = [
  { label: '版本号', key: 'version', format: (v) => v.version },
  { label: '发布日期', key: 'release_date', format: (v) => formatDate(v.release_date) },
  { label: '文件大小', key: 'file_size', format: (v) => formatBytes(v.file_size) },
  { label: '生命周期', key: 'lts', format: (v) => v.lts ? `LTS ${v.lts}` : (v.is_stable ? '稳定版' : '预览版') },
  { label: '类型', key: 'is_stable', format: (v) => v.is_stable ? '稳定版' : '预览版' },
];

function getLifecycle(v: RuntimeVersion): VersionLifecycle {
  if (v.lts) return 'lts';
  if (v.is_stable) return 'latest';
  return 'active';
}

function formatDate(date: string | null): string {
  if (!date) return '-';
  try {
    return new Date(date).toLocaleDateString('zh-CN');
  } catch {
    return '-';
  }
}

function formatBytes(bytes: number | null): string {
  if (bytes === null || bytes === undefined) return '-';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface DiffIndicatorProps {
  valueA: string | number;
  valueB: string | number;
}

function DiffIndicator({ valueA, valueB }: DiffIndicatorProps) {
  if (valueA === valueB) {
    return <Minus className="w-4 h-4 text-gray-400" />;
  }
  const a = typeof valueA === 'string' ? valueA : valueA;
  const b = typeof valueB === 'string' ? valueB : valueB;
  const diff = String(a) > String(b) ? 'up' : 'down';
  if (diff === 'up') {
    return <ArrowUpCircle className="w-4 h-4 text-green-500" />;
  }
  return <ArrowDownCircle className="w-4 h-4 text-red-500" />;
}

export function VersionCompareDialog({ versions, onClose }: VersionCompareDialogProps) {
  const [vA, vB] = versions;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-[520px] max-h-[80vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-gray-100">版本对比</h2>
          <button
            onClick={onClose}
            className="p-1 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="px-6 py-4">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-gray-100 dark:border-gray-700">
                <th className="text-left py-2 pr-4 text-gray-500 dark:text-gray-400 font-medium w-[100px]" />
                <th className="text-left py-2 pr-4 text-gray-900 dark:text-gray-100 font-medium">
                  <div className="flex items-center gap-2">
                    <LifecycleBadge lifecycle={getLifecycle(vA)} lts={vA.lts} />
                    <span className="text-xs text-gray-500">{vA.version}</span>
                  </div>
                </th>
                <th className="text-left py-2 text-gray-900 dark:text-gray-100 font-medium">
                  <div className="flex items-center gap-2">
                    <LifecycleBadge lifecycle={getLifecycle(vB)} lts={vB.lts} />
                    <span className="text-xs text-gray-500">{vB.version}</span>
                  </div>
                </th>
                <th className="text-left py-2 pl-4 w-[28px]" />
              </tr>
            </thead>
            <tbody>
              {ROWS.map((row) => {
                const valA = row.format(vA);
                const valB = row.format(vB);
                return (
                  <tr key={row.key} className="border-b border-gray-50 dark:border-gray-700/50">
                    <td className="py-3 pr-4 text-gray-500 dark:text-gray-400">{row.label}</td>
                    <td className="py-3 pr-4 text-gray-900 dark:text-gray-100">{valA}</td>
                    <td className="py-3 text-gray-900 dark:text-gray-100">{valB}</td>
                    <td className="py-3 pl-4">
                      <DiffIndicator valueA={valA} valueB={valB} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>

          <div className="mt-6">
            <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-2">下载地址</h3>
            <div className="space-y-1.5">
              <a
                href={vA.url}
                target="_blank"
                rel="noopener noreferrer"
                className="block text-sm text-blue-600 dark:text-blue-400 hover:underline truncate"
              >
                {vA.display_name}: {vA.url}
              </a>
              <a
                href={vB.url}
                target="_blank"
                rel="noopener noreferrer"
                className="block text-sm text-blue-600 dark:text-blue-400 hover:underline truncate"
              >
                {vB.display_name}: {vB.url}
              </a>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
