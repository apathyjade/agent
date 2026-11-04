import { useEffect, useState } from 'react';
import { Target } from 'lucide-react';
import { useStore } from '../../store';
import { listIntentDomains } from '../../api/intent';
import type { IntentDomainInfo } from '../../api/intent';

const SELECT_ID = 'session-intent-domain-select';

export function SessionIntentSelector() {
  const currentSession = useStore((s) => s.currentSession);
  const intentDomainId = useStore((s) => s.intentDomainId);
  const intentDomainMode = useStore((s) => s.intentDomainMode);
  const setSessionIntentDomain = useStore((s) => s.setSessionIntentDomain);
  const [domains, setDomains] = useState<IntentDomainInfo[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!currentSession) return;
    let cancelled = false;
    setLoading(true);
    listIntentDomains()
      .then((list) => {
        if (!cancelled) setDomains(list);
      })
      .catch((err) => {
        console.error('Failed to load intent domains:', err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [currentSession]);

  if (!currentSession) return null;

  const value = intentDomainMode === 'pinned' && intentDomainId ? intentDomainId : '';

  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const domainId = e.target.value || null;
    void setSessionIntentDomain(currentSession.id, domainId);
  };

  return (
    <div className="flex items-center gap-1.5">
      <Target size={12} className="text-purple-500 flex-shrink-0" />
      <select
        id={SELECT_ID}
        value={value}
        onChange={handleChange}
        disabled={loading}
        aria-label="会话领域"
        className="text-[11px] px-2 py-0.5 bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400 rounded-md border border-purple-100 dark:border-purple-800 focus:outline-none focus:ring-1 focus:ring-purple-500 disabled:opacity-50 cursor-pointer max-w-[140px] truncate"
      >
        <option value="">自动</option>
        {domains.map((domain) => (
          <option key={domain.id} value={domain.id}>
            {domain.name}
          </option>
        ))}
      </select>
      <span
        className={`text-[10px] px-1.5 py-0.5 rounded-full flex-shrink-0 ${
          intentDomainMode === 'auto'
            ? 'bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400'
            : 'bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-400'
        }`}
      >
        {intentDomainMode === 'auto' ? '自动' : '固定'}
      </span>
    </div>
  );
}
