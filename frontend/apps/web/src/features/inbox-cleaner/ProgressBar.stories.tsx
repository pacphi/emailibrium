import type { Meta, StoryObj } from '@storybook/react';
import { ProgressBar } from './ProgressBar';

const meta = {
  title: 'InboxCleaner/ProgressBar',
  component: ProgressBar,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'A horizontal progress bar with status-dependent coloring (pending, running, complete, error). Supports an optional percentage label.',
      },
    },
  },
  argTypes: {
    value: { control: { type: 'range', min: 0, max: 100, step: 1 } },
    status: { control: 'radio', options: ['pending', 'running', 'complete', 'error'] },
    label: { control: 'text' },
    showPercentage: { control: 'boolean' },
  },
} satisfies Meta<typeof ProgressBar>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Pending: Story = {
  args: {
    value: 0,
    label: 'Waiting to start',
    status: 'pending',
    showPercentage: true,
  },
};

export const Running: Story = {
  args: {
    value: 45,
    label: 'Embedding emails',
    status: 'running',
    showPercentage: true,
  },
};

export const Complete: Story = {
  args: {
    value: 100,
    label: 'Categorization complete',
    status: 'complete',
    showPercentage: true,
  },
};

export const Error: Story = {
  args: {
    value: 72,
    label: 'Sync failed',
    status: 'error',
    showPercentage: true,
  },
};
