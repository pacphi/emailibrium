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
  ApplyOptions,
  BeginApplyResponse,
  CleanupApplyJob,
  CleanupPlan,
  CreatePlanResponse,
  JobId,
  ListOpsResponse,
  ListPlansResponse,
  PlanId,
  PlanStatus,
  RiskLevel,
  RiskMax,
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

// ---------------------------------------------------------------------------
// Phase C: Apply orchestrator
//
// POST /api/v1/cleanup/apply/:planId?userId=…&riskMax=…  → 202 { jobId }
// GET  /api/v1/cleanup/apply/:jobId/stream               → SSE
// POST /api/v1/cleanup/apply/:jobId/cancel               → 204
// GET  /api/v1/cleanup/apply/:jobId                      → CleanupApplyJob
//
// The cancel and get-job handlers do not currently take userId in the query
// string (verified against backend/src/cleanup/api/apply.rs); only begin and
// the SSE stream do.
// ---------------------------------------------------------------------------

/** POST /api/v1/cleanup/apply/:planId?userId=…&riskMax=… */
export async function beginApply(
  planId: PlanId,
  userId: string,
  riskMax: RiskMax,
  opts: ApplyOptions,
): Promise<BeginApplyResponse> {
  return api
    .post(`cleanup/apply/${planId}`, {
      searchParams: { userId, riskMax },
      json: opts,
    })
    .json<BeginApplyResponse>();
}

/** POST /api/v1/cleanup/apply/:jobId/cancel */
export async function cancelApply(jobId: JobId): Promise<void> {
  await api.post(`cleanup/apply/${jobId}/cancel`);
}

/** GET /api/v1/cleanup/apply/:jobId */
export async function getApplyJob(jobId: JobId): Promise<CleanupApplyJob> {
  return api.get(`cleanup/apply/${jobId}`).json<CleanupApplyJob>();
}

/**
 * Build the absolute URL for the apply SSE stream. EventSource bypasses ky,
 * so this is a plain URL builder. The handler does not currently require
 * userId, but we leave the parameter in the signature to ease the Phase D
 * migration when auth-derived user resolution lands.
 */
export function applyStreamUrl(jobId: JobId, _userId: string): string {
  return `/api/v1/cleanup/apply/${encodeURIComponent(jobId)}/stream`;
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

// ---------------------------------------------------------------------------
// Phase D: Telemetry + audit-trail
// ---------------------------------------------------------------------------

/** Payload for the `cleanup_plan_reviewed` telemetry event. */
export interface CleanupPlanReviewedPayload {
  planId: PlanId;
  /** Time spent on the review screen in milliseconds. */
  timeOnReviewMs: number;
  /** Number of PlanDiffGroup expand-toggles fired. */
  expandedGroups: number;
  /** Number of SampleEmailPeek opens fired. */
  samplesViewed: number;
}

/**
 * POST /api/v1/cleanup/telemetry — best-effort. Errors are swallowed by
 * callers; this wrapper still throws so a wrapping `try {} catch {}` can
 * keep a clean stack trace if the caller wants to log.
 */
export async function emitCleanupReviewed(payload: CleanupPlanReviewedPayload): Promise<void> {
  await api.post('cleanup/telemetry', {
    json: { event: 'cleanup_plan_reviewed', ...payload },
  });
}

/** One audit row emitted by the apply orchestrator (Phase D backend). */
export interface CleanupAuditEntry {
  seq: number;
  accountId: string;
  /** "applied" | "failed" | "skipped" — mirrors backend audit-log enum. */
  outcome: 'applied' | 'failed' | 'skipped';
  at: string;
  jobId?: JobId;
  error?: { code: string; message: string };
  skipReason?: string;
}

export interface CleanupAuditResponse {
  items: CleanupAuditEntry[];
}

/**
 * GET /api/v1/cleanup/plan/:id/audit?userId=… — optional Phase D endpoint.
 * Returns 404 when the parallel backend agent hasn't shipped it yet; the
 * caller should treat any error as "audit history unavailable".
 */
export async function listPlanAudit(planId: PlanId, userId: string): Promise<CleanupAuditResponse> {
  return api
    .get(`cleanup/plan/${planId}/audit`, { searchParams: { userId } })
    .json<CleanupAuditResponse>();
}
