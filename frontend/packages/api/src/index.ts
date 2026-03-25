export { api } from './client.js';
export { createSSEStream } from './sse.js';
export type { SSEStream } from './sse.js';

export { searchEmails, findSimilar, classifyEmail } from './searchApi.js';

export { getSubscriptions, getRecurringSenders, getInboxReport } from './insightsApi.js';

export {
  startIngestion,
  pauseIngestion,
  resumeIngestion,
  createIngestionStream,
} from './ingestionApi.js';

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
  deleteEmail,
  sendEmail,
  replyToEmail,
  forwardEmail,
  getLabels,
  moveEmail,
} from './emailApi.js';
export type { GetEmailsParams, SendEmailDraft, FolderOrLabel } from './emailApi.js';

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

export { submitFeedback } from './learningApi.js';
export type { FeedbackPayload, FeedbackAction } from './learningApi.js';
