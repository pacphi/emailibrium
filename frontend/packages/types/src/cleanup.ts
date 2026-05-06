// Cleanup Dry-Run domain types.
//
// These types mirror the backend serde wire format for the
// `/api/v1/cleanup` surface (Phase A). Backend DTOs use
// `#[serde(rename_all = "camelCase")]`, so all field names here are camelCase.
//
// Where backend serde tags are camelCase enums (`tag = "type"` /
// `tag = "kind"` / `tag = "opKind"` with `rename_all = "camelCase"`), the
// discriminator string values are the camelCase forms of the variant names.
// Example: `AccountStateEtag::ImapUvms` → `{ "kind": "imapUvms", ... }`.
//
// Some enum string values intentionally use snake_case because the backend
// emits them as snake_case strings (e.g. `PlanStatus::PartiallyApplied`
// renders as `"partially_applied"` per backend test asserts in `operation.rs`
// — see `PlanStatus::as_str` and `serde(rename_all = "camelCase")` interaction;
// because the backend API uses serde rename_all="camelCase" the actual JSON
// is camelCase like `"partiallyApplied"`. Verified by reading the enum
// declarations: PlanStatus serde uses `rename_all = "camelCase"` so wire is
// camelCase. SkipReason similarly camelCase: `stateDrift`, `userCancelled`.).
//
// IMPORTANT: For PlanStatus and SkipReason and PredicateStatus, the wire
// values are camelCase (because the enums use `rename_all = "camelCase"`).
// The Rust `as_str()` helpers return snake_case but those are for internal
// DB storage, NOT the JSON wire format. We mirror the JSON wire format.

// ---------------------------------------------------------------------------
// Enums (string unions matching backend serde camelCase output)
// ---------------------------------------------------------------------------

export type RiskLevel = 'low' | 'medium' | 'high';
export type RiskMax = 'low' | 'medium' | 'high';

export type PlanStatus =
  | 'draft'
  | 'ready'
  | 'applying'
  | 'applied'
  | 'partiallyApplied'
  | 'failed'
  | 'expired'
  | 'cancelled';

export type OperationStatus = 'pending' | 'applied' | 'failed' | 'skipped';

export type PredicateStatus =
  | 'pending'
  | 'expanding'
  | 'expanded'
  | 'applied'
  | 'partiallyApplied'
  | 'failed'
  | 'skipped';

export type SkipReason = 'stateDrift' | 'unacknowledged' | 'dedup' | 'userCancelled';

export type CleanupProvider = 'gmail' | 'outlook' | 'imap' | 'pop3';

export type MoveKind = 'folder' | 'label';

export type UnsubscribeMethodKind = 'listUnsubscribePost' | 'mailto' | 'webLink' | 'none';

export type CleanupClusterAction = 'archive' | 'deleteSoft' | 'deletePermanent' | 'label';

// Renamed to avoid clashing with the existing user-preference ArchiveStrategy
// in `auth.ts` (`'instant' | 'delayed' | 'manual'`).
export type CleanupArchiveStrategy = 'olderThan30d' | 'olderThan90d' | 'olderThan1y' | 'custom';

export type PredicateKind = 'rule' | 'archiveStrategy' | 'labelFilter';

// ---------------------------------------------------------------------------
// Value objects
// ---------------------------------------------------------------------------

export interface CleanupFolderOrLabel {
  id: string;
  name: string;
  kind: MoveKind;
}

// PlanAction — backend: #[serde(tag = "type", rename_all = "camelCase")]
export type PlanAction =
  | { type: 'archive' }
  | { type: 'addLabel'; kind: MoveKind }
  | { type: 'move'; kind: MoveKind }
  | { type: 'delete'; permanent: boolean }
  | { type: 'unsubscribe'; method: UnsubscribeMethodKind }
  | { type: 'markRead' }
  | { type: 'star'; on: boolean };

// PlanSource — backend: #[serde(tag = "type", rename_all = "camelCase")]
export type PlanSource =
  | { type: 'subscription'; sender: string }
  | { type: 'cluster'; clusterId: string; clusterAction: CleanupClusterAction }
  | { type: 'rule'; ruleId: string; matchBasis: string }
  | { type: 'archiveStrategy'; strategy: CleanupArchiveStrategy }
  | { type: 'manual' };

// AccountStateEtag — backend:
//   #[serde(tag = "kind",
//           rename_all = "camelCase",
//           rename_all_fields = "camelCase")]
// Verified against backend/src/cleanup/domain/operation.rs:
//   GmailHistory{historyId} → kind = "gmailHistory"
//   OutlookDelta{deltaToken} → kind = "outlookDelta"
//   ImapUvms{uidvalidity, highestModseq} → kind = "imapUvms"
//   None → kind = "none"
// (The Rust `kind_str()` helper returns snake_case for DB storage; the wire
//  JSON uses camelCase per the serde attributes above.)
export type AccountStateEtag =
  | { kind: 'gmailHistory'; historyId: string }
  | { kind: 'outlookDelta'; deltaToken: string }
  | { kind: 'imapUvms'; uidvalidity: number; highestModseq: number }
  | { kind: 'pop3Sentinel'; lastUidl: string }
  | { kind: 'none' };

// ReverseOp — backend: #[serde(tag = "type", rename_all = "camelCase")]
export type ReverseOp =
  | { type: 'addLabel'; kind: MoveKind; target: CleanupFolderOrLabel }
  | { type: 'removeLabel'; kind: MoveKind; target: CleanupFolderOrLabel }
  | { type: 'moveBack'; kind: MoveKind; target: CleanupFolderOrLabel }
  | { type: 'irreversible' };

export interface CleanupErrorCode {
  code: string;
  message: string;
}

// PlanWarning — backend: #[serde(tag = "type", rename_all = "camelCase")]
export type PlanWarning =
  | { type: 'largeGroup'; source: PlanSource; projectedCount: number }
  | { type: 'targetConflict'; accountId: string; emailId: string; sources: PlanSource[] }
  | { type: 'planExceedsThreshold'; totalCount: number }
  | { type: 'lowConfidence'; ruleId: string; reason: string };

// ---------------------------------------------------------------------------
// Operation rows
// ---------------------------------------------------------------------------

export interface PlannedOperationRow {
  seq: number;
  accountId: string;
  emailId: string | null;
  action: PlanAction;
  source: PlanSource;
  target: CleanupFolderOrLabel | null;
  reverseOp: ReverseOp | null;
  risk: RiskLevel;
  status: OperationStatus;
  skipReason?: SkipReason;
  appliedAt?: number;
  error?: CleanupErrorCode;
}

export interface PlannedOperationPredicate {
  seq: number;
  accountId: string;
  predicateKind: PredicateKind;
  predicateId: string;
  action: PlanAction;
  target: CleanupFolderOrLabel | null;
  source: PlanSource;
  projectedCount: number;
  sampleEmailIds: string[];
  risk: RiskLevel;
  status: PredicateStatus;
  partialAppliedCount: number;
  error?: CleanupErrorCode;
}

// PlannedOperation — backend: #[serde(tag = "opKind", rename_all = "camelCase")]
export type PlannedOperation =
  | ({ opKind: 'materialized' } & PlannedOperationRow)
  | ({ opKind: 'predicate' } & PlannedOperationPredicate);

// ---------------------------------------------------------------------------
// Wizard selections (input to POST /cleanup/plan)
// ---------------------------------------------------------------------------

export interface SubscriptionSelection {
  sender: string;
  accountId: string;
}

export interface ClusterSelectionInput {
  clusterId: string;
  action: CleanupClusterAction;
  accountId: string;
}

export interface RuleSelectionInput {
  ruleId: string;
  accountId: string;
}

export interface WizardSelections {
  subscriptions: SubscriptionSelection[];
  clusterActions: ClusterSelectionInput[];
  ruleSelections: RuleSelectionInput[];
  archiveStrategy: CleanupArchiveStrategy | null;
  accountIds: string[];
}

// ---------------------------------------------------------------------------
// Plan rollups
// ---------------------------------------------------------------------------

export interface PlanTotals {
  totalOperations: number;
  byAction: Record<string, number>;
  byAccount: Record<string, number>;
  bySource: Record<string, number>;
}

export interface RiskRollup {
  low: number;
  medium: number;
  high: number;
}

// ---------------------------------------------------------------------------
// Plan aggregate + summaries
// ---------------------------------------------------------------------------

export type PlanId = string; // UUID v7
export type JobId = string;

export interface CleanupPlanSummary {
  id: PlanId;
  createdAt: string;
  validUntil: string;
  status: PlanStatus;
  totals: PlanTotals;
  risk: RiskRollup;
  warningsCount: number;
}

export interface CleanupPlan {
  id: PlanId;
  userId: string;
  accountIds: string[];
  createdAt: string;
  validUntil: string;
  /** 64-char hex (blake3). */
  planHash: string;
  accountStateEtags: Record<string, AccountStateEtag>;
  /**
   * Phase D: per-account provider lookup populated by the backend PlanBuilder.
   * Optional for backwards-compat with cached plans built before the field
   * existed; consumers fall back to etag-kind heuristics when missing.
   */
  accountProviders?: Record<string, CleanupProvider>;
  status: PlanStatus;
  totals: PlanTotals;
  risk: RiskRollup;
  warnings: PlanWarning[];
  operations: PlannedOperation[];
}

// ---------------------------------------------------------------------------
// API response shapes
// ---------------------------------------------------------------------------

export interface CreatePlanResponse {
  planId: PlanId;
  validUntil: string;
  totals: PlanTotals;
  risk: RiskRollup;
  status: PlanStatus;
  warningsCount: number;
}

export interface ListOpsResponse {
  items: PlannedOperation[];
  nextCursor: number | null;
}

export interface SampleResponse {
  emailIds: string[];
}

export interface ListPlansResponse {
  items: CleanupPlanSummary[];
}

// ---------------------------------------------------------------------------
// Phase C: Apply orchestrator wire types
//
// These mirror backend/src/cleanup/orchestrator/sse.rs and
// backend/src/cleanup/api/apply.rs. All shapes use camelCase per
// `#[serde(rename_all = "camelCase")]`.
// ---------------------------------------------------------------------------

export type JobState = 'queued' | 'running' | 'finished' | 'cancelled' | 'failed';

export type PauseReason = 'hardDrift' | 'rateLimit' | 'authError';

/** Per-account paused/active runtime state, included in the `snapshot` event. */
export interface AccountSnapshotState {
  paused: boolean;
  pauseReason?: PauseReason;
}

/**
 * Aggregated counters for an in-flight or finished apply job. Mirrors backend
 * `JobCounts` (Phase C added `skippedByReason`). Backend omits the field when
 * empty, so it is optional on the wire.
 */
export interface ApplyJobCounts {
  applied: number;
  failed: number;
  skipped: number;
  pending: number;
  skippedByReason?: Partial<Record<SkipReason, number>>;
}

/** Body of POST /api/v1/cleanup/apply/:planId. */
export interface ApplyOptions {
  acknowledgedHighRiskSeqs: number[];
  acknowledgedMediumGroups: string[];
}

/** Response body of POST /api/v1/cleanup/apply/:planId (202 Accepted). */
export interface BeginApplyResponse {
  jobId: JobId;
}

/** GET /api/v1/cleanup/apply/:jobId — full apply-job aggregate. */
export interface CleanupApplyJob {
  jobId: JobId;
  planId: PlanId;
  startedAt: string;
  finishedAt: string | null;
  state: JobState;
  riskMax: RiskMax;
  counts: ApplyJobCounts;
}

/**
 * Server-sent events emitted on /api/v1/cleanup/apply/:jobId/stream.
 *
 * Variant tag is `type` and field names are camelCase. The stream always
 * begins with a `snapshot` event so clients reconnecting mid-job get a
 * coherent baseline without replaying historical OpApplied events.
 */
export type ApplyEvent =
  | {
      type: 'snapshot';
      jobId: JobId;
      planId: PlanId;
      counts: ApplyJobCounts;
      accountStates: Record<string, AccountSnapshotState>;
    }
  | {
      type: 'started';
      jobId: JobId;
      planId: PlanId;
      totalsByAccount: Record<string, ApplyJobCounts>;
    }
  | {
      type: 'opApplied';
      seq: number;
      accountId: string;
      /** camelCase serde tag of the row's PlanAction (e.g. 'archive', 'addLabel'). */
      actionType: string;
      appliedAt: number;
    }
  | {
      type: 'opFailed';
      seq: number;
      accountId: string;
      actionType: string;
      error: CleanupErrorCode;
    }
  | {
      type: 'opSkipped';
      seq: number;
      accountId: string;
      actionType: string;
      reason: SkipReason;
    }
  | { type: 'predicateExpanded'; predicateSeq: number; producedRows: number }
  | { type: 'accountPaused'; accountId: string; reason: PauseReason }
  | { type: 'accountResumed'; accountId: string }
  | { type: 'progress'; counts: ApplyJobCounts }
  | { type: 'finished'; jobId: JobId; status: JobState; counts: ApplyJobCounts };
