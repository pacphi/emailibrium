import type { Meta, StoryObj } from '@storybook/react';
import { useEffect } from 'react';
import { ToastContainer } from './Toast';
import { useToastStore } from '@/shared/stores/toastStore';

/**
 * The ToastContainer reads from a zustand store. These stories seed the store
 * with sample toasts so the component has data to render.
 */
const meta = {
  title: 'Shared/Toast',
  component: ToastContainer,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'A stack of dismissible toast notifications rendered in the bottom-right corner. Each toast includes an icon, message, dismiss button, and an auto-dismiss progress bar.',
      },
    },
  },
} satisfies Meta<typeof ToastContainer>;

export default meta;
type Story = StoryObj<typeof meta>;

function ToastSeeder({
  type,
  message,
}: {
  type: 'success' | 'error' | 'warning' | 'info';
  message: string;
}) {
  const addToast = useToastStore((s) => s.addToast);
  const clearAll = useToastStore((s) => s.clearAll);

  useEffect(() => {
    clearAll();
    addToast(message, type, 0); // duration=0 prevents auto-dismiss in stories
    return () => clearAll();
  }, [addToast, clearAll, message, type]);

  return null;
}

export const Success: Story = {
  decorators: [
    (Story) => (
      <div className="relative min-h-[200px]">
        <ToastSeeder type="success" message="Email rules applied successfully." />
        <Story />
      </div>
    ),
  ],
};

export const Error: Story = {
  decorators: [
    (Story) => (
      <div className="relative min-h-[200px]">
        <ToastSeeder type="error" message="Failed to sync inbox. Please try again." />
        <Story />
      </div>
    ),
  ],
};

export const Warning: Story = {
  decorators: [
    (Story) => (
      <div className="relative min-h-[200px]">
        <ToastSeeder type="warning" message="Your session will expire in 5 minutes." />
        <Story />
      </div>
    ),
  ],
};

export const Info: Story = {
  decorators: [
    (Story) => (
      <div className="relative min-h-[200px]">
        <ToastSeeder type="info" message="New emails detected. Refresh to see updates." />
        <Story />
      </div>
    ),
  ],
};
