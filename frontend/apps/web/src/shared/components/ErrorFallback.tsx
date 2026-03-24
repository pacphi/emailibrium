import { AlertCircle } from 'lucide-react';

export type ErrorFallbackVariant = 'page' | 'section' | 'toast';

interface ErrorFallbackProps {
  /** The error message to display. */
  message?: string;
  /** Optional title override. */
  title?: string;
  /** Visual variant controlling the layout. Defaults to 'section'. */
  variant?: ErrorFallbackVariant;
  /** Callback when the user clicks the retry button. If omitted, the retry button is hidden. */
  onRetry?: () => void;
  /** Whether to show a "Go Home" link. Defaults to true for 'page' variant. */
  showHomeLink?: boolean;
}

const VARIANT_CLASSES: Record<ErrorFallbackVariant, string> = {
  page: 'flex min-h-[60vh] flex-col items-center justify-center px-4 text-center',
  section:
    'flex flex-col items-center justify-center gap-3 rounded-lg border border-red-200 bg-red-50 p-8 text-center',
  toast: 'flex items-center gap-3 rounded-lg border border-red-200 bg-red-50 px-4 py-3',
};

/**
 * Generic error display component with multiple layout variants.
 * Provides a retry button and optional navigation back to the home page.
 */
export function ErrorFallback({
  message = 'An unexpected error occurred. Please try again.',
  title = 'Something went wrong',
  variant = 'section',
  onRetry,
  showHomeLink,
}: ErrorFallbackProps) {
  const shouldShowHomeLink = showHomeLink ?? variant === 'page';

  if (variant === 'toast') {
    return (
      <div role="alert" className={VARIANT_CLASSES.toast}>
        <AlertCircle className="h-5 w-5 flex-shrink-0 text-red-500" />
        <p className="flex-1 text-sm text-red-700">{message}</p>
        {onRetry && (
          <button
            type="button"
            onClick={onRetry}
            className="text-sm font-medium text-red-600 underline hover:text-red-800"
          >
            Retry
          </button>
        )}
      </div>
    );
  }

  return (
    <div role="alert" className={VARIANT_CLASSES[variant]}>
      <AlertCircle className="h-12 w-12 text-red-400" />
      <h2 className="mt-4 text-lg font-semibold text-gray-900">{title}</h2>
      <p className="mt-2 max-w-md text-sm text-gray-600">{message}</p>
      <div className="mt-6 flex gap-3">
        {onRetry && (
          <button
            type="button"
            onClick={onRetry}
            className="rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-700"
          >
            Try Again
          </button>
        )}
        {shouldShowHomeLink && (
          <a
            href="/"
            className="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 transition-colors hover:bg-gray-50"
          >
            Go Home
          </a>
        )}
      </div>
    </div>
  );
}
