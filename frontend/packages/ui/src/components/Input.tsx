import { forwardRef, useId, type InputHTMLAttributes } from 'react';

export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> {
  label?: string;
  error?: string;
  helperText?: string;
  className?: string;
}

/**
 * Text input with label, error state, and helper text.
 * Forwards ref for form library integration.
 */
export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { label, error, helperText, className = '', id: providedId, ...rest },
  ref,
) {
  const generatedId = useId();
  const inputId = providedId ?? generatedId;
  const errorId = error ? `${inputId}-error` : undefined;
  const helperId = helperText && !error ? `${inputId}-helper` : undefined;

  return (
    <div className={className}>
      {label && (
        <label
          htmlFor={inputId}
          className="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          {label}
        </label>
      )}
      <input
        ref={ref}
        id={inputId}
        aria-invalid={error ? true : undefined}
        aria-describedby={errorId ?? helperId}
        className={[
          'block w-full rounded-md border px-3 py-2 text-sm outline-none transition-colors',
          'bg-white dark:bg-gray-800',
          'text-gray-900 placeholder-gray-400 dark:text-gray-100 dark:placeholder-gray-500',
          error
            ? 'border-red-300 focus:border-red-500 focus:ring-1 focus:ring-red-500'
            : 'border-gray-300 focus:border-indigo-400 focus:ring-1 focus:ring-indigo-400 dark:border-gray-600 dark:focus:border-indigo-500',
          'disabled:cursor-not-allowed disabled:opacity-50',
        ].join(' ')}
        {...rest}
      />
      {error && (
        <p id={errorId} className="mt-1 text-xs text-red-600 dark:text-red-400" role="alert">
          {error}
        </p>
      )}
      {helperText && !error && (
        <p id={helperId} className="mt-1 text-xs text-gray-500 dark:text-gray-400">
          {helperText}
        </p>
      )}
    </div>
  );
});
