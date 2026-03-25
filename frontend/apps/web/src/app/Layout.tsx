import { type ReactNode, useState } from 'react';
import {
  LayoutDashboard,
  Mail,
  Sparkles,
  BarChart3,
  Cog,
  MessageSquare,
  ListChecks,
  ChevronLeft,
  ChevronRight,
} from 'lucide-react';
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

const NAV_ITEMS: {
  href: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  needsLlm?: boolean;
}[] = [
  { href: '/command-center', label: 'Command Center', icon: LayoutDashboard },
  { href: '/email', label: 'Email', icon: Mail },
  { href: '/inbox-cleaner', label: 'Inbox Cleaner', icon: Sparkles, needsLlm: true },
  { href: '/insights', label: 'Insights', icon: BarChart3 },
  { href: '/rules', label: 'Rules', icon: ListChecks },
  { href: '/chat', label: 'Chat', icon: MessageSquare, needsLlm: true },
  { href: '/settings', label: 'Settings', icon: Cog },
];

function Sidebar() {
  const { sidebarPosition, llmProvider } = useSettings();
  const [collapsed, setCollapsed] = useState(false);
  const borderClass = sidebarPosition === 'right' ? 'border-l' : 'border-r';
  const hasLlm = llmProvider !== 'none';

  return (
    <nav
      className={`${collapsed ? 'w-16' : 'w-52'} ${borderClass} border-gray-200 bg-white dark:bg-gray-800 dark:border-gray-700 flex flex-col transition-all duration-200`}
    >
      {/* Header */}
      <div className="flex items-center justify-between p-3 border-b border-gray-200 dark:border-gray-700">
        {!collapsed && (
          <h1 className="text-lg font-bold text-indigo-600">Emailibrium</h1>
        )}
        <button
          type="button"
          onClick={() => setCollapsed(!collapsed)}
          className={`p-1 rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300 transition-colors ${collapsed ? 'mx-auto' : ''}`}
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? (
            <ChevronRight className="h-4 w-4" />
          ) : (
            <ChevronLeft className="h-4 w-4" />
          )}
        </button>
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
            disabled={item.needsLlm && !hasLlm}
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
  disabled?: boolean;
}

function NavItem({ href, label, icon: Icon, collapsed, needsLlm, disabled }: NavItemProps) {
  const isActive = window.location.pathname.startsWith(href);

  if (disabled) {
    return (
      <div
        className={`flex items-center gap-3 rounded-md text-sm font-medium text-gray-400 dark:text-gray-600 cursor-not-allowed ${collapsed ? 'justify-center px-2 py-2' : 'px-3 py-2'}`}
        title={
          needsLlm
            ? `${label} — Configure an LLM provider in Settings > AI / LLM`
            : label
        }
      >
        <Icon className="h-4 w-4 shrink-0" />
        {!collapsed && (
          <span className="flex-1 truncate">{label}</span>
        )}
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
