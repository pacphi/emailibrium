import React from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useOffline } from '../hooks/useOffline';

/**
 * A banner that slides in at the top of the viewport when the user goes
 * offline and slides out when connectivity is restored.
 */
export function OfflineIndicator(): React.ReactNode {
  const isOffline = useOffline();

  return (
    <AnimatePresence>
      {isOffline && (
        <motion.div
          key="offline-banner"
          role="alert"
          aria-live="assertive"
          initial={{ y: -48, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: -48, opacity: 0 }}
          transition={{ type: 'spring', stiffness: 400, damping: 30 }}
          className={[
            'fixed inset-x-0 top-0 z-[9998]',
            'flex items-center justify-center gap-2',
            'bg-amber-600 px-4 py-2 text-sm font-medium text-white shadow-md',
          ].join(' ')}
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            className="h-4 w-4 flex-shrink-0"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <line x1="1" y1="1" x2="23" y2="23" />
            <path d="M16.72 11.06A10.94 10.94 0 0 1 19 12.55" />
            <path d="M5 12.55a10.94 10.94 0 0 1 5.17-2.39" />
            <path d="M10.71 5.05A16 16 0 0 1 22.56 9" />
            <path d="M1.42 9a15.91 15.91 0 0 1 4.7-2.88" />
            <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
            <line x1="12" y1="20" x2="12.01" y2="20" />
          </svg>
          <span>You&apos;re offline. Changes will sync when reconnected.</span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
