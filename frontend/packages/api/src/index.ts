export { api } from './client.js';
export { createSSEStream } from './sse.js';
export type { SSEStream } from './sse.js';

export { searchEmails, findSimilar, classifyEmail } from './searchApi.js';

export {
  getSubscriptions,
  getRecurringSenders,
  getInboxReport,
  getTemporalInsights,
  getTopicClusters,
} from './insightsApi.js';
export type { TopicCluster } from './insightsApi.js';

export {
  startIngestion,
  pauseIngestion,
  resumeIngestion,
  createIngestionStream,
  getEmbeddingStatus,
  triggerReembed,
} from './ingestionApi.js';
export type { EmbeddingStatus, ReembedMode, ReembedResponse } from './ingestionApi.js';

export { getHealth, getStats } from './vectorsApi.js';

export {
  connectGmail,
  connectOutlook,
  connectImap,
  getAccounts,
  disconnectAccount,
  updateAccount,
  removeAccountLabels,
  unarchiveAccount,
} from './authApi.js';

export {
  getEmails,
  getEmail,
  getThread,
  archiveEmail,
  starEmail,
  markEmailRead,
  deleteEmail,
  sendEmail,
  replyToEmail,
  forwardEmail,
  getCategories,
  getLabels,
  moveEmail,
  getAttachments,
  getAttachmentDownloadUrl,
  getAttachmentsZipUrl,
  getAllLabels,
  getEnrichedCategories,
  getEmailCounts,
  markAsSpam,
  unmarkSpam,
  restoreEmail,
  emptyTrash,
  permanentDelete,
} from './emailApi.js';
export type {
  GetEmailsParams,
  SendEmailDraft,
  FolderOrLabel,
  AggregatedLabel,
  EnrichedCategory,
  EmailCounts,
} from './emailApi.js';

export {
  getRules,
  getRule,
  createRule,
  updateRule,
  deleteRule,
  getRuleSuggestions,
  validateRule,
  testRule,
} from './rulesApi.js';
export type { RuleValidationResult, RuleTestResult } from './rulesApi.js';

export { bulkArchive, bulkDelete, bulkLabel, unsubscribe } from './actionsApi.js';

export { batchUnsubscribe, undoUnsubscribe, previewUnsubscribe } from './unsubscribeApi.js';

export {
  sendChatMessage,
  createChatStream,
  streamChatMessage,
  getChatSessions,
  deleteChatSession,
} from './chatApi.js';

export { recordConsent, getConsents, requestDataExport, requestDataErase } from './consentApi.js';

export { getClusters, getClusteringStatus, triggerRecluster } from './clusterApi.js';
export type { ClusteringStatus } from './clusterApi.js';

export { getAppConfig } from './configApi.js';
export type { AppConfig, AppCacheConfig, AppNetworkConfig } from './configApi.js';

export { submitFeedback } from './learningApi.js';
export type { FeedbackPayload, FeedbackAction } from './learningApi.js';
