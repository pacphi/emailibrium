import { useState, useEffect } from 'react';
import { X, Undo2, Loader2 } from 'lucide-react';

interface UndoToastProps {
  count: number;
  deadline: number;
  isUndoing: boolean;
  onUndo: () => void;
  onDismiss: () => void;
}

export function UndoToast({ count, deadline, isUndoing, onUndo, onDismiss }: UndoToastProps) {
  const [remainingSeconds, setRemainingSeconds] = useState(
    Math.max(0, Math.ceil((deadline - Date.now()) / 1000)),
  );

  useEffect(() => {
    const interval = setInterval(() => {
      const remaining = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
      setRemainingSeconds(remaining);
      if (remaining <= 0) {
        clearInterval(interval);
        onDismiss();
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [deadline, onDismiss]);

  const minutes = Math.floor(remainingSeconds / 60);
  const seconds = remainingSeconds % 60;
  const timeLabel = minutes > 0 ? `${minutes}m ${seconds}s` : `${seconds}s`;

  return (
    <div className="fixed bottom-6 right-6 z-50 flex items-center gap-3 rounded-lg border border-gray-200 bg-white px-4 py-3 shadow-lg dark:border-gray-700 dark:bg-gray-800">
      <div className="text-sm text-gray-700 dark:text-gray-300">
        Unsubscribed from {count} sender{count === 1 ? '' : 's'}.
        <span className="ml-1 text-xs text-gray-500 dark:text-gray-400">
          Undo available for {timeLabel}
        </span>
      </div>
      <button
        type="button"
        onClick={onUndo}
        disabled={isUndoing}
        className="flex items-center gap-1 rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
      >
        {isUndoing ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
        ) : (
          <Undo2 className="h-3.5 w-3.5" aria-hidden="true" />
        )}
        Undo
      </button>
      <button
        type="button"
        onClick={onDismiss}
        className="rounded p-0.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
        aria-label="Dismiss"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
