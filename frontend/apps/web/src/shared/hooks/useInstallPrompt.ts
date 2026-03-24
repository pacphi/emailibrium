import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * The `beforeinstallprompt` event is not in the standard TypeScript DOM
 * typings, so we declare a minimal interface for it here.
 */
interface BeforeInstallPromptEvent extends Event {
  readonly platforms: string[];
  readonly userChoice: Promise<{ outcome: 'accepted' | 'dismissed'; platform: string }>;
  prompt(): Promise<void>;
}

interface UseInstallPromptReturn {
  /** Whether the browser supports and has queued a PWA install prompt. */
  canInstall: boolean;
  /** Trigger the native install prompt. No-op if `canInstall` is false. */
  install: () => void;
}

/**
 * Captures the `beforeinstallprompt` event fired by the browser when
 * the PWA meets installability criteria.
 *
 * Returns a boolean indicating whether installation is available and a
 * function to trigger the native install dialog.
 */
export function useInstallPrompt(): UseInstallPromptReturn {
  const [canInstall, setCanInstall] = useState(false);
  const deferredPromptRef = useRef<BeforeInstallPromptEvent | null>(null);

  useEffect(() => {
    const handler = (event: Event): void => {
      // Prevent the mini-infobar from appearing on mobile
      event.preventDefault();
      deferredPromptRef.current = event as BeforeInstallPromptEvent;
      setCanInstall(true);
    };

    window.addEventListener('beforeinstallprompt', handler);

    // If the app is already installed, the `appinstalled` event fires
    const installedHandler = (): void => {
      deferredPromptRef.current = null;
      setCanInstall(false);
    };

    window.addEventListener('appinstalled', installedHandler);

    return () => {
      window.removeEventListener('beforeinstallprompt', handler);
      window.removeEventListener('appinstalled', installedHandler);
    };
  }, []);

  const install = useCallback((): void => {
    const prompt = deferredPromptRef.current;
    if (!prompt) {
      return;
    }

    prompt.prompt();

    prompt.userChoice.then(({ outcome }) => {
      if (outcome === 'accepted') {
        deferredPromptRef.current = null;
        setCanInstall(false);
      }
    });
  }, []);

  return { canInstall, install };
}
