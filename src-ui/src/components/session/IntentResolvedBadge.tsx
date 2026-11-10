import type { IntentResolvedEvent } from '../../api/intent';

interface IntentResolvedBadgeProps {
  intent: IntentResolvedEvent;
}

export function IntentResolvedBadge({ intent }: IntentResolvedBadgeProps) {
  const names = intent.intents.map((item) => item.intent_name);
  if (names.length === 0) return null;

  const candidateSummary =
    intent.candidates.length > 0
      ? `候选: ${intent.candidates
          .slice(0, 3)
          .map((c) => `${c.name} ${(c.score * 100).toFixed(0)}%`)
          .join(' · ')}`
      : null;

  const tooltip = [
    `来源: ${intent.source}`,
    `置信度: ${(intent.confidence * 100).toFixed(0)}%`,
    candidateSummary,
  ]
    .filter(Boolean)
    .join(' · ');

  if (intent.uncertain) {
    const uncertainNames = intent.intents
      .filter((item) => item.uncertain)
      .map((item) => item.intent_name);
    const possible = uncertainNames.length > 0 ? uncertainNames : names;
    const suffix =
      names.length > possible.length ? ` 等 ${names.length} 个意图` : '';

    return (
      <span
        title={tooltip}
        className="inline-flex items-center gap-1 px-2 py-0.5 bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 text-[11px] rounded-md border border-amber-100 dark:border-amber-800 cursor-help"
        data-testid="intent-resolved-badge"
      >
        <span className="truncate max-w-[240px]">
          可能：{possible.join('、')}
          {suffix}
        </span>
      </span>
    );
  }

  const summary =
    names.length === 1 ? names[0] : `${names.length} 个意图：${names.join('、')}`;

  return (
    <span
      title={tooltip}
      className="inline-flex items-center gap-1 px-2 py-0.5 bg-emerald-50 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-400 text-[11px] rounded-md border border-emerald-100 dark:border-emerald-800 cursor-help"
      data-testid="intent-resolved-badge"
    >
      {intent.domain_name && (
        <>
          <span className="truncate max-w-[120px]">{intent.domain_name}</span>
          <span className="text-emerald-400 dark:text-emerald-600 flex-shrink-0">/</span>
        </>
      )}
      <span className="truncate max-w-[240px]">{summary}</span>
    </span>
  );
}

interface IntentUncertaintyNoticeProps {
  intent: IntentResolvedEvent | null;
}

export function IntentUncertaintyNotice({ intent }: IntentUncertaintyNoticeProps) {
  if (!intent || !intent.uncertain || intent.intents.length === 0) return null;

  const names = intent.intents.map((item) => item.intent_name);

  return (
    <div
      className="px-6 py-2 text-xs text-amber-700 dark:text-amber-400 bg-amber-50/80 dark:bg-amber-900/20 border-b border-amber-100 dark:border-amber-800/50 flex-shrink-0"
      data-testid="intent-uncertainty-notice"
    >
      不确定意图，先按「{names.join('、')}」继续；可切换领域后重发
    </div>
  );
}
