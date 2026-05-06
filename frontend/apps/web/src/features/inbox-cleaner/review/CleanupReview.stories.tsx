import type { Meta, StoryObj } from '@storybook/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type {
  CleanupPlan,
  ListOpsResponse,
  PlannedOperation,
  PlanWarning,
} from '@emailibrium/types';
import { CleanupReview } from './CleanupReview';

// ---------------------------------------------------------------------------
// Storybook fixtures for the 5 scenarios required by Phase B DoD:
//   empty plan / low-only plan / mixed-risk plan / large-with-warnings /
//   conflict-warnings.
// ---------------------------------------------------------------------------

const PLAN_ID = '0190e7c0-1234-7000-8000-000000000001';
const USER_ID = 'storybook-user';

function basePlan(overrides: Partial<CleanupPlan> = {}): CleanupPlan {
  const now = new Date().toISOString();
  return {
    id: PLAN_ID,
    userId: USER_ID,
    accountIds: ['acct-a'],
    createdAt: now,
    validUntil: new Date(Date.now() + 30 * 60_000).toISOString(),
    planHash: '0'.repeat(64),
    accountStateEtags: { 'acct-a': { kind: 'gmailHistory', historyId: '1' } },
    status: 'ready',
    totals: { totalOperations: 0, byAction: {}, byAccount: {}, bySource: {} },
    risk: { low: 0, medium: 0, high: 0 },
    warnings: [],
    operations: [],
    ...overrides,
  };
}

function lowOp(seq: number, accountId = 'acct-a'): PlannedOperation {
  return {
    opKind: 'materialized',
    seq,
    accountId,
    emailId: `e${seq}`,
    action: { type: 'archive' },
    source: { type: 'manual' },
    target: null,
    reverseOp: null,
    risk: 'low',
    status: 'pending',
  };
}

function mediumOp(seq: number, accountId = 'acct-a'): PlannedOperation {
  return {
    opKind: 'materialized',
    seq,
    accountId,
    emailId: `e${seq}`,
    action: { type: 'move', kind: 'folder' },
    source: { type: 'cluster', clusterId: 'c1', clusterAction: 'archive' },
    target: { id: 'old', name: 'Old Projects', kind: 'folder' },
    reverseOp: null,
    risk: 'medium',
    status: 'pending',
  };
}

function highOp(seq: number, accountId = 'acct-a'): PlannedOperation {
  return {
    opKind: 'materialized',
    seq,
    accountId,
    emailId: `e${seq}`,
    action: { type: 'delete', permanent: true },
    source: { type: 'manual' },
    target: null,
    reverseOp: { type: 'irreversible' },
    risk: 'high',
    status: 'pending',
  };
}

function predicateOp(seq: number, projectedCount: number): PlannedOperation {
  return {
    opKind: 'predicate',
    seq,
    accountId: 'acct-a',
    predicateKind: 'rule',
    predicateId: 'r1',
    action: { type: 'addLabel', kind: 'label' },
    target: { id: 'L', name: 'Receipts/2026', kind: 'label' },
    source: { type: 'rule', ruleId: 'r1', matchBasis: 'literal' },
    projectedCount,
    sampleEmailIds: ['e1', 'e2', 'e3', 'e4', 'e5'],
    risk: 'low',
    status: 'pending',
    partialAppliedCount: 0,
  };
}

function makeQueryClient(plan: CleanupPlan): QueryClient {
  const qc = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
    },
  });
  qc.setQueryData(['cleanup', 'plan', PLAN_ID, USER_ID], plan);
  // Prime the infinite-query cache for usePlanOperations.
  const opsResponse: ListOpsResponse = {
    items: plan.operations,
    nextCursor: null,
  };
  qc.setQueryData(['cleanup', 'plan-operations', PLAN_ID, USER_ID, { pageSize: 100 }], {
    pages: [opsResponse],
    pageParams: [0],
  });
  return qc;
}

const meta = {
  title: 'InboxCleaner/Review/CleanupReview',
  component: CleanupReview,
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Step 4.5 — Review & Confirm. Renders the plan envelope plus per-account/per-source diff groups, risk acknowledgers, and the three-tier Apply buttons. The five stories cover the Phase B DoD scenarios.',
      },
    },
  },
  args: {
    planId: PLAN_ID,
    userId: USER_ID,
    onCancel: () => {},
  },
} satisfies Meta<typeof CleanupReview>;

export default meta;
type Story = StoryObj<typeof meta>;

// 1. Empty plan
export const EmptyPlan: Story = {
  decorators: [
    (Story) => (
      <QueryClientProvider client={makeQueryClient(basePlan())}>
        <Story />
      </QueryClientProvider>
    ),
  ],
};

// 2. Low-only plan
export const LowOnlyPlan: Story = {
  decorators: [
    (Story) => {
      const ops = [lowOp(1), lowOp(2), lowOp(3), predicateOp(4, 87)];
      const plan = basePlan({
        operations: ops,
        risk: { low: ops.length, medium: 0, high: 0 },
        totals: {
          totalOperations: ops.length,
          byAction: { archive: 3, addLabel: 1 },
          byAccount: { 'acct-a': ops.length },
          bySource: { manual: 3, rule: 1 },
        },
      });
      return (
        <QueryClientProvider client={makeQueryClient(plan)}>
          <Story />
        </QueryClientProvider>
      );
    },
  ],
};

// 3. Mixed-risk plan (Low + Medium + High)
export const MixedRiskPlan: Story = {
  decorators: [
    (Story) => {
      const ops = [lowOp(1), lowOp(2), mediumOp(3), mediumOp(4), highOp(5), highOp(6)];
      const plan = basePlan({
        operations: ops,
        risk: { low: 2, medium: 2, high: 2 },
        totals: {
          totalOperations: 6,
          byAction: { archive: 2, move: 2, deletePermanent: 2 },
          byAccount: { 'acct-a': 6 },
          bySource: { manual: 4, cluster: 2 },
        },
      });
      return (
        <QueryClientProvider client={makeQueryClient(plan)}>
          <Story />
        </QueryClientProvider>
      );
    },
  ],
};

// 4. Large plan with warnings
export const LargePlanWithWarnings: Story = {
  decorators: [
    (Story) => {
      const big = predicateOp(1, 12_500);
      const warnings: PlanWarning[] = [
        {
          type: 'largeGroup',
          source: { type: 'rule', ruleId: 'r1', matchBasis: 'literal' },
          projectedCount: 12_500,
        },
        { type: 'planExceedsThreshold', totalCount: 120_000 },
      ];
      const plan = basePlan({
        operations: [big],
        risk: { low: 1, medium: 0, high: 0 },
        warnings,
        totals: {
          totalOperations: 12_500,
          byAction: { addLabel: 12_500 },
          byAccount: { 'acct-a': 12_500 },
          bySource: { rule: 12_500 },
        },
      });
      return (
        <QueryClientProvider client={makeQueryClient(plan)}>
          <Story />
        </QueryClientProvider>
      );
    },
  ],
};

// 5. Conflict warnings (rule + cluster colliding on same email)
export const ConflictWarnings: Story = {
  decorators: [
    (Story) => {
      const ops = [lowOp(1), mediumOp(2)];
      const warnings: PlanWarning[] = [
        {
          type: 'targetConflict',
          accountId: 'acct-a',
          emailId: 'e1',
          sources: [
            { type: 'rule', ruleId: 'r1', matchBasis: 'literal' },
            { type: 'cluster', clusterId: 'c1', clusterAction: 'archive' },
          ],
        },
      ];
      const plan = basePlan({
        operations: ops,
        risk: { low: 1, medium: 1, high: 0 },
        warnings,
        totals: {
          totalOperations: 2,
          byAction: { archive: 1, move: 1 },
          byAccount: { 'acct-a': 2 },
          bySource: { manual: 1, cluster: 1 },
        },
      });
      return (
        <QueryClientProvider client={makeQueryClient(plan)}>
          <Story />
        </QueryClientProvider>
      );
    },
  ],
};
