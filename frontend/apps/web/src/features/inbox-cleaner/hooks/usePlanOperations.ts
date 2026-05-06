// React Query infinite-pagination hook for cleanup plan operations.
//
// Fetches `/api/v1/cleanup/plan/:id/operations` page by page. Each page
// returns `{ items, nextCursor }`; we feed `nextCursor` back as the next
// page-param. When `nextCursor` is null the cursor terminates.

import { useInfiniteQuery } from '@tanstack/react-query';
import type { ListOpsResponse, PlanId, PlannedOperation, RiskLevel } from '@emailibrium/types';
import { listPlanOperations } from '@emailibrium/api';

export interface UsePlanOperationsOptions {
  risk?: RiskLevel;
  accountId?: string;
  /** Page size; defaults to 100, max enforced server-side at 1000. */
  pageSize?: number;
  action?: string;
  enabled?: boolean;
}

export interface UsePlanOperationsResult {
  pages: ListOpsResponse[];
  items: PlannedOperation[];
  fetchNextPage: () => void;
  hasNextPage: boolean;
  isLoading: boolean;
  isFetching: boolean;
  isFetchingNextPage: boolean;
  isError: boolean;
  error: unknown;
  refetch: () => void;
}

export function usePlanOperations(
  planId: PlanId | null,
  userId: string,
  opts: UsePlanOperationsOptions = {},
): UsePlanOperationsResult {
  const { risk, accountId, action, pageSize = 100, enabled = true } = opts;

  const query = useInfiniteQuery({
    queryKey: ['cleanup', 'plan-operations', planId, userId, { risk, accountId, action, pageSize }],
    enabled: enabled && Boolean(planId) && Boolean(userId),
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      listPlanOperations(planId as PlanId, {
        userId,
        cursor: pageParam,
        limit: pageSize,
        risk,
        action,
        accountId,
      }),
    getNextPageParam: (lastPage) => lastPage.nextCursor ?? undefined,
    staleTime: 30_000,
  });

  const pages: ListOpsResponse[] = query.data?.pages ?? [];
  const items: PlannedOperation[] = pages.flatMap((p: ListOpsResponse) => p.items);

  return {
    pages,
    items,
    fetchNextPage: () => {
      void query.fetchNextPage();
    },
    hasNextPage: Boolean(query.hasNextPage),
    isLoading: query.isLoading,
    isFetching: query.isFetching,
    isFetchingNextPage: query.isFetchingNextPage,
    isError: query.isError,
    error: query.error,
    refetch: () => {
      void query.refetch();
    },
  };
}
