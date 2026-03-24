import { useState } from 'react';
import { Pencil, Trash2, ArrowUpDown, Loader2 } from 'lucide-react';
import type { Rule } from '@emailibrium/types';
import { useToggleRule, useDeleteRule } from './hooks/useRules';

interface ActiveRulesListProps {
  rules: Rule[];
  isLoading: boolean;
  isError: boolean;
  onEdit: (rule: Rule) => void;
}

type SortField = 'name' | 'matchCount' | 'accuracy';
type SortDir = 'asc' | 'desc';

function sortRules(rules: Rule[], field: SortField, dir: SortDir): Rule[] {
  const sorted = [...rules].sort((a, b) => {
    if (field === 'name') return a.name.localeCompare(b.name);
    return a[field] - b[field];
  });
  return dir === 'desc' ? sorted.reverse() : sorted;
}

export function ActiveRulesList({ rules, isLoading, isError, onEdit }: ActiveRulesListProps) {
  const [sortField, setSortField] = useState<SortField>('matchCount');
  const [sortDir, setSortDir] = useState<SortDir>('desc');
  const toggleMutation = useToggleRule();
  const deleteMutation = useDeleteRule();

  function handleSort(field: SortField) {
    if (sortField === field) {
      setSortDir((prev) => (prev === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortField(field);
      setSortDir('desc');
    }
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-5 w-5 animate-spin text-indigo-500" />
        <span className="ml-2 text-sm text-gray-500">Loading rules...</span>
      </div>
    );
  }

  if (isError) {
    return <p className="py-8 text-center text-sm text-red-500">Failed to load rules.</p>;
  }

  if (rules.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-gray-500 dark:text-gray-400">
        No rules yet. Create one to get started.
      </p>
    );
  }

  const sorted = sortRules(rules, sortField, sortDir);

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-sm" aria-label="Active rules">
        <thead>
          <tr className="border-b border-gray-200 dark:border-gray-700">
            <SortableHeader
              label="Rule Name"
              field="name"
              currentField={sortField}
              currentDir={sortDir}
              onSort={handleSort}
            />
            <SortableHeader
              label="Matches"
              field="matchCount"
              currentField={sortField}
              currentDir={sortDir}
              onSort={handleSort}
            />
            <SortableHeader
              label="Accuracy"
              field="accuracy"
              currentField={sortField}
              currentDir={sortDir}
              onSort={handleSort}
            />
            <th className="px-3 py-2 font-medium text-gray-500 dark:text-gray-400">Status</th>
            <th className="px-3 py-2 font-medium text-gray-500 dark:text-gray-400">Actions</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((rule) => (
            <tr
              key={rule.id}
              className="border-b border-gray-100 transition-colors hover:bg-gray-50 dark:border-gray-700/50 dark:hover:bg-gray-800/50"
            >
              <td className="px-3 py-2.5 font-medium text-gray-900 dark:text-white">{rule.name}</td>
              <td className="px-3 py-2.5 text-gray-600 dark:text-gray-300">
                {rule.matchCount.toLocaleString()}
              </td>
              <td className="px-3 py-2.5">
                <span
                  className={`font-medium ${
                    rule.accuracy >= 0.9
                      ? 'text-green-600 dark:text-green-400'
                      : rule.accuracy >= 0.7
                        ? 'text-yellow-600 dark:text-yellow-400'
                        : 'text-red-600 dark:text-red-400'
                  }`}
                >
                  {(rule.accuracy * 100).toFixed(1)}%
                </span>
              </td>
              <td className="px-3 py-2.5">
                <button
                  type="button"
                  role="switch"
                  aria-checked={rule.isActive}
                  aria-label={`${rule.isActive ? 'Disable' : 'Enable'} ${rule.name}`}
                  onClick={() =>
                    toggleMutation.mutate({
                      id: rule.id,
                      isActive: !rule.isActive,
                    })
                  }
                  className={`
                    relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full transition-colors
                    ${rule.isActive ? 'bg-indigo-600' : 'bg-gray-300 dark:bg-gray-600'}
                  `}
                >
                  <span
                    className={`
                      inline-block h-4 w-4 rounded-full bg-white shadow transition-transform
                      ${rule.isActive ? 'translate-x-4' : 'translate-x-0.5'}
                      mt-0.5
                    `}
                  />
                </button>
              </td>
              <td className="px-3 py-2.5">
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => onEdit(rule)}
                    className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
                    aria-label={`Edit ${rule.name}`}
                  >
                    <Pencil className="h-4 w-4" />
                  </button>
                  <button
                    type="button"
                    onClick={() => deleteMutation.mutate(rule.id)}
                    className="rounded p-1 text-gray-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/30 dark:hover:text-red-400"
                    aria-label={`Delete ${rule.name}`}
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SortableHeader({
  label,
  field,
  currentField,
  currentDir,
  onSort,
}: {
  label: string;
  field: SortField;
  currentField: SortField;
  currentDir: SortDir;
  onSort: (field: SortField) => void;
}) {
  const isActive = currentField === field;
  return (
    <th className="px-3 py-2">
      <button
        type="button"
        onClick={() => onSort(field)}
        className="flex items-center gap-1 font-medium text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
        aria-sort={isActive ? (currentDir === 'asc' ? 'ascending' : 'descending') : 'none'}
      >
        {label}
        <ArrowUpDown
          className={`h-3 w-3 ${isActive ? 'text-indigo-500' : 'text-gray-300 dark:text-gray-600'}`}
          aria-hidden="true"
        />
      </button>
    </th>
  );
}
