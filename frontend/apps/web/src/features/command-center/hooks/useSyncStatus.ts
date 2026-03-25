import { useSyncExternalStore, useCallback } from 'react';

export interface PendingOperation {
  id: string;
  type: string;
  payload: unknown;
  createdAt: number;
  retryCount: number;
}

const STORAGE_KEY = 'emailibrium-offline-queue';

// Cache the parsed result so useSyncExternalStore gets a stable reference.
// A new array on every call triggers infinite re-renders because [] !== [].
let cachedRaw: string | null = null;
let cachedResult: PendingOperation[] = [];
const EMPTY: PendingOperation[] = [];

function getSnapshot(): PendingOperation[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === cachedRaw) return cachedResult;
    cachedRaw = raw;
    cachedResult = raw ? (JSON.parse(raw) as PendingOperation[]) : EMPTY;
    return cachedResult;
  } catch {
    return EMPTY;
  }
}

function getServerSnapshot(): PendingOperation[] {
  return EMPTY;
}

let listeners: Array<() => void> = [];

function emitChange() {
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners = [...listeners, listener];

  // Listen for storage changes from other tabs
  const handleStorage = (e: StorageEvent) => {
    if (e.key === STORAGE_KEY) emitChange();
  };
  window.addEventListener('storage', handleStorage);

  return () => {
    listeners = listeners.filter((l) => l !== listener);
    window.removeEventListener('storage', handleStorage);
  };
}

/**
 * Adds an operation to the offline queue.
 */
export function enqueueOperation(type: string, payload: unknown): void {
  const ops = getSnapshot();
  const op: PendingOperation = {
    id: `op_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
    type,
    payload,
    createdAt: Date.now(),
    retryCount: 0,
  };
  localStorage.setItem(STORAGE_KEY, JSON.stringify([...ops, op]));
  emitChange();
}

/**
 * Removes a completed operation from the queue.
 */
export function dequeueOperation(id: string): void {
  const ops = getSnapshot().filter((op) => op.id !== id);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(ops));
  emitChange();
}

/**
 * Clears all pending operations.
 */
export function clearQueue(): void {
  localStorage.removeItem(STORAGE_KEY);
  emitChange();
}

/**
 * Hook that tracks the offline operation queue using useSyncExternalStore
 * for cross-tab reactivity.
 */
export function useSyncStatus() {
  const operations = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  const isOnline = typeof navigator !== 'undefined' ? navigator.onLine : true;

  const flush = useCallback(() => {
    // In production, iterate pending operations and replay them via API
    // For now, just clear the queue
    clearQueue();
  }, []);

  return {
    pendingCount: operations.length,
    operations,
    isOnline,
    flush,
  };
}
