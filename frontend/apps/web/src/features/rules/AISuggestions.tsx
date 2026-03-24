import { Check, Pencil, X, Loader2, Sparkles } from 'lucide-react';
import type { RuleSuggestion } from '@emailibrium/types';
import { useRuleSuggestionsQuery, useCreateRule } from './hooks/useRules';

interface AISuggestionsProps {
  onCustomize: (suggestion: RuleSuggestion) => void;
}

export function AISuggestions({ onCustomize }: AISuggestionsProps) {
  const { data: suggestions, isLoading, isError } = useRuleSuggestionsQuery();
  const createMutation = useCreateRule();

  function handleAccept(suggestion: RuleSuggestion) {
    createMutation.mutate({
      name: suggestion.rule.name,
      conditions: suggestion.rule.conditions,
      actions: suggestion.rule.actions,
      isActive: true,
    });
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-5 w-5 animate-spin text-indigo-500" />
        <span className="ml-2 text-sm text-gray-500">Analyzing your inbox...</span>
      </div>
    );
  }

  if (isError) {
    return <p className="py-8 text-center text-sm text-red-500">Failed to load suggestions.</p>;
  }

  if (!suggestions || suggestions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-gray-500">
        <Sparkles className="mb-2 h-8 w-8" />
        <p className="text-sm">No suggestions right now.</p>
        <p className="text-xs">Check back after processing more emails.</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {suggestions.map((suggestion, index) => (
        <div
          key={index}
          className="rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800"
        >
          <div className="mb-2 flex items-start justify-between">
            <div>
              <h4 className="text-sm font-semibold text-gray-900 dark:text-white">
                {suggestion.rule.name}
              </h4>
              <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">{suggestion.reason}</p>
            </div>
            <span className="shrink-0 rounded-full bg-indigo-100 px-2 py-0.5 text-xs font-medium text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300">
              ~{suggestion.estimatedMatches} matches
            </span>
          </div>

          {/* Conditions summary */}
          <div className="mb-3 text-xs text-gray-500 dark:text-gray-400">
            <span className="font-medium">When: </span>
            {suggestion.rule.conditions
              .map((c) => `${c.field} ${c.operator} "${c.value}"`)
              .join(' AND ')}
          </div>
          <div className="mb-3 text-xs text-gray-500 dark:text-gray-400">
            <span className="font-medium">Then: </span>
            {suggestion.rule.actions
              .map((a) => `${a.type}${a.value ? ` "${a.value}"` : ''}`)
              .join(', ')}
          </div>

          {/* Actions */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => handleAccept(suggestion)}
              disabled={createMutation.isPending}
              className="flex items-center gap-1 rounded-md bg-green-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-green-700 disabled:opacity-50"
            >
              <Check className="h-3.5 w-3.5" aria-hidden="true" />
              Accept
            </button>
            <button
              type="button"
              onClick={() => onCustomize(suggestion)}
              className="flex items-center gap-1 rounded-md border border-gray-200 px-3 py-1.5 text-xs font-medium text-gray-600 transition-colors hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
            >
              <Pencil className="h-3.5 w-3.5" aria-hidden="true" />
              Customize
            </button>
            <button
              type="button"
              className="flex items-center gap-1 rounded-md px-3 py-1.5 text-xs text-gray-400 transition-colors hover:text-gray-600 dark:hover:text-gray-300"
            >
              <X className="h-3.5 w-3.5" aria-hidden="true" />
              Dismiss
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
