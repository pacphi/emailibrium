import React from 'react';
import { ErrorFallback, type ErrorFallbackVariant } from './ErrorFallback';

interface ErrorBoundaryProps {
  children: React.ReactNode;
  /** Optional custom fallback component. If omitted, the default ErrorFallback is rendered. */
  fallback?: React.ReactNode;
  /** Variant passed to the default ErrorFallback. Defaults to 'section'. */
  variant?: ErrorFallbackVariant;
  /** Callback invoked when an error is caught, useful for logging. */
  onError?: (error: Error, errorInfo: React.ErrorInfo) => void;
  /** When this key changes, the error boundary resets. Useful for resetting on navigation. */
  resetKey?: string;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

/**
 * React error boundary that catches render errors in its subtree
 * and displays a fallback UI with an option to retry.
 *
 * Automatically resets when the `resetKey` prop changes, which makes
 * it convenient to wire up to route changes.
 */
export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo): void {
    this.props.onError?.(error, errorInfo);
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps): void {
    if (this.state.hasError && prevProps.resetKey !== this.props.resetKey) {
      this.reset();
    }
  }

  reset = (): void => {
    this.setState({ hasError: false, error: null });
  };

  render(): React.ReactNode {
    if (!this.state.hasError) {
      return this.props.children;
    }

    if (this.props.fallback !== undefined) {
      return this.props.fallback;
    }

    return (
      <ErrorFallback
        variant={this.props.variant}
        message={this.state.error?.message}
        onRetry={this.reset}
      />
    );
  }
}
