import { useEffect } from 'react';
import { Providers } from './Providers';
import { AppRouter } from './Router';
import {
  useSettings,
  hydrateFromBackend,
} from '../features/settings/hooks/useSettings';

/**
 * Apply the user's theme preference to the <html> element so Tailwind's
 * `dark:` variants activate. Supports 'light', 'dark', and 'system'
 * (which follows the OS via matchMedia).
 */
function useThemeEffect() {
  const theme = useSettings((s) => s.theme);

  useEffect(() => {
    const root = document.documentElement;

    function applyTheme(isDark: boolean) {
      if (isDark) {
        root.classList.add('dark');
      } else {
        root.classList.remove('dark');
      }
    }

    if (theme === 'dark') {
      applyTheme(true);
      return;
    }

    if (theme === 'light') {
      applyTheme(false);
      return;
    }

    // 'system' — follow OS preference and listen for changes.
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    applyTheme(mq.matches);

    function onChange(e: MediaQueryListEvent) {
      applyTheme(e.matches);
    }
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, [theme]);
}

export function App() {
  useThemeEffect();

  // Hydrate settings from backend on first load so that persisted
  // model selections and other server-side settings are restored.
  useEffect(() => {
    hydrateFromBackend();
  }, []);

  return (
    <Providers>
      <AppRouter />
    </Providers>
  );
}
