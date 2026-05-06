import { useEffect, useState } from 'react';
import type { PlanId, PlanSource } from '@emailibrium/types';
import { usePlanSamples } from '../hooks/usePlanSamples';
import { sourceKey } from './groupKey';

export interface SampleEmailPeekProps {
  planId: PlanId;
  userId: string;
  source: PlanSource;
  /** Phase D telemetry — fires on the first open of this peek instance. */
  onOpened?: () => void;
}

export function SampleEmailPeek({ planId, userId, source, onOpened }: SampleEmailPeekProps) {
  const [open, setOpen] = useState(false);
  const [emailIds, setEmailIds] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const samples = usePlanSamples(planId, userId);
  const key = sourceKey(source);

  useEffect(() => {
    if (!open || emailIds !== null || loading) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    samples
      .getSample(key, 5)
      .then((ids) => {
        if (!cancelled) setEmailIds(ids);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, emailIds, loading, samples, key]);

  return (
    <div className="text-xs">
      <button
        type="button"
        onClick={() => {
          setOpen((o) => {
            const next = !o;
            if (next && !o && onOpened) onOpened();
            return next;
          });
        }}
        aria-expanded={open}
        className="text-blue-600 dark:text-blue-400 underline hover:no-underline"
      >
        {open ? 'Hide sample' : 'Peek sample emails'}
      </button>
      {open && (
        <div className="mt-2 rounded-md border border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 p-2">
          {loading && <p className="text-gray-500 dark:text-gray-400">Loading sample…</p>}
          {error && (
            <p className="text-red-600 dark:text-red-400" role="alert">
              Failed to load sample: {error}
            </p>
          )}
          {!loading && !error && emailIds !== null && emailIds.length === 0 && (
            <p className="text-gray-500 dark:text-gray-400">No sample emails available.</p>
          )}
          {!loading && !error && emailIds && emailIds.length > 0 && (
            <ul
              role="list"
              className="space-y-0.5 font-mono text-[11px] text-gray-700 dark:text-gray-300"
            >
              {emailIds.slice(0, 5).map((id) => (
                <li key={id} role="listitem" className="truncate">
                  · {id}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
