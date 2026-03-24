import type { Meta, StoryObj } from '@storybook/react';
import { fn } from '@storybook/test';
import { HealthScoreGauge } from './HealthScoreGauge';

const meta = {
  title: 'Insights/HealthScoreGauge',
  component: HealthScoreGauge,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'A circular SVG gauge displaying the inbox health score (0-100). Color-coded: red for poor (<40), yellow for good (40-70), and green for excellent (>70). Optionally shows an "Improve Score" button.',
      },
    },
  },
  argTypes: {
    score: { control: { type: 'range', min: 0, max: 100, step: 1 } },
    size: { control: { type: 'range', min: 80, max: 300, step: 10 } },
  },
} satisfies Meta<typeof HealthScoreGauge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Excellent: Story = {
  args: {
    score: 87,
    size: 160,
  },
};

export const Good: Story = {
  args: {
    score: 55,
    size: 160,
  },
};

export const Poor: Story = {
  args: {
    score: 22,
    size: 160,
  },
};

export const WithImproveButton: Story = {
  args: {
    score: 45,
    size: 160,
    onImprove: fn(),
  },
};
