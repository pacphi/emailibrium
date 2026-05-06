import type { Meta, StoryObj } from '@storybook/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { CleanupPlanSummary, ListPlansResponse } from '@emailibrium/types';
import { CleanupHistory } from './CleanupHistory';

const USER_ID = 'storybook-user';

function makeQueryClient(items: CleanupPlanSummary[]): QueryClient {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  const resp: ListPlansResponse = { items };
  qc.setQueryData(['cleanup', 'plans', USER_ID], resp);
  return qc;
}

function plan(overrides: Partial<CleanupPlanSummary> = {}): CleanupPlanSummary {
  return {
    id: '0190e7c0-1234-7000-8000-00000000aaaa',
    createdAt: new Date(Date.now() - 60 * 60_000).toISOString(),
    validUntil: new Date(Date.now() + 30 * 60_000).toISOString(),
    status: 'ready',
    totals: {
      totalOperations: 142,
      byAction: { archive: 142 },
      byAccount: { 'acct-a': 142 },
      bySource: { manual: 142 },
    },
    risk: { low: 130, medium: 10, high: 2 },
    warningsCount: 0,
    ...overrides,
  };
}

const meta = {
  title: 'InboxCleaner/History/CleanupHistory',
  component: CleanupHistory,
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          "Phase D — plan history list view at `/cleanup/history`. Lists the user's last 20 cleanup plans with a status pill, totals, and risk breakdown; rows link to the read-only review.",
      },
    },
  },
  args: { userId: USER_ID },
} satisfies Meta<typeof CleanupHistory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {
  decorators: [
    (Story) => (
      <QueryClientProvider client={makeQueryClient([])}>
        <Story />
      </QueryClientProvider>
    ),
  ],
};

export const WithPlans: Story = {
  decorators: [
    (Story) => {
      const items: CleanupPlanSummary[] = [
        plan({
          id: '0190e7c0-1234-7000-8000-00000000aaaa',
          status: 'applied',
          createdAt: new Date(Date.now() - 24 * 60 * 60_000).toISOString(),
          totals: {
            totalOperations: 1450,
            byAction: { archive: 1200, delete: 250 },
            byAccount: { 'acct-a': 1000, 'acct-b': 450 },
            bySource: { rule: 1450 },
          },
          risk: { low: 1200, medium: 200, high: 50 },
        }),
        plan({
          id: '0190e7c0-1234-7000-8000-00000000bbbb',
          status: 'partiallyApplied',
          createdAt: new Date(Date.now() - 3 * 60 * 60_000).toISOString(),
          totals: {
            totalOperations: 87,
            byAction: { addLabel: 87 },
            byAccount: { 'acct-a': 87 },
            bySource: { rule: 87 },
          },
          risk: { low: 87, medium: 0, high: 0 },
          warningsCount: 1,
        }),
        plan({
          id: '0190e7c0-1234-7000-8000-00000000cccc',
          status: 'ready',
          createdAt: new Date(Date.now() - 30 * 60_000).toISOString(),
        }),
        plan({
          id: '0190e7c0-1234-7000-8000-00000000dddd',
          status: 'expired',
          createdAt: new Date(Date.now() - 7 * 24 * 60 * 60_000).toISOString(),
          totals: {
            totalOperations: 12,
            byAction: { unsubscribe: 12 },
            byAccount: { 'acct-a': 12 },
            bySource: { subscription: 12 },
          },
          risk: { low: 12, medium: 0, high: 0 },
        }),
      ];
      return (
        <QueryClientProvider client={makeQueryClient(items)}>
          <Story />
        </QueryClientProvider>
      );
    },
  ],
};
