import { useState, useEffect, useCallback, useRef } from 'react';
import type { IngestionProgress } from '@emailibrium/types';
import { createIngestionStream, pauseIngestion, resumeIngestion } from '@emailibrium/api';

export interface Discovery {
  id: string;
  type: 'subscription' | 'cluster' | 'pattern';
  message: string;
  timestamp: number;
}

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

export function useIngestionProgress(jobId: string) {
  const [progress, setProgress] = useState<IngestionProgress | null>(null);
  const [discoveries, setDiscoveries] = useState<Discovery[]>([]);
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>('connecting');
  const [isPaused, setIsPaused] = useState(false);
  const prevProgress = useRef<IngestionProgress | null>(null);

  useEffect(() => {
    if (!jobId) return;

    setConnectionStatus('connecting');
    const stream = createIngestionStream(jobId);

    stream.subscribe((data) => {
      setConnectionStatus('connected');
      setProgress(data);

      // Generate discovery events from progress changes
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
