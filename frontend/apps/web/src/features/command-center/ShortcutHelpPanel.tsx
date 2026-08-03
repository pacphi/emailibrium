import { useMemo } from 'react';
import { X } from 'lucide-react';
import { useActiveShortcuts, useFocusTrap, splitShortcut } from '@/shared/hooks';
import { useShortcutHelpStore } from './hooks/useShortcutHelp';

// Friendly label for each known shortcut key. The SET of shortcuts shown is always
// sourced live from useActiveShortcuts() -- this table only supplies display text for
// keys we recognize, so a shortcut can never silently go missing from the panel just
// because it wasn't added here (it still shows, using the raw key string as its label).
const SHORTCUT_LABELS: Record<string, string> = {
  'cmd+k': 'Open command palette',
  'ctrl+k': 'Open command palette',
  escape: 'Close dialog',
  c: 'Compose new email',
  r: 'Reply to email',
  f: 'Forward email',
  e: 'Archive email',
  '#': 'Delete email',
  'cmd+shift+a': 'Select all emails',
  'ctrl+shift+a': 'Select all emails',
  'cmd+,': 'Open settings',
  'ctrl+,': 'Open settings',
  '?': 'Show keyboard shortcuts',
};

const MODIFIER_SYMBOLS: Record<string, string> = {
  cmd: '⌘',
  meta: '⌘',
  ctrl: 'Ctrl',
  shift: '⇧',
  alt: '⌥',
};

/** e.g. "cmd+shift+a" -> "⌘+⇧+A", "shift+#" -> "⇧+#" */
export function formatShortcutKey(shortcut: string): string {
  const { modifierTokens, key } = splitShortcut(shortcut);
  const modifiers = modifierTokens.map((m) => MODIFIER_SYMBOLS[m] ?? m);
  const keyDisplay =
    key.length === 1 ? key.toUpperCase() : key.charAt(0).toUpperCase() + key.slice(1);
  return [...modifiers, keyDisplay].join('+');
}

interface ShortcutRow {
  label: string;
  keys: string[];
}

/** Groups active shortcut keys that share the same label into one row (e.g. cmd+k and
 * ctrl+k both read "Open command palette"), sourced entirely from the live registry. */
export function buildShortcutRows(activeKeys: string[]): ShortcutRow[] {
  const byLabel = new Map<string, string[]>();
  for (const key of activeKeys) {
    const label = SHORTCUT_LABELS[key] ?? key;
    const existing = byLabel.get(label);
    if (existing) {
      existing.push(key);
    } else {
      byLabel.set(label, [key]);
    }
  }
  return [...byLabel.entries()]
    .map(([label, keys]) => ({ label, keys: [...keys].sort() }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

export function ShortcutHelpPanel() {
  const isOpen = useShortcutHelpStore((s) => s.isOpen);
  const close = useShortcutHelpStore((s) => s.close);

  if (!isOpen) return null;

  return <ShortcutHelpDialog onClose={close} />;
}

/** Mounted only while the panel is open, so useFocusTrap's mount effect lines up with
 * the dialog appearing: it moves focus to the first focusable element (the close
 * button), cycles Tab/Shift+Tab inside the dialog, and restores focus on close. */
function ShortcutHelpDialog({ onClose }: { onClose: () => void }) {
  const activeKeys = useActiveShortcuts();
  const rows = useMemo(() => buildShortcutRows(activeKeys), [activeKeys]);
  const trapRef = useFocusTrap<HTMLDivElement>();

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]">
      <div
        className="fixed inset-0 bg-black/50 backdrop-blur-sm"
        onClick={onClose}
        aria-hidden="true"
      />

      <div
        ref={trapRef}
        className="relative w-full max-w-xl rounded-xl border border-gray-200 bg-white shadow-2xl dark:border-gray-700 dark:bg-gray-800"
        role="dialog"
        aria-label="Keyboard shortcuts"
        aria-modal="true"
      >
        <div className="flex items-center justify-between border-b border-gray-200 px-4 py-3 dark:border-gray-700">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">
            Keyboard Shortcuts
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-700 dark:hover:text-gray-300"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <ul className="max-h-96 space-y-2 overflow-y-auto p-4">
          {rows.length === 0 && (
            <li className="text-sm text-gray-500 dark:text-gray-400">
              No shortcuts are currently active on this page.
            </li>
          )}
          {rows.map((row) => (
            <li key={row.label} className="flex items-center justify-between text-sm">
              <span className="text-gray-700 dark:text-gray-300">{row.label}</span>
              <span className="flex gap-1">
                {row.keys.map((key) => (
                  <kbd
                    key={key}
                    className="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-500 dark:bg-gray-700 dark:text-gray-400"
                  >
                    {formatShortcutKey(key)}
                  </kbd>
                ))}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
