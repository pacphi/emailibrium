import { useState } from 'react';
import type { PlanWarning } from '@emailibrium/types';
import { sourceLabel } from './groupKey';

export interface PlanWarningsBannerProps {
  warnings: PlanWarning[];
}

function warningKey(w: PlanWarning, idx: number): string {
  switch (w.type) {
    case 'largeGroup':
      return `largeGroup:${idx}:${w.projectedCount}`;
    case 'targetConflict':
      return `targetConflict:${w.accountId}:${w.emailId}`;
    case 'planExceedsThreshold':
      return `planExceedsThreshold:${w.totalCount}`;
    case 'lowConfidence':
      return `lowConfidence:${w.ruleId}`;
  }
}

function describeWarning(w: PlanWarning): { title: string; body: string } {
  switch (w.type) {
    case 'targetConflict':
      return {
        title: 'Conflicting sources for the same email',
        body: `Email ${w.emailId} on account ${w.accountId} is targeted by ${w.sources.length} sources: ${w.sources.map(sourceLabel).join('; ')}.`,
      };
    case 'largeGroup':
      return {
        title: 'Large group — verify the sample',
        body: `${sourceLabel(w.source)} expands to ~${w.projectedCount.toLocaleString()} emails (>10k threshold). Spot-check the sample before applying.`,
      };
    case 'planExceedsThreshold':
      return {
        title: 'Plan exceeds 100k operations',
        body: `This plan touches ${w.totalCount.toLocaleString()} operations. You will be required to type a confirmation phrase before applying high-risk actions.`,
      };
    case 'lowConfidence':
      return {
        title: 'Low-confidence rule match',
        body: `Rule ${w.ruleId}: ${w.reason}`,
      };
  }
}

export function PlanWarningsBanner({ warnings }: PlanWarningsBannerProps) {
  const [dismissed, setDismissed] = useState<Set<string>>(() => new Set());

  const visible = warnings
    .map((w, idx) => ({ w, key: warningKey(w, idx) }))
    .filter(({ key }) => !dismissed.has(key));

  if (visible.length === 0) return null;

  return (
    <section aria-label="Plan warnings" className="space-y-2">
      {visible.map(({ w, key }) => {
        const { title, body } = describeWarning(w);
        return (
          <div
            key={key}
            role="alert"
            className="rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 px-4 py-3 flex items-start gap-3"
          >
            <span aria-hidden="true" className="text-amber-700 dark:text-amber-300 font-bold">
              !
            </span>
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium text-amber-900 dark:text-amber-200">{title}</p>
              <p className="mt-0.5 text-xs text-amber-700 dark:text-amber-300">{body}</p>
            </div>
            <button
              type="button"
              onClick={() => setDismissed((prev) => new Set(prev).add(key))}
              className="text-xs text-amber-700 dark:text-amber-300 underline hover:no-underline"
              aria-label={`Dismiss warning: ${title}`}
            >
              Dismiss
            </button>
          </div>
        );
      })}
    </section>
  );
}
