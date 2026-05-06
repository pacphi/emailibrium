import { useRef, useState, useMemo } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { PlanId, PlannedOperation, RiskLevel } from '@emailibrium/types';
import { PlanDiffRow } from './PlanDiffRow';
import { mediumGroupKey } from './groupKey';

const DEFAULT_VISIBLE = 20;
const VIRTUALIZE_THRESHOLD = 50;

export interface PlanDiffGroupProps {
  accountId: string;
  group: {
    sourceKey: string;
    sourceLabel: string;
    rows: PlannedOperation[];
    risk: RiskLevel;
  };
  planId: PlanId;
  userId: string;
  ackedHighSeqs: Set<number>;
  ackedMediumGroupKeys: Set<string>;
  onToggleHighAck(seq: number): void;
  onToggleMediumAck(groupKey: string): void;
  /** Phase D telemetry — fired when this group toggles into the expanded state. */
  onGroupExpanded?: () => void;
  /** Phase D telemetry — forwarded to each rendered PlanDiffRow. */
  onSampleViewed?: () => void;
  /** Phase D — readOnly disables ack checkboxes (history view). */
  readOnly?: boolean;
}

const riskBorderClasses: Record<RiskLevel, string> = {
  low: 'border-green-300 dark:border-green-700',
  medium: 'border-amber-300 dark:border-amber-700',
  high: 'border-red-300 dark:border-red-700',
};

const riskHeaderBgClasses: Record<RiskLevel, string> = {
  low: 'bg-green-50 dark:bg-green-900/10',
  medium: 'bg-amber-50 dark:bg-amber-900/10',
  high: 'bg-red-50 dark:bg-red-900/10',
};

export function PlanDiffGroup({
  accountId,
  group,
  planId,
  userId,
  ackedHighSeqs,
  ackedMediumGroupKeys,
  onToggleHighAck,
  onToggleMediumAck,
  onGroupExpanded,
  onSampleViewed,
  readOnly = false,
}: PlanDiffGroupProps) {
  const [expanded, setExpanded] = useState(true);
  const [showAll, setShowAll] = useState(false);
  const parentRef = useRef<HTMLDivElement | null>(null);

  const totalRows = group.rows.length;
  const shouldVirtualize = totalRows >= VIRTUALIZE_THRESHOLD;

  const visibleRows = useMemo(() => {
    if (shouldVirtualize || showAll) return group.rows;
    return group.rows.slice(0, DEFAULT_VISIBLE);
  }, [group.rows, shouldVirtualize, showAll]);

  const virtualizer = useVirtualizer({
    count: shouldVirtualize ? group.rows.length : 0,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 64,
    overscan: 8,
  });

  // Determine the medium-group key for this group (rows share accountId+source).
  const firstRow = group.rows[0];
  const groupAckKey = firstRow ? mediumGroupKey(accountId, firstRow.source) : null;
  const isMediumAcked = groupAckKey ? ackedMediumGroupKeys.has(groupAckKey) : false;

  const ackStateForRow = (op: PlannedOperation) =>
    !readOnly && op.risk === 'high'
      ? {
          isAcked: ackedHighSeqs.has(op.seq),
          onToggle: () => onToggleHighAck(op.seq),
        }
      : null;

  return (
    <section
      aria-labelledby={`group-${group.sourceKey}-heading`}
      className={`rounded-lg border ${riskBorderClasses[group.risk]} bg-white dark:bg-gray-800 overflow-hidden`}
    >
      <header
        className={`flex items-center gap-3 px-4 py-2 ${riskHeaderBgClasses[group.risk]} border-b ${riskBorderClasses[group.risk]}`}
      >
        <button
          type="button"
          onClick={() => {
            setExpanded((e) => {
              const next = !e;
              if (next && !e && onGroupExpanded) onGroupExpanded();
              return next;
            });
          }}
          aria-expanded={expanded}
          className="flex-1 text-left flex items-center gap-2 min-w-0"
        >
          <svg
            className={`w-4 h-4 transition-transform shrink-0 ${expanded ? 'rotate-90' : ''}`}
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
          </svg>
          <h3
            id={`group-${group.sourceKey}-heading`}
            className="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate"
          >
            {group.sourceLabel}
          </h3>
          <span className="text-xs text-gray-600 dark:text-gray-300 shrink-0">
            · {totalRows.toLocaleString()} row{totalRows === 1 ? '' : 's'}
          </span>
        </button>

        {!readOnly && group.risk === 'medium' && groupAckKey && (
          <label className="flex items-center gap-1.5 text-xs text-amber-800 dark:text-amber-200 cursor-pointer shrink-0">
            <input
              type="checkbox"
              checked={isMediumAcked}
              onChange={() => onToggleMediumAck(groupAckKey)}
              className="h-4 w-4 rounded border-gray-300 text-amber-600 focus:ring-amber-500"
              aria-label={`Acknowledge group: ${group.sourceLabel}`}
            />
            <span>Ack group</span>
          </label>
        )}
      </header>

      {expanded && (
        <>
          {shouldVirtualize ? (
            <div
              ref={parentRef}
              className="max-h-96 overflow-y-auto"
              aria-label={`${group.sourceLabel} operations`}
            >
              <ul
                role="list"
                style={{
                  height: virtualizer.getTotalSize(),
                  width: '100%',
                  position: 'relative',
                  margin: 0,
                  padding: 0,
                  listStyle: 'none',
                }}
              >
                {virtualizer.getVirtualItems().map((vi) => {
                  const op = group.rows[vi.index]!;
                  return (
                    <div
                      key={op.seq}
                      style={{
                        position: 'absolute',
                        top: 0,
                        left: 0,
                        width: '100%',
                        transform: `translateY(${vi.start}px)`,
                      }}
                      ref={virtualizer.measureElement}
                      data-index={vi.index}
                    >
                      <PlanDiffRow
                        op={op}
                        planId={planId}
                        userId={userId}
                        ackState={ackStateForRow(op)}
                        onSampleViewed={onSampleViewed}
                      />
                    </div>
                  );
                })}
              </ul>
            </div>
          ) : (
            <ul role="list" aria-label={`${group.sourceLabel} operations`}>
              {visibleRows.map((op) => (
                <PlanDiffRow
                  key={op.seq}
                  op={op}
                  planId={planId}
                  userId={userId}
                  ackState={ackStateForRow(op)}
                  onSampleViewed={onSampleViewed}
                />
              ))}
            </ul>
          )}

          {!shouldVirtualize && totalRows > DEFAULT_VISIBLE && !showAll && (
            <div className="px-4 py-2 border-t border-gray-100 dark:border-gray-700 text-center">
              <button
                type="button"
                onClick={() => setShowAll(true)}
                className="text-xs font-medium text-blue-600 dark:text-blue-400 underline hover:no-underline"
              >
                Show all {totalRows.toLocaleString()} rows ({totalRows - DEFAULT_VISIBLE} more)
              </button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
