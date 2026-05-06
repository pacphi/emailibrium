// Phase C: Apply orchestrator React hook.
//
// Wraps `POST /apply` + `GET /apply/:jobId/stream` (SSE) + `POST /cancel`.
// The reducer handles all 10 wire variants emitted by
// backend/src/cleanup/orchestrator/sse.rs. The first event on every
// subscription is `snapshot`, which replaces (does not accumulate)
// `counts` + `accountStates`.
//
// Phase D: SSE op events now carry `actionType` (camelCase serde tag of the
// row's PlanAction). The reducer aggregates per-action counters lazily,
// initializing zero-buckets on first sight of each action type. The
// `snapshot` event resets perAction to {} (snapshots are point-in-time
// cumulative — future events forward-reconstruct the breakdown).
//
// Reconnect strategy: on EventSource error, exponential backoff (1s, 2s, 4s,
// up to 30s). We stop reconnecting once the job is in a terminal state
// (`finished` / `cancelled` / `error`).

import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AccountSnapshotState,
  ApplyEvent,
  ApplyJobCounts,
  ApplyOptions,
  JobId,
  PlanId,
  RiskMax,
} from '@emailibrium/types';
import { applyStreamUrl, beginApply, cancelApply } from '@emailibrium/api';

export type ApplyJobUiState = 'idle' | 'starting' | 'running' | 'done' | 'error' | 'cancelled';

export interface PerActionCounters {
  applied: number;
  failed: number;
  skipped: number;
}

export interface UseCleanupApplyState {
  jobId: JobId | null;
  jobState: ApplyJobUiState;
  counts: ApplyJobCounts;
  /**
   * Phase D: per-action breakdown driven by the SSE `actionType` discriminator
   * on opApplied/opFailed/opSkipped. Keys are camelCase PlanAction tags
   * (e.g. 'archive', 'addLabel', 'delete').
   */
  perAction: Record<string, PerActionCounters>;
  accountStates: Record<string, AccountSnapshotState>;
  /** Bounded ring buffer of recent events for UI debug; oldest evicted. */
  events: ApplyEvent[];
  error: string | null;
}

export interface UseCleanupApplyResult extends UseCleanupApplyState {
  startApply(planId: PlanId, riskMax: RiskMax, opts: ApplyOptions): Promise<void>;
  cancelApply(): Promise<void>;
  reconnect(): void;
}

const MAX_EVENT_BUFFER = 100;
const BACKOFF_MS = [1000, 2000, 4000, 8000, 16000, 30000];

const emptyCounts = (): ApplyJobCounts => ({
  applied: 0,
  failed: 0,
  skipped: 0,
  pending: 0,
});

const initialState: UseCleanupApplyState = {
  jobId: null,
  jobState: 'idle',
  counts: emptyCounts(),
  perAction: {},
  accountStates: {},
  events: [],
  error: null,
};

function bumpPerAction(
  prev: Record<string, PerActionCounters>,
  actionType: string,
  field: keyof PerActionCounters,
): Record<string, PerActionCounters> {
  const existing = prev[actionType] ?? { applied: 0, failed: 0, skipped: 0 };
  return {
    ...prev,
    [actionType]: { ...existing, [field]: existing[field] + 1 },
  };
}

function reduceEvent(prev: UseCleanupApplyState, ev: ApplyEvent): UseCleanupApplyState {
  const events = [...prev.events, ev];
  if (events.length > MAX_EVENT_BUFFER) events.splice(0, events.length - MAX_EVENT_BUFFER);

  switch (ev.type) {
    case 'snapshot':
      return {
        ...prev,
        jobId: ev.jobId,
        counts: ev.counts,
        // Snapshots are cumulative point-in-time — they don't carry per-action
        // detail. Reset and let subsequent op events forward-reconstruct.
        perAction: {},
        accountStates: ev.accountStates,
        // Snapshot itself does not flip jobState; subsequent `started` /
        // `progress` / `finished` carry the lifecycle signal.
        jobState: prev.jobState === 'idle' ? 'running' : prev.jobState,
        events,
      };
    case 'started': {
      // Aggregate per-account totals into a starting `pending` baseline so
      // the progress bar can render before any opApplied arrives.
      const summed: ApplyJobCounts = emptyCounts();
      for (const c of Object.values(ev.totalsByAccount)) {
        summed.applied += c.applied;
        summed.failed += c.failed;
        summed.skipped += c.skipped;
        summed.pending += c.pending;
      }
      return {
        ...prev,
        jobId: ev.jobId,
        jobState: 'running',
        counts: summed,
        events,
      };
    }
    case 'opApplied':
      return {
        ...prev,
        counts: {
          ...prev.counts,
          applied: prev.counts.applied + 1,
          pending: Math.max(0, prev.counts.pending - 1),
        },
        perAction: bumpPerAction(prev.perAction, ev.actionType, 'applied'),
        events,
      };
    case 'opFailed':
      return {
        ...prev,
        counts: {
          ...prev.counts,
          failed: prev.counts.failed + 1,
          pending: Math.max(0, prev.counts.pending - 1),
        },
        perAction: bumpPerAction(prev.perAction, ev.actionType, 'failed'),
        events,
      };
    case 'opSkipped': {
      const skippedByReason = { ...(prev.counts.skippedByReason ?? {}) };
      skippedByReason[ev.reason] = (skippedByReason[ev.reason] ?? 0) + 1;
      return {
        ...prev,
        counts: {
          ...prev.counts,
          skipped: prev.counts.skipped + 1,
          pending: Math.max(0, prev.counts.pending - 1),
          skippedByReason,
        },
        perAction: bumpPerAction(prev.perAction, ev.actionType, 'skipped'),
        events,
      };
    }
    case 'predicateExpanded':
      // Newly-materialized rows enter pending. The orchestrator will then
      // emit per-row events for each child.
      return {
        ...prev,
        counts: {
          ...prev.counts,
          pending: prev.counts.pending + ev.producedRows,
        },
        events,
      };
    case 'accountPaused':
      return {
        ...prev,
        accountStates: {
          ...prev.accountStates,
          [ev.accountId]: { paused: true, pauseReason: ev.reason },
        },
        events,
      };
    case 'accountResumed':
      return {
        ...prev,
        accountStates: {
          ...prev.accountStates,
          [ev.accountId]: { paused: false },
        },
        events,
      };
    case 'progress':
      // Authoritative counts replacement (drops drift from per-op
      // accumulation if the broadcast channel dropped a frame).
      return {
        ...prev,
        counts: ev.counts,
        events,
      };
    case 'finished':
      return {
        ...prev,
        jobState:
          ev.status === 'cancelled' ? 'cancelled' : ev.status === 'failed' ? 'error' : 'done',
        counts: ev.counts,
        events,
      };
    default:
      return { ...prev, events };
  }
}

function isTerminal(s: ApplyJobUiState): boolean {
  return s === 'done' || s === 'cancelled' || s === 'error';
}

export function useCleanupApply(userId: string): UseCleanupApplyResult {
  const [state, setState] = useState<UseCleanupApplyState>(initialState);

  // Refs hold mutable wiring that must not retrigger effects on every render.
  const sourceRef = useRef<EventSource | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const backoffIndexRef = useRef(0);
  const jobStateRef = useRef<ApplyJobUiState>('idle');
  const jobIdRef = useRef<JobId | null>(null);

  const closeStream = useCallback(() => {
    if (reconnectTimerRef.current !== null) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (sourceRef.current) {
      sourceRef.current.close();
      sourceRef.current = null;
    }
  }, []);

  const openStream = useCallback(
    (jobId: JobId) => {
      closeStream();
      const url = applyStreamUrl(jobId, userId);
      const es = new EventSource(url, { withCredentials: true });
      sourceRef.current = es;

      es.onmessage = (event: MessageEvent) => {
        try {
          const ev = JSON.parse(event.data) as ApplyEvent;
          setState((prev) => {
            const next = reduceEvent(prev, ev);
            jobStateRef.current = next.jobState;
            return next;
          });
          // Successful frame ⇒ reset backoff.
          backoffIndexRef.current = 0;

          if (ev.type === 'finished') {
            // Server is done; close the connection cleanly.
            closeStream();
          }
        } catch {
          // Malformed messages are silently dropped (matches createSSEStream).
        }
      };

      es.onerror = () => {
        // EventSource auto-reconnects; we layer our own backoff to cap it
        // and to flip into `error` UI when the job is not yet terminal.
        if (isTerminal(jobStateRef.current)) {
          closeStream();
          return;
        }
        // Tear down the native auto-reconnect and schedule our own.
        if (sourceRef.current) {
          sourceRef.current.close();
          sourceRef.current = null;
        }
        const idx = Math.min(backoffIndexRef.current, BACKOFF_MS.length - 1);
        const delay = BACKOFF_MS[idx];
        backoffIndexRef.current = idx + 1;
        setState((prev) =>
          prev.error === null ? { ...prev, error: 'Lost connection — reconnecting…' } : prev,
        );
        reconnectTimerRef.current = setTimeout(() => {
          reconnectTimerRef.current = null;
          if (jobIdRef.current && !isTerminal(jobStateRef.current)) {
            openStream(jobIdRef.current);
          }
        }, delay);
      };
    },
    [closeStream, userId],
  );

  const startApply = useCallback(
    async (planId: PlanId, riskMax: RiskMax, opts: ApplyOptions) => {
      setState({
        ...initialState,
        jobState: 'starting',
      });
      jobStateRef.current = 'starting';
      jobIdRef.current = null;
      backoffIndexRef.current = 0;
      try {
        const { jobId } = await beginApply(planId, userId, riskMax, opts);
        jobIdRef.current = jobId;
        setState((prev) => ({ ...prev, jobId, jobState: 'running' }));
        jobStateRef.current = 'running';
        openStream(jobId);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setState((prev) => ({ ...prev, jobState: 'error', error: msg }));
        jobStateRef.current = 'error';
      }
    },
    [openStream, userId],
  );

  const cancel = useCallback(async () => {
    const id = jobIdRef.current;
    if (!id) return;
    try {
      await cancelApply(id);
      // The orchestrator will emit `finished{ status: 'cancelled' }`; we let
      // the reducer set jobState. As a defensive fallback, mark cancelled
      // optimistically here so the UI doesn't appear stuck if the SSE frame
      // is delayed.
      setState((prev) => (isTerminal(prev.jobState) ? prev : { ...prev, jobState: 'cancelled' }));
      jobStateRef.current = 'cancelled';
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setState((prev) => ({ ...prev, error: msg }));
    }
  }, []);

  const reconnect = useCallback(() => {
    if (jobIdRef.current && !isTerminal(jobStateRef.current)) {
      backoffIndexRef.current = 0;
      openStream(jobIdRef.current);
    }
  }, [openStream]);

  // Cleanup on unmount.
  useEffect(() => {
    return () => {
      closeStream();
    };
  }, [closeStream]);

  return {
    ...state,
    startApply,
    cancelApply: cancel,
    reconnect,
  };
}
