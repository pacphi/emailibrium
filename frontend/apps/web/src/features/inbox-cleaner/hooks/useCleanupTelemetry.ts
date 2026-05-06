// Phase D — telemetry hook for the cleanup review surface.
//
// Emits the `cleanup_plan_reviewed` event when:
//   - the consuming component unmounts (after a minimum on-screen dwell), or
//   - the user clicks Apply (whichever fires first).
//
// Telemetry is best-effort: backend errors are swallowed silently so a 404
// (parallel backend agent has not landed the endpoint yet) cannot block the
// UX. The hook also tracks two interaction counters that the caller wires
// in via callbacks: expanded groups (PlanDiffGroup expand toggles) and
// samples viewed (SampleEmailPeek opens).

import { useCallback, useEffect, useRef } from 'react';
import { emitCleanupReviewed } from '@emailibrium/api';
import type { PlanId } from '@emailibrium/types';

/** Minimum dwell on the review screen before unmount-emit fires (ms). */
const MIN_DWELL_MS = 2_000;

export interface UseCleanupTelemetryResult {
  /** Increment the "expanded groups" counter — call from PlanDiffGroup. */
  noteGroupExpanded(): void;
  /** Increment the "samples viewed" counter — call from SampleEmailPeek. */
  noteSampleViewed(): void;
  /** Manually fire the event (e.g. just before Apply). Idempotent. */
  emitNow(): void;
}

export function useCleanupTelemetry(planId: PlanId | null): UseCleanupTelemetryResult {
  const startedAtRef = useRef<number>(Date.now());
  const expandedGroupsRef = useRef<number>(0);
  const samplesViewedRef = useRef<number>(0);
  const firedRef = useRef<boolean>(false);

  // Reset start timer when the planId changes (i.e. we're now reviewing a
  // different plan).
  useEffect(() => {
    startedAtRef.current = Date.now();
    expandedGroupsRef.current = 0;
    samplesViewedRef.current = 0;
    firedRef.current = false;
  }, [planId]);

  const fire = useCallback(() => {
    if (firedRef.current || !planId) return;
    const dwell = Date.now() - startedAtRef.current;
    if (dwell < MIN_DWELL_MS) return;
    firedRef.current = true;
    // Fire-and-forget: telemetry must NEVER block UX.
    void emitCleanupReviewed({
      planId,
      timeOnReviewMs: dwell,
      expandedGroups: expandedGroupsRef.current,
      samplesViewed: samplesViewedRef.current,
    }).catch(() => {
      /* best-effort */
    });
  }, [planId]);

  // Emit on unmount.
  useEffect(() => {
    return () => {
      fire();
    };
  }, [fire]);

  const noteGroupExpanded = useCallback(() => {
    expandedGroupsRef.current += 1;
  }, []);

  const noteSampleViewed = useCallback(() => {
    samplesViewedRef.current += 1;
  }, []);

  const emitNow = useCallback(() => {
    fire();
  }, [fire]);

  return { noteGroupExpanded, noteSampleViewed, emitNow };
}
