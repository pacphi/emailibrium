import type { RiskLevel } from '@emailibrium/types';

interface LegendChipProps {
  level: RiskLevel;
  label: string;
  description: string;
}

const chipClasses: Record<RiskLevel, string> = {
  low: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-200 border-green-300 dark:border-green-700',
  medium:
    'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200 border-amber-300 dark:border-amber-700',
  high: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-200 border-red-300 dark:border-red-700',
};

function LegendChip({ level, label, description }: LegendChipProps) {
  return (
    <div
      className={`inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium ${chipClasses[level]}`}
      role="listitem"
    >
      <span aria-hidden="true" className="font-bold uppercase tracking-wide">
        {label}
      </span>
      <span className="hidden sm:inline opacity-80">— {description}</span>
    </div>
  );
}

export function RiskLegend() {
  return (
    <section aria-labelledby="risk-legend-heading" className="space-y-2">
      <h2 id="risk-legend-heading" className="sr-only">
        Risk legend
      </h2>
      <div className="flex flex-wrap gap-2" role="list" aria-label="Risk levels">
        <LegendChip level="low" label="Low" description="Safe & reversible (archive, label)" />
        <LegendChip
          level="medium"
          label="Medium"
          description="Bulk actions or soft-delete (recoverable)"
        />
        <LegendChip
          level="high"
          label="High"
          description="Permanent or hard to undo — requires acknowledgment"
        />
      </div>
      <p className="text-xs text-gray-500 dark:text-gray-400">
        Risk levels mix color and text — high-risk operations require explicit acknowledgment per
        row before they can be applied.
      </p>
    </section>
  );
}
