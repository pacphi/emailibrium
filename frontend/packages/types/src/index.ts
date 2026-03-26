export type { Attachment, Email, EmailThread, EmbeddingStatus } from './email.js';

export type {
  SearchMode,
  SearchQuery,
  SearchFilters,
  SearchResult,
  SearchResponse,
} from './search.js';

export type { Cluster, VectorStats, HealthStatus } from './vectors.js';

export type {
  RecurrencePattern,
  SubscriptionCategory,
  SuggestedAction,
  SubscriptionInsight,
  InboxReport,
} from './insights.js';

export type { IngestionPhase, IngestionProgress } from './ingestion.js';

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

export type { ChatSession, ChatRequest, ChatResponse, ChatStreamChunk } from './chat.js';

export type { UnsubscribeRequest, UnsubscribeResult, UnsubscribePreview } from './unsubscribe.js';
