import { useState, useCallback, useRef } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  batchUnsubscribe,
  undoUnsubscribe,
  previewUnsubscribe,
} from '@emailibrium/api';
import type { UnsubscribeResult, UnsubscribePreview } from '@emailibrium/types';

const UNDO_WINDOW_MS = 5 * 60 * 1000; // 5 minutes

interface UndoState {
  batchId: string;
  count: number;
  deadline: number;
}

export function useUnsubscribePreview() {
  return useMutation({
    mutationFn: (subscriptionIds: string[]) => previewUnsubscribe(subscriptionIds),
  });
}

export function useBatchUnsubscribe() {
  const queryClient = useQueryClient();
  return useMutation<UnsubscribeResult, Error, string[]>({
    mutationFn: (subscriptionIds: string[]) =>
      batchUnsubscribe({ subscriptionIds }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['subscriptions'] });
    },
  });
}

export function useUndoUnsubscribe() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: undoUnsubscribe,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['subscriptions'] });
    },
  });
}

/**
 * Manages the full unsubscribe flow: preview, execute, and undo with toast.
 */
export function useUnsubscribeFlow() {
  const [previewData, setPreviewData] = useState<UnsubscribePreview[] | null>(null);
  const [isPreviewOpen, setIsPreviewOpen] = useState(false);
  const [undoState, setUndoState] = useState<UndoState | null>(null);
  const [pendingIds, setPendingIds] = useState<string[]>([]);
  const undoTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const previewMutation = useUnsubscribePreview();
  const unsubscribeMutation = useBatchUnsubscribe();
  const undoMutation = useUndoUnsubscribe();

  const openPreview = useCallback(
    (subscriptionIds: string[]) => {
      setPendingIds(subscriptionIds);
      previewMutation.mutate(subscriptionIds, {
        onSuccess: (data) => {
          setPreviewData(data);
          setIsPreviewOpen(true);
        },
        onError: () => {
          // If preview fails, proceed directly to confirm dialog
          setPreviewData(null);
          setIsPreviewOpen(true);
        },
      });
    },
    [previewMutation],
  );

  const closePreview = useCallback(() => {
    setIsPreviewOpen(false);
    setPreviewData(null);
    setPendingIds([]);
  }, []);

  const confirmUnsubscribe = useCallback(() => {
    unsubscribeMutation.mutate(pendingIds, {
      onSuccess: (result) => {
        setIsPreviewOpen(false);
        setPreviewData(null);
        setPendingIds([]);

        // Set up undo window
        const deadline = Date.now() + UNDO_WINDOW_MS;
        setUndoState({
          batchId: result.batchId,
          count: result.succeeded.length,
          deadline,
        });

        // Auto-dismiss undo toast after deadline
        if (undoTimerRef.current) clearTimeout(undoTimerRef.current);
        undoTimerRef.current = setTimeout(() => {
          setUndoState(null);
        }, UNDO_WINDOW_MS);
      },
    });
  }, [pendingIds, unsubscribeMutation]);

  const handleUndo = useCallback(() => {
    if (!undoState) return;
    undoMutation.mutate(undoState.batchId, {
      onSuccess: () => {
        setUndoState(null);
        if (undoTimerRef.current) clearTimeout(undoTimerRef.current);
      },
    });
  }, [undoState, undoMutation]);

  const dismissUndo = useCallback(() => {
    setUndoState(null);
    if (undoTimerRef.current) clearTimeout(undoTimerRef.current);
  }, []);

  return {
    // Preview dialog state
    isPreviewOpen,
    isPreviewLoading: previewMutation.isPending,
    previewData,
    pendingIds,
    openPreview,
    closePreview,

    // Execute
    confirmUnsubscribe,
    isUnsubscribing: unsubscribeMutation.isPending,

    // Undo toast state
    undoState,
    isUndoing: undoMutation.isPending,
    handleUndo,
    dismissUndo,
  };
}
