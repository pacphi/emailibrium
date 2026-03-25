import type { ReactNode } from 'react';

interface QuickAction {
  id: string;
  label: string;
  description: string;
  icon: ReactNode;
  href?: string;
  onClick?: () => void;
}

interface QuickActionsProps {
  onAction?: (actionId: string) => void;
  syncing?: boolean;
  hasAccounts?: boolean;
}

// Actions that require at least one connected account.
const ACCOUNT_REQUIRED = new Set([
  'clean-inbox',
  'view-insights',
  'chat-ai',
  'manage-rules',
  'sync-now',
]);

export function QuickActions({ onAction, syncing, hasAccounts = true }: QuickActionsProps) {
  const actions: QuickAction[] = [
    {
      id: 'clean-inbox',
      label: 'Clean Inbox',
      description: 'Remove clutter and organize',
      icon: <SparklesIcon />,
      href: '/inbox-cleaner',
    },
    {
      id: 'view-insights',
      label: 'View Insights',
      description: 'Email patterns and analytics',
      icon: <ChartIcon />,
      href: '/insights',
    },
    {
      id: 'chat-ai',
      label: 'Chat with AI',
      description: 'Ask about your emails',
      icon: <ChatIcon />,
      href: '/chat',
    },
    {
      id: 'manage-rules',
      label: 'Manage Rules',
      description: 'Automation and filters',
      icon: <CogIcon />,
      href: '/rules',
    },
    {
      id: 'add-account',
      label: 'Add Account',
      description: 'Connect email provider',
      icon: <PlusIcon />,
      href: '/settings',
    },
    {
      id: 'sync-now',
      label: syncing ? 'Syncing...' : 'Sync Now',
      description: syncing ? 'In progress' : 'Fetch latest emails',
      icon: <RefreshIcon spinning={syncing} />,
    },
  ];

  function handleClick(action: QuickAction) {
    if (onAction) {
      onAction(action.id);
    }
    if (action.href) {
      window.location.href = action.href;
    }
  }

  return (
    <section aria-label="Quick actions">
      <h2 className="mb-3 text-lg font-semibold text-gray-900 dark:text-white">Quick Actions</h2>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
        {actions.map((action) => {
          const disabled = !hasAccounts && ACCOUNT_REQUIRED.has(action.id);
          return (
            <button
              key={action.id}
              type="button"
              disabled={disabled}
              onClick={() => handleClick(action)}
              className={`flex flex-col items-center gap-2 rounded-xl border p-4 text-center shadow-sm transition-all focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 ${
                disabled
                  ? 'cursor-not-allowed border-gray-100 bg-gray-50 opacity-50 dark:border-gray-800 dark:bg-gray-900'
                  : 'border-gray-200 bg-white hover:border-indigo-300 hover:shadow-md dark:border-gray-700 dark:bg-gray-800 dark:hover:border-indigo-600'
              }`}
              aria-label={action.label}
            >
              <div
                className={`flex h-10 w-10 items-center justify-center rounded-lg ${
                  disabled
                    ? 'bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-600'
                    : 'bg-indigo-50 text-indigo-600 dark:bg-indigo-900/30 dark:text-indigo-400'
                }`}
              >
                {action.icon}
              </div>
              <div>
                <p
                  className={`text-sm font-medium ${disabled ? 'text-gray-400 dark:text-gray-600' : 'text-gray-900 dark:text-white'}`}
                >
                  {action.label}
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  {disabled ? 'Connect an account first' : action.description}
                </p>
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}

function SparklesIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z"
      />
    </svg>
  );
}

function ChartIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z"
      />
    </svg>
  );
}

function ChatIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
      />
    </svg>
  );
}

function CogIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
      />
      <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg
      className="h-5 w-5"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="M12 4v16m8-8H4" />
    </svg>
  );
}

function RefreshIcon({ spinning }: { spinning?: boolean }) {
  return (
    <svg
      className={`h-5 w-5 ${spinning ? 'animate-spin' : ''}`}
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
      />
    </svg>
  );
}
