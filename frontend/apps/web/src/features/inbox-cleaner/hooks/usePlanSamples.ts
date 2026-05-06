// Lazy sample-fetcher for cleanup plan rows.
//
// `getSample(source)` calls `/api/v1/cleanup/plan/:id/sample?source=…&n=…` on
// demand and caches the resulting email-id list per `source` key. The UI
// can call this when expanding a predicate row to show a few representative
// messages without forcing a list of every row up-front.
//
// `source` is the raw query value the backend `SampleQuery::source` accepts —
// see backend/src/cleanup/api/plan.rs. In Phase A this is treated as an
// opaque string by the repository; Phase C may formalize it.

import { useCallback, useState } from 'react';
import type { PlanId } from '@emailibrium/types';
import { samplePlanOperations } from '@emailibrium/api';

export interface UsePlanSamplesResult {
  /** Map of source → cached email ids, populated as `getSample` is called. */
  samples: Record<string, string[]>;
  /** Fetch (or return cached) sample email ids for a given `source`. */
  getSample: (source: string, n?: number) => Promise<string[]>;
  /** True while at least one in-flight fetch is pending. */
  isLoading: boolean;
  /** Last error from a sample fetch, if any. */
  error: unknown;
  /** Drop the cache (e.g. after refresh). */
  reset: () => void;
}

export function usePlanSamples(planId: PlanId | null, userId: string): UsePlanSamplesResult {
  const [samples, setSamples] = useState<Record<string, string[]>>({});
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [error, setError] = useState<unknown>(null);

  const getSample = useCallback(
    async (source: string, n = 5): Promise<string[]> => {
      if (!planId || !userId) return [];
      const cached = samples[source];
      if (cached) return cached;

      setPending((prev) => {
        const next = new Set(prev);
        next.add(source);
        return next;
      });
      try {
        const resp = await samplePlanOperations(planId, userId, source, n);
        setSamples((prev) => ({ ...prev, [source]: resp.emailIds }));
        return resp.emailIds;
      } catch (e) {
        setError(e);
        throw e;
      } finally {
        setPending((prev) => {
          const next = new Set(prev);
          next.delete(source);
          return next;
        });
      }
    },
    [planId, userId, samples],
  );

  const reset = useCallback(() => {
    setSamples({});
    setError(null);
  }, []);

  return {
    samples,
    getSample,
    isLoading: pending.size > 0,
    error,
    reset,
  };
}
