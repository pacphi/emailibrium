import { useCallback } from 'react';
import { useToast } from './useToast';

interface KyHTTPError {
  response?: {
    status: number;
    headers: { get: (name: string) => string | null };
  };
  message?: string;
}

function isKyHTTPError(error: unknown): error is KyHTTPError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'response' in error &&
    typeof (error as KyHTTPError).response === 'object'
  );
}

/**
 * Provides handlers for common error scenarios: API errors, network
 * failures, and rate limiting. Each handler displays an appropriate
 * toast notification to the user.
 */
export function useErrorHandler() {
  const { toast } = useToast();

  const handleApiError = useCallback(
    (error: unknown) => {
      if (isKyHTTPError(error)) {
        const status = error.response?.status;

        switch (status) {
          case 400:
            toast('Invalid request. Please check your input.', 'error');
            break;
          case 401:
            toast('Your session has expired. Please sign in again.', 'error');
            break;
          case 403:
            toast('You do not have permission to perform this action.', 'error');
            break;
          case 404:
            toast('The requested resource was not found.', 'error');
            break;
          case 429: {
            const retryAfter = error.response?.headers.get('Retry-After');
            const seconds = retryAfter ? parseInt(retryAfter, 10) : 60;
            toast(
              `Too many requests. Please wait ${seconds} seconds.`,
              'warning',
              (seconds + 1) * 1000,
            );
            break;
          }
          case 500:
          case 502:
          case 503:
            toast('Server error. Please try again in a moment.', 'error');
            break;
          default:
            toast(error.message ?? 'An unexpected error occurred.', 'error');
        }
        return;
      }

      if (error instanceof Error) {
        toast(error.message, 'error');
        return;
      }

      toast('An unexpected error occurred.', 'error');
    },
    [toast],
  );

  const handleNetworkError = useCallback(() => {
    toast('You appear to be offline. Check your connection and try again.', 'warning', 8000);
  }, [toast]);

  const handleRateLimit = useCallback(
    (retryAfter: number) => {
      const seconds = Math.ceil(retryAfter);
      toast(
        `Rate limited. Retry available in ${seconds} seconds.`,
        'warning',
        (seconds + 1) * 1000,
      );
    },
    [toast],
  );

  return {
    handleApiError,
    handleNetworkError,
    handleRateLimit,
  };
}
