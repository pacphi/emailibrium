import { forwardRef, useId, type SelectHTMLAttributes } from 'react';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SelectProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'children'> {
  label?: string;
  options: SelectOption[];
  placeholder?: string;
  error?: string;
  className?: string;
}

/**
 * Select dropdown with label, placeholder, and error state.
 * Forwards ref for form library integration.
 */
export const Select = forwardRef<HTMLSelectElement, SelectProps>(function Select(
  { label, options, placeholder, error, className = '', id: providedId, ...rest },
  ref,
) {
  const generatedId = useId();
  const selectId = providedId ?? generatedId;
  const errorId = error ? `${selectId}-error` : undefined;

  return (
    <div className={className}>
      {label && (
        <label
          htmlFor={selectId}
          className="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          {label}
        </label>
      )}
      <select
        ref={ref}
        id={selectId}
        aria-invalid={error ? true : undefined}
        aria-describedby={errorId}
        className={[
          'block w-full rounded-md border px-3 py-2 text-sm outline-none transition-colors',
          'bg-white dark:bg-gray-800',
          'text-gray-900 dark:text-gray-100',
          error
            ? 'border-red-300 focus:border-red-500 focus:ring-1 focus:ring-red-500'
            : 'border-gray-300 focus:border-indigo-400 focus:ring-1 focus:ring-indigo-400 dark:border-gray-600 dark:focus:border-indigo-500',
          'disabled:cursor-not-allowed disabled:opacity-50',
        ].join(' ')}
        {...rest}
      >
        {placeholder && (
          <option value="" disabled>
            {placeholder}
          </option>
        )}
        {options.map((opt) => (
          <option key={opt.value} value={opt.value} disabled={opt.disabled}>
            {opt.label}
          </option>
        ))}
      </select>
      {error && (
        <p id={errorId} className="mt-1 text-xs text-red-600 dark:text-red-400" role="alert">
          {error}
        </p>
      )}
    </div>
  );
});
