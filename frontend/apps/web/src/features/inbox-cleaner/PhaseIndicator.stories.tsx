import type { Meta, StoryObj } from '@storybook/react';
import { PhaseIndicator } from './PhaseIndicator';

const meta = {
  title: 'InboxCleaner/PhaseIndicator',
  component: PhaseIndicator,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'A horizontal stepper showing the current ingestion phase (Sync, Embed, Categorize, Cluster, Analyze, Done). Completed phases show a checkmark; the active phase pulses.',
      },
    },
  },
  argTypes: {
    currentPhase: {
      control: 'radio',
      options: ['syncing', 'embedding', 'categorizing', 'clustering', 'analyzing', 'complete'],
    },
  },
} satisfies Meta<typeof PhaseIndicator>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Syncing: Story = {
  args: { currentPhase: 'syncing' },
};

export const Embedding: Story = {
  args: { currentPhase: 'embedding' },
};

export const Categorizing: Story = {
  args: { currentPhase: 'categorizing' },
};

export const Complete: Story = {
  args: { currentPhase: 'complete' },
};
