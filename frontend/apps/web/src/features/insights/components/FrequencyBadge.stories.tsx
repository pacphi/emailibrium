import type { Meta, StoryObj } from '@storybook/react';
import { FrequencyBadge } from './FrequencyBadge';

const meta = {
  title: 'Insights/FrequencyBadge',
  component: FrequencyBadge,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'A compact pill badge displaying an email recurrence frequency (daily, weekly, biweekly, monthly, quarterly, irregular). Each variant has a distinct color and icon.',
      },
    },
  },
  argTypes: {
    frequency: {
      control: 'radio',
      options: ['daily', 'weekly', 'biweekly', 'monthly', 'quarterly', 'irregular'],
    },
  },
} satisfies Meta<typeof FrequencyBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Daily: Story = {
  args: { frequency: 'daily' },
};

export const Weekly: Story = {
  args: { frequency: 'weekly' },
};

export const Monthly: Story = {
  args: { frequency: 'monthly' },
};

export const Irregular: Story = {
  args: { frequency: 'irregular' },
};
