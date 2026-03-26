import { useState } from 'react';
import { Inbox, Tag, Hash, Bell, ChevronDown, ChevronRight } from 'lucide-react';

export interface SidebarGroup {
  id: string;
  label: string;
  icon: 'inbox' | 'category' | 'topic' | 'subscription';
  unreadCount?: number;
  children?: SidebarGroup[];
}

interface EmailSidebarProps {
  groups: SidebarGroup[];
  activeGroupId: string;
  onGroupSelect: (groupId: string) => void;
}

const iconMap = {
  inbox: Inbox,
  category: Tag,
  topic: Hash,
  subscription: Bell,
} as const;

function SidebarItem({
  group,
  isActive,
  onSelect,
  depth,
}: {
  group: SidebarGroup;
  isActive: boolean;
  onSelect: (id: string) => void;
  depth: number;
}) {
  const Icon = iconMap[group.icon];

  return (
    <button
      type="button"
      onClick={() => onSelect(group.id)}
      className={`
        flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm font-medium transition-colors
        ${
          isActive
            ? 'bg-indigo-50 text-indigo-700 dark:bg-indigo-900/50 dark:text-indigo-200'
            : 'text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-700/50'
        }
      `}
      style={{ paddingLeft: `${8 + depth * 16}px` }}
      aria-current={isActive ? 'page' : undefined}
    >
      <Icon className="h-4 w-4 shrink-0" aria-hidden="true" />
      <span className="flex-1 truncate text-left">{group.label}</span>
      {group.unreadCount != null && group.unreadCount > 0 && (
        <span
          className={`
            ml-auto inline-flex h-5 min-w-[20px] items-center justify-center rounded-full px-1.5 text-xs font-semibold
            ${
              isActive
                ? 'bg-indigo-200 text-indigo-800 dark:bg-indigo-800 dark:text-indigo-100'
                : 'bg-gray-200 text-gray-600 dark:bg-gray-600 dark:text-gray-200'
            }
          `}
        >
          {group.unreadCount.toLocaleString()}
        </span>
      )}
    </button>
  );
}

function CollapsibleSection({
  title,
  children,
  defaultOpen = true,
}: {
  title: string;
  children: React.ReactNode;
  defaultOpen?: boolean;
}) {
  const [isOpen, setIsOpen] = useState(defaultOpen);

  return (
    <div>
      <button
        type="button"
        onClick={() => setIsOpen((prev) => !prev)}
        className="flex w-full items-center gap-1 px-2 py-1 text-xs font-semibold uppercase tracking-wider text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
        aria-expanded={isOpen}
      >
        {isOpen ? (
          <ChevronDown className="h-3 w-3" aria-hidden="true" />
        ) : (
          <ChevronRight className="h-3 w-3" aria-hidden="true" />
        )}
        {title}
      </button>
      {isOpen && <div className="mt-0.5 space-y-0.5">{children}</div>}
    </div>
  );
}

export function EmailSidebar({ groups, activeGroupId, onGroupSelect }: EmailSidebarProps) {
  const inboxGroups = groups.filter((g) => g.icon === 'inbox');
  const categoryGroups = groups.filter((g) => g.icon === 'category');
  const topicGroups = groups.filter((g) => g.icon === 'topic');
  const subscriptionGroups = groups.filter((g) => g.icon === 'subscription');

  return (
    <nav
      className="flex h-full w-56 shrink-0 flex-col overflow-y-auto border-r border-gray-200 bg-white p-2 dark:border-gray-700 dark:bg-gray-800 lg:w-60"
      aria-label="Email groups"
    >
      <div className="space-y-0.5">
        {inboxGroups.map((group) => (
          <SidebarItem
            key={group.id}
            group={group}
            isActive={activeGroupId === group.id}
            onSelect={onGroupSelect}
            depth={0}
          />
        ))}
      </div>

      {categoryGroups.length > 0 && (
        <div className="mt-4">
          <CollapsibleSection title="Categories">
            {categoryGroups.map((group) => (
              <SidebarItem
                key={group.id}
                group={group}
                isActive={activeGroupId === group.id}
                onSelect={onGroupSelect}
                depth={1}
              />
            ))}
          </CollapsibleSection>
        </div>
      )}

      {topicGroups.length > 0 && (
        <div className="mt-4">
          <CollapsibleSection title="Topics">
            {topicGroups.map((group) => (
              <SidebarItem
                key={group.id}
                group={group}
                isActive={activeGroupId === group.id}
                onSelect={onGroupSelect}
                depth={1}
              />
            ))}
          </CollapsibleSection>
        </div>
      )}

      {subscriptionGroups.length > 0 && (
        <div className="mt-4">
          <CollapsibleSection title="Subscriptions" defaultOpen={false}>
            {subscriptionGroups.map((group) => (
              <SidebarItem
                key={group.id}
                group={group}
                isActive={activeGroupId === group.id}
                onSelect={onGroupSelect}
                depth={1}
              />
            ))}
          </CollapsibleSection>
        </div>
      )}
    </nav>
  );
}
