import type { IntentResolvedEvent } from '../../api/intent';

interface IntentResolvedBadgeProps {
  intent: IntentResolvedEvent;
}

export function IntentResolvedBadge({ intent }: IntentResolvedBadgeProps) {
  const parts = [
    `来源: ${intent.source}`,
    `置信度: ${(intent.confidence * 100).toFixed(0)}%`,
    intent.widened ? '已扩域' : null,
  ].filter(Boolean);

  return (
    <span
      title={parts.join(' · ')}
      className="inline-flex items-center gap-1 px-2 py-0.5 bg-emerald-50 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-400 text-[11px] rounded-md border border-emerald-100 dark:border-emerald-800 cursor-help"
      data-testid="intent-resolved-badge"
    >
      <span className="truncate max-w-[120px]">{intent.domain_name}</span>
      <span className="text-emerald-400 dark:text-emerald-600 flex-shrink-0">/</span>
      <span className="truncate max-w-[120px]">{intent.intent_name}</span>
    </span>
  );
}
