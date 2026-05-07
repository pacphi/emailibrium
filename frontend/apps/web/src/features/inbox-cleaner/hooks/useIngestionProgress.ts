import { useState, useEffect, useCallback, useRef } from 'react';
import type { IngestionProgress } from '@emailibrium/types';
import {
  createIngestionStream,
  pauseIngestion,
  resumeIngestion,
  getIngestionProgress,
} from '@emailibrium/api';

export interface Discovery {
  id: string;
  type: 'subscription' | 'cluster' | 'pattern';
  message: string;
  timestamp: number;
}

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

/** How long to wait for the first SSE event before falling back to REST polling. */
const FAST_COMPLETE_TIMEOUT_MS = 2_000;

export function useIngestionProgress(jobId: string) {
  const [progress, setProgress] = useState<IngestionProgress | null>(null);
  const [discoveries, setDiscoveries] = useState<Discovery[]>([]);
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>('connecting');
  const [isPaused, setIsPaused] = useState(false);
  const prevProgress = useRef<IngestionProgress | null>(null);
  const settledRef = useRef(false);

  useEffect(() => {
    if (!jobId) return;

    settledRef.current = false;
    setConnectionStatus('connecting');
    setProgress(null);

    const applyCompletion = (snapshot: {
      total?: number;
      processed?: number;
      embedded?: number;
      categorized?: number;
      failed?: number;
      emailsPerSecond?: number;
    }) => {
      settledRef.current = true;
      setProgress({
        jobId,
        phase: 'complete',
        total: snapshot.total ?? 0,
        processed: snapshot.processed ?? 0,
        embedded: snapshot.embedded ?? 0,
        categorized: snapshot.categorized ?? 0,
        failed: snapshot.failed ?? 0,
        etaSeconds: null,
        emailsPerSecond: snapshot.emailsPerSecond ?? 0,
      });
      setConnectionStatus('connected');
    };

    // Fallback: the backend SSE endpoint uses keep_alive, so EventSource.onerror
    // never fires when the pipeline completes before we connect. Instead, after
    // FAST_COMPLETE_TIMEOUT_MS with no events, query the REST endpoint directly.
    const fallbackTimer = setTimeout(async () => {
      if (settledRef.current) return;
      try {
        const snapshot = await getIngestionProgress();
        // active:false  → no job in memory
        // phase:complete → job finished but current_job not yet cleared in backend
        if (!snapshot.active || snapshot.phase === 'complete') {
          applyCompletion(snapshot);
        }
        // Otherwise pipeline is genuinely running; SSE will deliver events.
      } catch {
        // Ignore — connection issues will surface through the SSE error path.
      }
    }, FAST_COMPLETE_TIMEOUT_MS);

    const stream = createIngestionStream(jobId, async () => {
      // onerror fires if the SSE connection is rejected (e.g. auth failure).
      // The keep_alive case is handled by the timer above.
      clearTimeout(fallbackTimer);
      if (settledRef.current) return;
      try {
        const snapshot = await getIngestionProgress();
        if (!snapshot.active) {
          applyCompletion(snapshot);
          return;
        }
      } catch {
        // fall through
      }
      setConnectionStatus('error');
    });

    stream.subscribe((data) => {
      settledRef.current = true;
      clearTimeout(fallbackTimer);
      setConnectionStatus('connected');
      setProgress(data);

      const prev = prevProgress.current;
      if (prev) {
        if (data.phase !== prev.phase) {
          setDiscoveries((d) => [
            ...d,
            {
              id: `phase-${Date.now()}`,
              type: 'pattern' as const,
              message: `Phase transition: ${prev.phase} -> ${data.phase}`,
              timestamp: Date.now(),
            },
          ]);
        }
        if (data.categorized > prev.categorized) {
          const delta = data.categorized - prev.categorized;
          setDiscoveries((d) => [
            ...d,
            {
              id: `cat-${Date.now()}`,
              type: 'subscription' as const,
              message: `${delta} new email${delta > 1 ? 's' : ''} categorized (${data.categorized} total)`,
              timestamp: Date.now(),
            },
          ]);
        }
      }
      prevProgress.current = data;
    });

    return () => {
      clearTimeout(fallbackTimer);
      stream.close();
      setConnectionStatus('disconnected');
    };
  }, [jobId]);

  const pause = useCallback(async () => {
    await pauseIngestion(jobId);
    setIsPaused(true);
  }, [jobId]);

  const resume = useCallback(async () => {
    await resumeIngestion(jobId);
    setIsPaused(false);
  }, [jobId]);

  const addDiscovery = useCallback((discovery: Omit<Discovery, 'id' | 'timestamp'>) => {
    setDiscoveries((d) => [
      ...d,
      { ...discovery, id: `manual-${Date.now()}`, timestamp: Date.now() },
    ]);
  }, []);

  return {
    progress,
    discoveries,
    connectionStatus,
    isPaused,
    pause,
    resume,
    addDiscovery,
  };
}
