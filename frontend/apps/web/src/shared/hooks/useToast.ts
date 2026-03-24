import { useCallback } from 'react';
import { type ToastType, useToastStore } from '@/shared/stores/toastStore';

/**
 * Provides a simple API for showing toast notifications.
 * Backed by a Zustand store so toasts are globally accessible.
 */
export function useToast() {
  const addToast = useToastStore((state) => state.addToast);
  const removeToast = useToastStore((state) => state.removeToast);
  const clearAll = useToastStore((state) => state.clearAll);

  const toast = useCallback(
    (message: string, type: ToastType = 'info', duration?: number) => {
      addToast(message, type, duration);
    },
    [addToast],
  );

  return { toast, removeToast, clearAll };
}
