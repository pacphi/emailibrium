import type { Meta, StoryObj } from '@storybook/react';
import { fn } from 'storybook/test';
import { ErrorFallback } from './ErrorFallback';

const meta = {
  title: 'Shared/ErrorFallback',
  component: ErrorFallback,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'A generic error display component with three layout variants: page (full viewport), section (inline card), and toast (compact inline). Provides a retry button and optional navigation.',
      },
    },
  },
  argTypes: {
    variant: {
      control: 'radio',
      options: ['page', 'section', 'toast'],
    },
    message: { control: 'text' },
    title: { control: 'text' },
    showHomeLink: { control: 'boolean' },
  },
  args: {
    onRetry: fn(),
  },
} satisfies Meta<typeof ErrorFallback>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Section: Story = {
  args: {
    variant: 'section',
    message: 'Failed to load inbox data. Please try again.',
    title: 'Something went wrong',
  },
};

export const Page: Story = {
  args: {
    variant: 'page',
    message: 'We could not connect to the server. Check your network and try again.',
    title: 'Connection Error',
    showHomeLink: true,
  },
  parameters: {
    layout: 'fullscreen',
  },
};

export const Toast: Story = {
  args: {
    variant: 'toast',
    message: 'Email sync failed. Retrying in 30s.',
  },
};

export const WithoutRetry: Story = {
  args: {
    variant: 'section',
    message: 'This operation is no longer available.',
    title: 'Feature Removed',
    onRetry: undefined,
    showHomeLink: true,
  },
};
