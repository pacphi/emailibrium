// Cleanup Dry-Run API client (Phase B foundation).
//
// KNOWN PHASE-A DEVIATION: All Phase A handlers expect `userId` as a query
// parameter (see backend/src/cleanup/api/plan.rs `UserQuery` / `ListOpsQuery`
// / etc.). The rest of the Emailibrium API derives the user from the auth
// header. Phase D will migrate these handlers to auth-derived userIds, at
// which point we can drop the `userId` argument from these wrappers and rely
// purely on the `Authorization: Bearer …` header injected by `client.ts`.
// TODO(phase-d): remove userId arguments once backend handlers stop reading
// `?userId=` from the query string.

import type {
  CleanupPlan,
  CreatePlanResponse,
  ListOpsResponse,
  ListPlansResponse,
  PlanId,
  PlanStatus,
  RiskLevel,
  SampleResponse,
  WizardSelections,
} from '@emailibrium/types';
import { api } from './client.js';

export interface ListOpsParams {
  userId: string;
  cursor?: number;
  limit?: number;
  risk?: RiskLevel;
  /** PlanAction discriminator value, e.g. 'archive', 'addLabel'. */
  action?: string;
  accountId?: string;
}

/** POST /api/v1/cleanup/plan?userId=… */
export async function buildPlan(
  userId: string,
  selections: WizardSelections,
): Promise<CreatePlanResponse> {
  return api
    .post('cleanup/plan', {
      searchParams: { userId },
      json: selections,
    })
    .json<CreatePlanResponse>();
}

/** GET /api/v1/cleanup/plan/:id?userId=… */
export async function getPlan(userId: string, planId: PlanId): Promise<CleanupPlan> {
  return api.get(`cleanup/plan/${planId}`, { searchParams: { userId } }).json<CleanupPlan>();
}

/** GET /api/v1/cleanup/plan/:id/operations?userId=…&cursor=…&limit=…&risk=…&action=…&accountId=… */
export async function listPlanOperations(
  planId: PlanId,
  params: ListOpsParams,
): Promise<ListOpsResponse> {
  const sp: Record<string, string> = { userId: params.userId };
  if (params.cursor !== undefined) sp.cursor = String(params.cursor);
  if (params.limit !== undefined) sp.limit = String(params.limit);
  if (params.risk) sp.risk = params.risk;
  if (params.action) sp.action = params.action;
  if (params.accountId) sp.accountId = params.accountId;
  return api.get(`cleanup/plan/${planId}/operations`, { searchParams: sp }).json<ListOpsResponse>();
}

/** GET /api/v1/cleanup/plan/:id/sample?userId=…&source=…&n=… */
export async function samplePlanOperations(
  planId: PlanId,
  userId: string,
  source: string,
  n = 5,
): Promise<SampleResponse> {
  return api
    .get(`cleanup/plan/${planId}/sample`, {
      searchParams: { userId, source, n: String(n) },
    })
    .json<SampleResponse>();
}

/** POST /api/v1/cleanup/plan/:id/refresh?userId=…&accountId=… */
export async function refreshPlanAccount(
  planId: PlanId,
  userId: string,
  accountId: string,
): Promise<void> {
  await api.post(`cleanup/plan/${planId}/refresh`, {
    searchParams: { userId, accountId },
  });
}

/** DELETE /api/v1/cleanup/plan/:id?userId=… */
export async function cancelPlan(planId: PlanId, userId: string): Promise<void> {
  await api.delete(`cleanup/plan/${planId}`, { searchParams: { userId } });
}

/** GET /api/v1/cleanup/plans?userId=…&status=…&limit=… */
export async function listPlans(
  userId: string,
  status?: PlanStatus,
  limit = 20,
): Promise<ListPlansResponse> {
  const sp: Record<string, string> = { userId, limit: String(limit) };
  if (status) sp.status = status;
  return api.get('cleanup/plans', { searchParams: sp }).json<ListPlansResponse>();
}
