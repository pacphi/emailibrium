import { type ReactNode, useEffect, useState } from 'react';
import { APP_VERSION } from '../utils/version';
import {
  LayoutDashboard,
  Mail,
  Wind,
  BarChart3,
  Cog,
  MessageSquare,
  ListChecks,
  ChevronLeft,
} from 'lucide-react';
import { useSettings } from '../features/settings/hooks/useSettings';
import { useSyncStore } from '../shared/stores/syncStore';

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

const NAV_ITEMS: {
  href: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  needsLlm?: boolean;
  needsAccount?: boolean;
}[] = [
  { href: '/command-center', label: 'Command Center', icon: LayoutDashboard },
  { href: '/email', label: 'Email', icon: Mail, needsAccount: true },
  {
    href: '/inbox-cleaner',
    label: 'Inbox Cleaner',
    icon: Wind,
    needsLlm: true,
    needsAccount: true,
  },
  { href: '/insights', label: 'Insights', icon: BarChart3, needsAccount: true },
  { href: '/rules', label: 'Rules', icon: ListChecks, needsAccount: true },
  { href: '/chat', label: 'Chat', icon: MessageSquare, needsLlm: true, needsAccount: true },
  { href: '/settings', label: 'Settings', icon: Cog },
];

function Sidebar() {
  const { sidebarPosition, llmProvider } = useSettings();
  const hasAccounts = useSyncStore((s) => s.hasAccounts);
  const refreshAccounts = useSyncStore((s) => s.refreshAccounts);
  const [collapsed, setCollapsed] = useState(false);
  const borderClass = sidebarPosition === 'right' ? 'border-l' : 'border-r';
  const hasLlm = llmProvider !== 'none';

  // Ensure account state is fresh on mount
  useEffect(() => {
    refreshAccounts();
  }, [refreshAccounts]);

  return (
    <nav
      className={`${collapsed ? 'w-16' : 'w-52'} ${borderClass} border-gray-200 bg-white dark:bg-gray-800 dark:border-gray-700 flex flex-col transition-all duration-200`}
    >
      {/* Brand header — no borders */}
      <div className="flex items-center justify-between px-3 py-3">
        {collapsed ? (
          <button
            type="button"
            onClick={() => setCollapsed(false)}
            className="mx-auto"
            title="Expand sidebar"
          >
            <img
              src="/emailibrium-icon.svg"
              alt="Emailibrium"
              className="h-8 w-8 dark:invert dark:hue-rotate-180"
            />
          </button>
        ) : (
          <>
            <div className="flex items-center gap-1.5">
              <img
                src="/emailibrium-text.svg"
                alt="Emailibrium"
                className="h-5 w-auto dark:invert dark:hue-rotate-180"
              />
              <span
                className="px-1.5 py-0.5 rounded-full text-[10px] font-medium bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 tracking-wide"
                title={`Version ${APP_VERSION}`}
              >
                v{APP_VERSION}
              </span>
            </div>
            <button
              type="button"
              onClick={() => setCollapsed(true)}
              className="p-1 rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300 transition-colors"
              title="Collapse sidebar"
              aria-label="Collapse sidebar"
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
          </>
        )}
      </div>

      {/* Nav items */}
      <div className="flex-1 p-2 space-y-1">
        {NAV_ITEMS.map((item) => (
          <NavItem
            key={item.href}
            href={item.href}
            label={item.label}
            icon={item.icon}
            collapsed={collapsed}
            needsLlm={item.needsLlm}
            needsAccount={item.needsAccount}
            disabled={(item.needsLlm && !hasLlm) || (item.needsAccount && !hasAccounts)}
          />
        ))}
      </div>
    </nav>
  );
}

interface NavItemProps {
  href: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  collapsed: boolean;
  needsLlm?: boolean;
  needsAccount?: boolean;
  disabled?: boolean;
}

function disabledTitle(label: string, needsLlm?: boolean, needsAccount?: boolean): string {
  if (needsAccount) return `${label} — Connect an email account first`;
  if (needsLlm) return `${label} — Configure an LLM provider in Settings > AI / LLM`;
  return label;
}

function NavItem({
  href,
  label,
  icon: Icon,
  collapsed,
  needsLlm,
  needsAccount,
  disabled,
}: NavItemProps) {
  const isActive = window.location.pathname.startsWith(href);

  if (disabled) {
    return (
      <div
        className={`flex items-center gap-3 rounded-md text-sm font-medium text-gray-400 dark:text-gray-600 cursor-not-allowed ${collapsed ? 'justify-center px-2 py-2' : 'px-3 py-2'}`}
        title={disabledTitle(label, needsLlm, needsAccount)}
      >
        <Icon className="h-4 w-4 shrink-0" />
        {!collapsed && <span className="flex-1 truncate">{label}</span>}
        {!collapsed && needsLlm && (
          <span className="text-[10px] font-normal bg-gray-100 dark:bg-gray-700 px-1.5 py-0.5 rounded text-gray-400 dark:text-gray-500">
            LLM
          </span>
        )}
      </div>
    );
  }

  return (
    <a
      href={href}
      className={`flex items-center gap-3 rounded-md text-sm font-medium transition-colors ${collapsed ? 'justify-center px-2 py-2' : 'px-3 py-2'} ${
        isActive
          ? 'bg-indigo-50 text-indigo-700 dark:bg-indigo-900 dark:text-indigo-200'
          : 'text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700'
      }`}
      title={collapsed ? label : undefined}
    >
      <Icon className="h-4 w-4 shrink-0" />
      {!collapsed && <span className="truncate">{label}</span>}
    </a>
  );
}
