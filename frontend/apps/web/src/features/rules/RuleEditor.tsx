import { useState } from 'react';
import { Plus, X, FlaskConical, Save } from 'lucide-react';
import type { Rule, RuleCondition, RuleAction } from '@emailibrium/types';
import { SemanticCondition } from './SemanticCondition';
import { useCreateRule, useUpdateRule, useValidateRule, useTestRule } from './hooks/useRules';

interface RuleEditorProps {
  rule?: Rule;
  onClose: () => void;
}

const conditionFields = [
  'from',
  'to',
  'subject',
  'body',
  'category',
  'labels',
  'has-attachment',
  'similar-to',
  'about-topic',
];

const conditionOperators = [
  'contains',
  'equals',
  'starts-with',
  'ends-with',
  'matches-regex',
  'not-contains',
];

const actionTypes = [
  'move-to',
  'add-label',
  'remove-label',
  'archive',
  'star',
  'mark-read',
  'delete',
  'forward-to',
];

function emptyCondition(): RuleCondition {
  return { field: 'from', operator: 'contains', value: '' };
}

function emptyAction(): RuleAction {
  return { type: 'add-label', value: '' };
}

export function RuleEditor({ rule, onClose }: RuleEditorProps) {
  const isEditing = !!rule;
  const [name, setName] = useState(rule?.name ?? '');
  const [conditions, setConditions] = useState<RuleCondition[]>(
    rule?.conditions.length ? rule.conditions : [emptyCondition()],
  );
  const [actions, setActions] = useState<RuleAction[]>(
    rule?.actions.length ? rule.actions : [emptyAction()],
  );
  const [semanticThreshold, setSemanticThreshold] = useState(0.75);
  const [testResult, setTestResult] = useState<string | null>(null);

  const createMutation = useCreateRule();
  const updateMutation = useUpdateRule();
  const validateMutation = useValidateRule();
  const testMutation = useTestRule();
  const isSaving = createMutation.isPending || updateMutation.isPending;
  const [validationErrors, setValidationErrors] = useState<
    Array<{ field: string; message: string }>
  >([]);

  function updateCondition(index: number, patch: Partial<RuleCondition>) {
    setConditions((prev) => prev.map((c, i) => (i === index ? { ...c, ...patch } : c)));
  }

  function removeCondition(index: number) {
    setConditions((prev) => prev.filter((_, i) => i !== index));
  }

  function updateAction(index: number, patch: Partial<RuleAction>) {
    setActions((prev) => prev.map((a, i) => (i === index ? { ...a, ...patch } : a)));
  }

  function removeAction(index: number) {
    setActions((prev) => prev.filter((_, i) => i !== index));
  }

  function handleTest() {
    const payload = {
      name: name.trim(),
      conditions,
      actions,
      isActive: rule?.isActive ?? true,
    };
    setTestResult(null);
    testMutation.mutate(payload, {
      onSuccess: (result) => {
        const matchText =
          result.matchCount === 0
            ? 'This rule would not match any emails in your inbox.'
            : `This rule would match ${result.matchCount} email${result.matchCount === 1 ? '' : 's'} in your inbox.`;
        const sampleText =
          result.sampleMatches.length > 0
            ? ` Example: "${result.sampleMatches[0]!.subject}" from ${result.sampleMatches[0]!.from}`
            : '';
        setTestResult(matchText + sampleText);
      },
      onError: () => {
        setTestResult('Failed to test rule. Please try again.');
      },
    });
  }

  async function handleValidateAndSave() {
    const payload = {
      name: name.trim(),
      conditions,
      actions,
      isActive: rule?.isActive ?? true,
    };

    validateMutation.mutate(payload, {
      onSuccess: (result) => {
        setValidationErrors(result.errors);
        if (result.valid) {
          if (isEditing && rule) {
            updateMutation.mutate({ id: rule.id, rule: payload }, { onSuccess: () => onClose() });
          } else {
            createMutation.mutate(payload, { onSuccess: () => onClose() });
          }
        }
      },
      onError: () => {
        // Fall back to saving without validation if the endpoint is unavailable
        handleSave();
      },
    });
  }

  function handleSave() {
    const payload = {
      name: name.trim(),
      conditions,
      actions,
      isActive: rule?.isActive ?? true,
    };
    if (isEditing && rule) {
      updateMutation.mutate({ id: rule.id, rule: payload }, { onSuccess: () => onClose() });
    } else {
      createMutation.mutate(payload, { onSuccess: () => onClose() });
    }
  }

  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800">
      <h3 className="mb-4 text-base font-semibold text-gray-900 dark:text-white">
        {isEditing ? 'Edit Rule' : 'New Rule'}
      </h3>

      {/* Name */}
      <div className="mb-4">
        <label
          htmlFor="rule-name"
          className="mb-1 block text-sm font-medium text-gray-700 dark:text-gray-300"
        >
          Rule Name
        </label>
        <input
          id="rule-name"
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. Archive marketing emails"
          className="w-full rounded-md border border-gray-200 bg-transparent px-3 py-2 text-sm text-gray-900 outline-none focus:border-indigo-400 focus:ring-1 focus:ring-indigo-400 dark:border-gray-600 dark:text-white"
        />
      </div>

      {/* Conditions */}
      <div className="mb-4">
        <div className="mb-2 flex items-center justify-between">
          <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Conditions</h4>
          <button
            type="button"
            onClick={() => setConditions((prev) => [...prev, emptyCondition()])}
            className="flex items-center gap-1 text-xs text-indigo-600 hover:text-indigo-700 dark:text-indigo-400"
          >
            <Plus className="h-3 w-3" /> Add condition
          </button>
        </div>
        <div className="space-y-2">
          {conditions.map((cond, i) => {
            if (cond.field === 'similar-to' || cond.field === 'about-topic') {
              return (
                <div key={i} className="relative">
                  <SemanticCondition
                    type={cond.field as 'similar-to' | 'about-topic'}
                    value={cond.value}
                    threshold={semanticThreshold}
                    onChangeValue={(v) => updateCondition(i, { value: v })}
                    onChangeThreshold={setSemanticThreshold}
                  />
                  <button
                    type="button"
                    onClick={() => removeCondition(i)}
                    className="absolute right-2 top-2 rounded p-0.5 text-gray-400 hover:text-red-500"
                    aria-label="Remove condition"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>
              );
            }

            return (
              <div key={i} className="flex items-center gap-2">
                <select
                  value={cond.field}
                  onChange={(e) => updateCondition(i, { field: e.target.value })}
                  className="rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                  aria-label="Condition field"
                >
                  {conditionFields.map((f) => (
                    <option key={f} value={f}>
                      {f}
                    </option>
                  ))}
                </select>
                <select
                  value={cond.operator}
                  onChange={(e) => updateCondition(i, { operator: e.target.value })}
                  className="rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                  aria-label="Condition operator"
                >
                  {conditionOperators.map((op) => (
                    <option key={op} value={op}>
                      {op}
                    </option>
                  ))}
                </select>
                <input
                  type="text"
                  value={cond.value}
                  onChange={(e) => updateCondition(i, { value: e.target.value })}
                  placeholder="Value"
                  className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                  aria-label="Condition value"
                />
                <button
                  type="button"
                  onClick={() => removeCondition(i)}
                  className="rounded p-1 text-gray-400 hover:text-red-500"
                  aria-label="Remove condition"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
            );
          })}
        </div>
      </div>

      {/* Actions */}
      <div className="mb-4">
        <div className="mb-2 flex items-center justify-between">
          <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">Actions</h4>
          <button
            type="button"
            onClick={() => setActions((prev) => [...prev, emptyAction()])}
            className="flex items-center gap-1 text-xs text-indigo-600 hover:text-indigo-700 dark:text-indigo-400"
          >
            <Plus className="h-3 w-3" /> Add action
          </button>
        </div>
        <div className="space-y-2">
          {actions.map((action, i) => (
            <div key={i} className="flex items-center gap-2">
              <select
                value={action.type}
                onChange={(e) => updateAction(i, { type: e.target.value })}
                className="rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                aria-label="Action type"
              >
                {actionTypes.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
              {action.type !== 'archive' &&
                action.type !== 'star' &&
                action.type !== 'mark-read' &&
                action.type !== 'delete' && (
                  <input
                    type="text"
                    value={action.value ?? ''}
                    onChange={(e) => updateAction(i, { value: e.target.value })}
                    placeholder="Value"
                    className="flex-1 rounded-md border border-gray-200 bg-transparent px-2 py-1.5 text-sm text-gray-900 outline-none focus:border-indigo-400 dark:border-gray-600 dark:text-white"
                    aria-label="Action value"
                  />
                )}
              <button
                type="button"
                onClick={() => removeAction(i)}
                className="rounded p-1 text-gray-400 hover:text-red-500"
                aria-label="Remove action"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
          ))}
        </div>
      </div>

      {/* Validation errors */}
      {validationErrors.length > 0 && (
        <div className="mb-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 dark:border-red-800 dark:bg-red-900/20">
          <p className="mb-1 text-sm font-medium text-red-700 dark:text-red-400">
            Validation errors:
          </p>
          <ul className="list-inside list-disc text-sm text-red-600 dark:text-red-400">
            {validationErrors.map((err, i) => (
              <li key={i}>
                <span className="font-medium">{err.field}:</span> {err.message}
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Test result */}
      {testResult && (
        <div className="mb-4 rounded-md border border-green-200 bg-green-50 px-3 py-2 text-sm text-green-700 dark:border-green-800 dark:bg-green-900/20 dark:text-green-400">
          {testResult}
        </div>
      )}

      {/* Footer */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={handleTest}
          disabled={testMutation.isPending || !name.trim()}
          className="flex items-center gap-1 rounded-md border border-gray-200 px-3 py-1.5 text-sm text-gray-600 transition-colors hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
        >
          <FlaskConical className="h-4 w-4" aria-hidden="true" />
          {testMutation.isPending ? 'Testing...' : 'Test Rule'}
        </button>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleValidateAndSave}
            disabled={isSaving || validateMutation.isPending || !name.trim()}
            className="flex items-center gap-1 rounded-md bg-indigo-600 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Save className="h-4 w-4" aria-hidden="true" />
            {isSaving
              ? 'Saving...'
              : validateMutation.isPending
                ? 'Validating...'
                : isEditing
                  ? 'Update'
                  : 'Create'}
          </button>
        </div>
      </div>
    </div>
  );
}
