import React from 'react';
import type { Meta, StoryObj } from '@storybook/react';
import { SkipToContent } from './SkipToContent';

const meta = {
  title: 'Shared/SkipToContent',
  component: SkipToContent,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'An accessibility skip-link that becomes visible on keyboard focus, allowing users to jump directly to the main content area.',
      },
    },
  },
  argTypes: {
    targetId: {
      control: 'text',
      description: 'The id of the element to skip to.',
    },
    label: {
      control: 'text',
      description: 'Custom label for the skip link.',
    },
  },
} satisfies Meta<typeof SkipToContent>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {},
  decorators: [
    (Story: React.ComponentType) => (
      <div>
        <Story />
        <p className="mt-16 text-sm text-gray-500">
          Press <kbd className="rounded border px-1 font-mono text-xs">Tab</kbd> to reveal the skip
          link.
        </p>
        <div id="main-content" className="mt-4 rounded border p-4">
          <h2 className="font-semibold">Main Content</h2>
          <p>This is the target of the skip link.</p>
        </div>
      </div>
    ),
  ],
};

export const CustomLabel: Story = {
  args: {
    label: 'Jump to dashboard',
    targetId: 'dashboard',
  },
  decorators: [
    (Story: React.ComponentType) => (
      <div>
        <Story />
        <p className="mt-16 text-sm text-gray-500">
          Press <kbd className="rounded border px-1 font-mono text-xs">Tab</kbd> to reveal the skip
          link.
        </p>
        <div id="dashboard" className="mt-4 rounded border p-4">
          <h2 className="font-semibold">Dashboard</h2>
        </div>
      </div>
    ),
  ],
};

export const FocusedState: Story = {
  args: {},
  parameters: {
    pseudo: { focus: true },
  },
  decorators: [
    (Story: React.ComponentType) => (
      <div>
        <Story />
        <p className="mt-16 text-sm text-gray-400">
          This story simulates the focused state of the skip link.
        </p>
      </div>
    ),
  ],
};
