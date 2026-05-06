import { useCallback, useEffect, useMemo, useState } from 'react';
import type { PlannedOperation } from '@emailibrium/types';
import { mediumGroupKey, sourceLabel } from './groupKey';

export interface RiskAcknowledgerProps {
  rows: PlannedOperation[];
  onAcksChange(highSeqs: number[], mediumGroupKeys: string[]): void;
}

interface MediumGroup {
  key: string;
  accountId: string;
  label: string;
  rowCount: number;
}

interface HighRowDescriptor {
  seq: number;
  accountId: string;
  description: string;
  ackLabel: string;
}

function describeHighRow(op: PlannedOperation): string {
  if (op.opKind === 'predicate') {
    return `Predicate · ${op.action.type} · ~${op.projectedCount.toLocaleString()} emails`;
  }
  const target = op.target ? ` → ${op.target.name}` : '';
  return `${op.action.type}${target} · email ${op.emailId ?? '(unspecified)'}`;
}

/**
 * Phase D — action-specific high-risk acknowledgment copy. Matches §B.3
 * wireframe and ADR-030 §6 (risk levels). Pure presentation; no logic.
 */
function highAckLabel(op: PlannedOperation): string {
  const action = op.action;
  switch (action.type) {
    case 'delete':
      if (action.permanent) {
        return 'I understand this will permanently delete this email — it is NOT recoverable from Trash.';
      }
      return 'I understand this will move this email to Trash — recoverable for 30 days.';
    case 'unsubscribe':
      if (action.method === 'mailto') {
        return 'I understand this will send an email to the unsubscribe address — visible to the sender.';
      }
      if (action.method === 'webLink') {
        return 'I understand this will open a webpage in my browser — outcome unknown.';
      }
      return 'I understand this will send an unsubscribe request — visible to the sender.';
    default:
      return `I understand the consequences of #${op.seq}: ${describeHighRow(op)}`;
  }
}

export function RiskAcknowledger({ rows, onAcksChange }: RiskAcknowledgerProps) {
  const [ackedHigh, setAckedHigh] = useState<Set<number>>(() => new Set());
  const [ackedMedium, setAckedMedium] = useState<Set<string>>(() => new Set());

  const { highRows, mediumGroups } = useMemo(() => {
    const high: HighRowDescriptor[] = [];
    const groupMap = new Map<string, MediumGroup>();
    for (const op of rows) {
      if (op.risk === 'high') {
        high.push({
          seq: op.seq,
          accountId: op.accountId,
          description: describeHighRow(op),
          ackLabel: highAckLabel(op),
        });
      } else if (op.risk === 'medium') {
        const key = mediumGroupKey(op.accountId, op.source);
        const existing = groupMap.get(key);
        if (existing) {
          existing.rowCount += 1;
        } else {
          groupMap.set(key, {
            key,
            accountId: op.accountId,
            label: sourceLabel(op.source),
            rowCount: 1,
          });
        }
      }
    }
    high.sort((a, b) => a.seq - b.seq);
    return {
      highRows: high,
      mediumGroups: Array.from(groupMap.values()).sort((a, b) => a.key.localeCompare(b.key)),
    };
  }, [rows]);

  // Surface state up to parent. Only emit when ack sets actually change.
  useEffect(() => {
    onAcksChange(
      Array.from(ackedHigh).sort((a, b) => a - b),
      Array.from(ackedMedium).sort(),
    );
  }, [ackedHigh, ackedMedium, onAcksChange]);

  const toggleHigh = useCallback((seq: number) => {
    setAckedHigh((prev) => {
      const next = new Set(prev);
      if (next.has(seq)) next.delete(seq);
      else next.add(seq);
      return next;
    });
  }, []);

  const toggleMedium = useCallback((key: string) => {
    setAckedMedium((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const ackAllHigh = useCallback(() => {
    setAckedHigh(new Set(highRows.map((r) => r.seq)));
  }, [highRows]);

  const ackAllMedium = useCallback(() => {
    setAckedMedium(new Set(mediumGroups.map((g) => g.key)));
  }, [mediumGroups]);

  if (highRows.length === 0 && mediumGroups.length === 0) {
    return null;
  }

  return (
    <section
      aria-labelledby="risk-ack-heading"
      className="rounded-lg border border-amber-200 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-4 space-y-4"
    >
      <header className="flex items-center justify-between gap-3">
        <div>
          <h2
            id="risk-ack-heading"
            className="text-sm font-semibold text-amber-900 dark:text-amber-200"
          >
            Acknowledge risky operations
          </h2>
          <p className="text-xs text-amber-700 dark:text-amber-300 mt-0.5">
            High-risk rows must be acknowledged individually. Medium-risk operations are grouped by
            source — acknowledge once per group.
          </p>
        </div>
      </header>

      {mediumGroups.length > 0 && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-amber-800 dark:text-amber-300">
              Medium-risk groups ({ackedMedium.size}/{mediumGroups.length} acknowledged)
            </h3>
            <button
              type="button"
              onClick={ackAllMedium}
              className="text-xs font-medium text-amber-700 dark:text-amber-300 underline hover:no-underline"
            >
              Ack all groups
            </button>
          </div>
          <ul role="list" className="space-y-1">
            {mediumGroups.map((g) => (
              <li key={g.key} role="listitem">
                <label className="flex items-center gap-2 text-xs text-gray-800 dark:text-gray-200 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={ackedMedium.has(g.key)}
                    onChange={() => toggleMedium(g.key)}
                    className="h-4 w-4 rounded border-gray-300 text-amber-600 focus:ring-amber-500"
                    aria-label={`Acknowledge group: ${g.label} (${g.rowCount} rows)`}
                  />
                  <span className="font-medium">{g.label}</span>
                  <span className="text-gray-500 dark:text-gray-400">
                    · {g.rowCount.toLocaleString()} row{g.rowCount === 1 ? '' : 's'}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </div>
      )}

      {highRows.length > 0 && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-red-800 dark:text-red-300">
              High-risk rows ({ackedHigh.size}/{highRows.length} acknowledged)
            </h3>
            <button
              type="button"
              onClick={ackAllHigh}
              className="text-xs font-medium text-red-700 dark:text-red-300 underline hover:no-underline"
            >
              Ack all rows
            </button>
          </div>
          <ul role="list" className="space-y-1 max-h-48 overflow-y-auto">
            {highRows.map((r) => (
              <li key={r.seq} role="listitem">
                <label className="flex items-start gap-2 text-xs text-gray-800 dark:text-gray-200 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={ackedHigh.has(r.seq)}
                    onChange={() => toggleHigh(r.seq)}
                    className="mt-0.5 h-4 w-4 rounded border-gray-300 text-red-600 focus:ring-red-500"
                    aria-label={`Acknowledge high-risk row #${r.seq}: ${r.ackLabel}`}
                  />
                  <span className="font-mono text-[10px] text-gray-500 dark:text-gray-400 mt-0.5">
                    #{r.seq}
                  </span>
                  <span className="flex-1">
                    <span className="block">{r.ackLabel}</span>
                    <span className="block text-[10px] text-gray-500 dark:text-gray-400 truncate">
                      {r.description}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
