import { X, AlertTriangle, Loader2 } from 'lucide-react';
import type { UnsubscribePreview } from '@emailibrium/types';

interface UnsubscribePreviewDialogProps {
  isOpen: boolean;
  isLoading: boolean;
  previews: UnsubscribePreview[] | null;
  pendingCount: number;
  isUnsubscribing: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function UnsubscribePreviewDialog({
  isOpen,
  isLoading,
  previews,
  pendingCount,
  isUnsubscribing,
  onConfirm,
  onCancel,
}: UnsubscribePreviewDialogProps) {
  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      role="dialog"
      aria-modal="true"
      aria-label="Unsubscribe preview"
    >
      <div className="mx-4 w-full max-w-lg rounded-xl border border-gray-200 bg-white shadow-xl dark:border-gray-700 dark:bg-gray-800">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-gray-200 px-5 py-4 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <AlertTriangle className="h-5 w-5 text-amber-500" aria-hidden="true" />
            <h3 className="text-base font-semibold text-gray-900 dark:text-white">
              Confirm Unsubscribe
            </h3>
          </div>
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            aria-label="Close dialog"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Body */}
        <div className="max-h-80 overflow-y-auto px-5 py-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-6 w-6 animate-spin text-gray-400" />
              <span className="ml-2 text-sm text-gray-500">Loading preview...</span>
            </div>
          ) : previews && previews.length > 0 ? (
            <div className="space-y-3">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                You are about to unsubscribe from {previews.length} sender
                {previews.length === 1 ? '' : 's'}:
              </p>
              <div className="space-y-2">
                {previews.map((preview) => (
                  <div
                    key={preview.sender}
                    className="rounded-lg border border-gray-100 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-900/50"
                  >
                    <div className="flex items-center justify-between">
                      <div>
                        <p className="text-sm font-medium text-gray-900 dark:text-white">
                          {preview.sender}
                        </p>
                        <p className="text-xs text-gray-500 dark:text-gray-400">
                          via {preview.bestMethod?.type ?? 'unknown'}
                          {preview.methods.length > 1 &&
                            ` (+${preview.methods.length - 1} fallback${preview.methods.length > 2 ? 's' : ''})`}
                        </p>
                      </div>
                    </div>
                    {preview.warning && (
                      <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                        {preview.warning}
                      </p>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <p className="text-sm text-gray-600 dark:text-gray-400">
              You are about to unsubscribe from {pendingCount} subscription
              {pendingCount === 1 ? '' : 's'}. This action can be undone within 5 minutes.
            </p>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 border-t border-gray-200 px-5 py-4 dark:border-gray-700">
          <button
            type="button"
            onClick={onCancel}
            disabled={isUnsubscribing}
            className="rounded-md px-4 py-2 text-sm font-medium text-gray-600 transition-colors hover:text-gray-800 dark:text-gray-400 dark:hover:text-gray-200"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={isUnsubscribing || isLoading}
            className="flex items-center gap-2 rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {isUnsubscribing ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                Unsubscribing...
              </>
            ) : (
              'Unsubscribe'
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
