import { useState } from 'react';
import { Plus, Sparkles } from 'lucide-react';
import type { Rule, RuleSuggestion } from '@emailibrium/types';
import { useRulesQuery } from './hooks/useRules';
import { ActiveRulesList } from './ActiveRulesList';
import { RuleEditor } from './RuleEditor';
import { AISuggestions } from './AISuggestions';
import { RuleMetrics } from './RuleMetrics';

type TabId = 'active' | 'templates' | 'suggestions' | 'metrics';

const tabs: { id: TabId; label: string }[] = [
  { id: 'active', label: 'Active Rules' },
  { id: 'templates', label: 'Templates' },
  { id: 'suggestions', label: 'AI Suggestions' },
  { id: 'metrics', label: 'Metrics' },
];

export function RulesStudio() {
  const [activeTab, setActiveTab] = useState<TabId>('active');
  const [editingRule, setEditingRule] = useState<Rule | undefined>(undefined);
  const [isEditorOpen, setIsEditorOpen] = useState(false);

  const rulesQuery = useRulesQuery();
  const rules = rulesQuery.data ?? [];

  function handleNewRule() {
    setEditingRule(undefined);
    setIsEditorOpen(true);
  }

  function handleEditRule(rule: Rule) {
    setEditingRule(rule);
    setIsEditorOpen(true);
  }

  function handleCloseEditor() {
    setIsEditorOpen(false);
    setEditingRule(undefined);
  }

  function handleCustomizeSuggestion(suggestion: RuleSuggestion) {
    setEditingRule(suggestion.rule);
    setIsEditorOpen(true);
  }

  return (
    <div className="mx-auto h-full max-w-5xl overflow-y-auto p-4 sm:p-6">
      {/* Header */}
      <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Rules Studio</h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            Create and manage email automation rules.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => {
              setActiveTab('suggestions');
            }}
            className="flex items-center gap-1 rounded-md border border-indigo-200 bg-indigo-50 px-3 py-2 text-sm font-medium text-indigo-700 transition-colors hover:bg-indigo-100 dark:border-indigo-800 dark:bg-indigo-900/30 dark:text-indigo-300 dark:hover:bg-indigo-900/50"
          >
            <Sparkles className="h-4 w-4" aria-hidden="true" />
            Build with AI
          </button>
          <button
            type="button"
            onClick={handleNewRule}
            className="flex items-center gap-1 rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
          >
            <Plus className="h-4 w-4" aria-hidden="true" />
            New Rule
          </button>
        </div>
      </div>

      {/* Rule editor (inline) */}
      {isEditorOpen && (
        <div className="mb-6">
          <RuleEditor rule={editingRule} onClose={handleCloseEditor} />
        </div>
      )}

      {/* Tabs */}
      <div
        className="mb-4 flex border-b border-gray-200 dark:border-gray-700"
        role="tablist"
        aria-label="Rules Studio tabs"
      >
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`
              border-b-2 px-4 py-2 text-sm font-medium transition-colors
              ${
                activeTab === tab.id
                  ? 'border-indigo-600 text-indigo-600 dark:border-indigo-400 dark:text-indigo-400'
                  : 'border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200'
              }
            `}
          >
            {tab.label}
            {tab.id === 'active' && rules.length > 0 && (
              <span className="ml-1.5 inline-flex h-5 min-w-[20px] items-center justify-center rounded-full bg-gray-100 px-1.5 text-xs dark:bg-gray-700">
                {rules.length}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Tab content */}
      <div role="tabpanel">
        {activeTab === 'active' && (
          <ActiveRulesList
            rules={rules}
            isLoading={rulesQuery.isLoading}
            isError={rulesQuery.isError}
            onEdit={handleEditRule}
          />
        )}

        {activeTab === 'templates' && (
          <div className="space-y-3">
            {ruleTemplates.map((template, i) => (
              <div
                key={i}
                className="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800"
              >
                <div>
                  <h4 className="text-sm font-semibold text-gray-900 dark:text-white">
                    {template.name}
                  </h4>
                  <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">
                    {template.description}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    setEditingRule({
                      id: '',
                      name: template.name,
                      conditions: template.conditions,
                      actions: template.actions,
                      isActive: true,
                      matchCount: 0,
                      accuracy: 0,
                      createdAt: '',
                    });
                    setIsEditorOpen(true);
                  }}
                  className="rounded-md border border-gray-200 px-3 py-1.5 text-xs font-medium text-gray-600 transition-colors hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700"
                >
                  Use Template
                </button>
              </div>
            ))}
          </div>
        )}

        {activeTab === 'suggestions' && <AISuggestions onCustomize={handleCustomizeSuggestion} />}

        {activeTab === 'metrics' && <RuleMetrics rules={rules} />}
      </div>
    </div>
  );
}

const ruleTemplates = [
  {
    name: 'Archive Newsletters',
    description: 'Automatically archive newsletter emails after reading.',
    conditions: [{ field: 'category', operator: 'equals', value: 'newsletter' }],
    actions: [{ type: 'archive' }],
  },
  {
    name: 'Star Important Senders',
    description: 'Star emails from your key contacts.',
    conditions: [{ field: 'from', operator: 'contains', value: '' }],
    actions: [{ type: 'star' }],
  },
  {
    name: 'Label Finance Emails',
    description: 'Add a finance label to banking and invoice emails.',
    conditions: [{ field: 'category', operator: 'equals', value: 'finance' }],
    actions: [{ type: 'add-label', value: 'Finance' }],
  },
  {
    name: 'Auto-archive Marketing',
    description: 'Keep marketing emails out of your inbox.',
    conditions: [{ field: 'category', operator: 'equals', value: 'marketing' }],
    actions: [{ type: 'archive' }, { type: 'add-label', value: 'Marketing' }],
  },
  {
    name: 'Forward to Team',
    description: 'Forward emails from a specific sender to your team.',
    conditions: [{ field: 'from', operator: 'contains', value: '' }],
    actions: [{ type: 'forward-to', value: '' }],
  },
];
