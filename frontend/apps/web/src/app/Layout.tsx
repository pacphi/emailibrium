import type { ReactNode } from 'react';
import { useSettings } from '../features/settings/hooks/useSettings';

interface LayoutProps {
  children: ReactNode;
}

/** App shell layout with sidebar navigation and main content area. */
export function Layout({ children }: LayoutProps) {
  const { sidebarPosition } = useSettings();
  const isRight = sidebarPosition === 'right';

  return (
    <div
      className={`flex h-screen bg-gray-50 dark:bg-gray-900 ${isRight ? 'flex-row-reverse' : ''}`}
    >
      <Sidebar />
      <main className="flex-1 overflow-auto">{children}</main>
    </div>
  );
}

function Sidebar() {
  const { sidebarPosition } = useSettings();
  const borderClass = sidebarPosition === 'right' ? 'border-l' : 'border-r';

  return (
    <nav
      className={`w-64 ${borderClass} border-gray-200 bg-white dark:bg-gray-800 dark:border-gray-700 flex flex-col`}
    >
      <div className="p-4 border-b border-gray-200 dark:border-gray-700">
        <h1 className="text-xl font-bold text-indigo-600">Emailibrium</h1>
      </div>
      <div className="flex-1 p-2 space-y-1">
        <NavItem href="/command-center" label="Command Center" />
        <NavItem href="/email" label="Email" />
        <NavItem href="/inbox-cleaner" label="Inbox Cleaner" />
        <NavItem href="/insights" label="Insights" />
        <NavItem href="/rules" label="Rules" />
        <NavItem href="/chat" label="Chat" />
        <NavItem href="/settings" label="Settings" />
      </div>
    </nav>
  );
}

function NavItem({ href, label }: { href: string; label: string }) {
  const isActive = window.location.pathname.startsWith(href);
  return (
    <a
      href={href}
      className={`block px-3 py-2 rounded-md text-sm font-medium transition-colors ${
        isActive
          ? 'bg-indigo-50 text-indigo-700 dark:bg-indigo-900 dark:text-indigo-200'
          : 'text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700'
      }`}
    >
      {label}
    </a>
  );
}
