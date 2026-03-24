import { useState } from 'react';
import { GeneralSettings } from './GeneralSettings';
import { AccountSettings } from './AccountSettings';
import { AISettings } from './AISettings';
import { PrivacySettings } from './PrivacySettings';
import { AppearanceSettings } from './AppearanceSettings';

type SettingsTab = 'general' | 'accounts' | 'ai' | 'privacy' | 'appearance';

interface TabDef {
  id: SettingsTab;
  label: string;
}

const TABS: TabDef[] = [
  { id: 'general', label: 'General' },
  { id: 'accounts', label: 'Accounts' },
  { id: 'ai', label: 'AI / LLM' },
  { id: 'privacy', label: 'Privacy' },
  { id: 'appearance', label: 'Appearance' },
];

function TabPanel({ tab }: { tab: SettingsTab }) {
  switch (tab) {
    case 'general':
      return <GeneralSettings />;
    case 'accounts':
      return <AccountSettings />;
    case 'ai':
      return <AISettings />;
    case 'privacy':
      return <PrivacySettings />;
    case 'appearance':
      return <AppearanceSettings />;
  }
}

export function Settings() {
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');

  return (
    <div className="max-w-4xl mx-auto px-4 py-8 sm:px-6 lg:px-8">
      <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100 mb-6">Settings</h2>

      {/* Tab navigation */}
      <div className="border-b border-gray-200 dark:border-gray-700 mb-6">
        <nav className="-mb-px flex space-x-6 overflow-x-auto" aria-label="Settings tabs">
          {TABS.map((tab) => {
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                onClick={() => setActiveTab(tab.id)}
                className={`whitespace-nowrap pb-3 px-1 text-sm font-medium border-b-2 transition-colors ${
                  isActive
                    ? 'border-indigo-500 text-indigo-600 dark:border-indigo-400 dark:text-indigo-400'
                    : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
                }`}
                aria-current={isActive ? 'page' : undefined}
              >
                {tab.label}
              </button>
            );
          })}
        </nav>
      </div>

      {/* Active panel */}
      <TabPanel tab={activeTab} />
    </div>
  );
}
