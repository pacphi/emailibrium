import type { Meta, StoryObj } from '@storybook/react';
import { fn } from 'storybook/test';
import { SubscriptionRow } from './SubscriptionRow';
import type { SubscriptionInsight } from '@emailibrium/types';

const baseSubscription: SubscriptionInsight = {
  senderAddress: 'newsletter@techcrunch.com',
  senderDomain: 'techcrunch.com',
  frequency: 'daily',
  emailCount: 342,
  firstSeen: '2025-01-15T00:00:00Z',
  lastSeen: '2026-03-22T00:00:00Z',
  hasUnsubscribe: true,
  category: 'newsletter',
  suggestedAction: 'unsubscribe',
};

const meta = {
  title: 'InboxCleaner/SubscriptionRow',
  component: SubscriptionRow,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'A single row in the subscription list showing sender info, frequency badge, email count, suggested action, and an unsubscribe indicator. Supports selection via checkbox.',
      },
    },
  },
  argTypes: {
    isSelected: { control: 'boolean' },
  },
  args: {
    onToggle: fn(),
  },
} satisfies Meta<typeof SubscriptionRow>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    subscription: baseSubscription,
    isSelected: false,
  },
};

export const Selected: Story = {
  args: {
    subscription: baseSubscription,
    isSelected: true,
  },
};

export const MonthlyKeep: Story = {
  args: {
    subscription: {
      ...baseSubscription,
      senderAddress: 'billing@stripe.com',
      senderDomain: 'stripe.com',
      frequency: 'monthly',
      emailCount: 24,
      hasUnsubscribe: false,
      category: 'receipt',
      suggestedAction: 'keep',
    },
    isSelected: false,
  },
};

export const IrregularDigest: Story = {
  args: {
    subscription: {
      ...baseSubscription,
      senderAddress: 'updates@github.com',
      senderDomain: 'github.com',
      frequency: 'irregular',
      emailCount: 1287,
      hasUnsubscribe: true,
      category: 'notification',
      suggestedAction: 'digest',
    },
    isSelected: false,
  },
};
