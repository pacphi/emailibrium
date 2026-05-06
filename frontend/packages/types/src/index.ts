export type { Attachment, Email, EmailThread, EmbeddingStatus } from './email.js';

export type {
  SearchMode,
  SearchQuery,
  SearchFilters,
  SearchResult,
  SearchResponse,
} from './search.js';

export type {
  Cluster,
  ClusterTerm,
  RepresentativeEmail,
  VectorStats,
  HealthStatus,
} from './vectors.js';

export type {
  RecurrencePattern,
  SubscriptionCategory,
  SuggestedAction,
  SubscriptionInsight,
  InboxReport,
  DailyCount,
  CategoryDailyCount,
  DayOfWeekCount,
  HourOfDayCount,
  TemporalInsights,
} from './insights.js';

export type {
  IngestionPhase,
  IngestionProgress,
  PipelineSource,
  PipelineActivity,
  PipelineBusyResponse,
} from './ingestion.js';

export type {
  Provider,
  ArchiveStrategy,
  EmailAccount,
  OAuthCallbackParams,
  ImapConfig,
} from './auth.js';

export type { Rule, RuleCondition, RuleAction, RuleSuggestion } from './rules.js';

export type {
  ConsentPurpose,
  GdprConsent,
  ConsentRecord,
  DataExportRequest,
  DataExportResponse,
  DataEraseResponse,
} from './consent.js';

export type {
  ChatSession,
  ChatRequest,
  ChatResponse,
  ChatStreamChunk,
  ToolCallEvent,
  ToolResultEvent,
  ConfirmationEvent,
  ChatStreamEventType,
  ChatStreamEvent,
} from './chat.js';

export type {
  UnsubscribeTarget,
  UnsubscribeRequest,
  UnsubscribeResult,
  UnsubscribePreview,
} from './unsubscribe.js';

export type {
  RiskLevel,
  RiskMax,
  PlanStatus,
  OperationStatus,
  PredicateStatus,
  SkipReason,
  CleanupProvider,
  MoveKind,
  UnsubscribeMethodKind,
  CleanupClusterAction,
  CleanupArchiveStrategy,
  PredicateKind,
  CleanupFolderOrLabel,
  PlanAction,
  PlanSource,
  AccountStateEtag,
  ReverseOp,
  CleanupErrorCode,
  PlanWarning,
  PlannedOperationRow,
  PlannedOperationPredicate,
  PlannedOperation,
  SubscriptionSelection,
  ClusterSelectionInput,
  RuleSelectionInput,
  WizardSelections,
  PlanTotals,
  RiskRollup,
  PlanId,
  JobId,
  CleanupPlanSummary,
  CleanupPlan,
  CreatePlanResponse,
  ListOpsResponse,
  SampleResponse,
  ListPlansResponse,
} from './cleanup.js';
