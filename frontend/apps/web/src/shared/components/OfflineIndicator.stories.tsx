import React from 'react';
import type { Meta, StoryObj } from '@storybook/react';
import { OfflineIndicator } from './OfflineIndicator';

/**
 * The OfflineIndicator uses framer-motion AnimatePresence and the useOffline
 * hook internally. In Storybook we cannot easily toggle the browser's
 * navigator.onLine, so the stories document the component in its visible
 * (offline) state. The component renders nothing when online.
 */
const meta = {
  title: 'Shared/OfflineIndicator',
  component: OfflineIndicator,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Displays an animated banner at the top of the viewport when the user loses network connectivity. Automatically dismisses when the connection is restored.',
      },
    },
  },
} satisfies Meta<typeof OfflineIndicator>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Online: Story = {
  name: 'Online (hidden)',
  parameters: {
    docs: {
      description: {
        story:
          'When the user is online the component renders nothing. This story is intentionally blank.',
      },
    },
  },
};

export const Offline: Story = {
  name: 'Offline (visible)',
  parameters: {
    docs: {
      description: {
        story:
          'Simulates the offline state. In a real browser, toggling airplane mode or disconnecting the network would trigger this banner.',
      },
    },
  },
  decorators: [
    (Story: React.ComponentType) => {
      // Override navigator.onLine for this story frame
      Object.defineProperty(navigator, 'onLine', { value: false, writable: true });
      window.dispatchEvent(new Event('offline'));
      return (
        <div className="relative min-h-[200px]">
          <Story />
          <div className="pt-16 text-center text-sm text-gray-500">
            The amber banner appears at the top of the viewport.
          </div>
        </div>
      );
    },
  ],
};

export const WithContent: Story = {
  name: 'Offline with page content',
  decorators: [
    (Story: React.ComponentType) => {
      Object.defineProperty(navigator, 'onLine', { value: false, writable: true });
      window.dispatchEvent(new Event('offline'));
      return (
        <div className="relative min-h-[400px]">
          <Story />
          <div className="p-8 pt-16">
            <h1 className="text-xl font-bold">Inbox</h1>
            <p className="mt-2 text-gray-600">
              Your emails are still accessible in offline mode via the local cache.
            </p>
          </div>
        </div>
      );
    },
  ],
};
