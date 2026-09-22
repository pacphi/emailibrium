import { test as base, expect, type Request } from '@playwright/test';
import type {
  Email,
  EmailAccount,
  CleanupPlan,
  Rule,
  SubscriptionInsight,
} from '@emailibrium/types';

const date = '2026-09-20T12:00:00Z';
export const account: EmailAccount = {
  id: 'fixture-account',
  provider: 'gmail',
  emailAddress: 'alex@example.test',
  displayName: 'Alex Fixture',
  archiveStrategy: 'manual',
  syncDepth: 'all',
  labelPrefix: 'Emailibrium',
  syncFrequency: 5,
  isActive: true,
  emailCount: 2,
};
export const messages: Email[] = [
  {
    id: 'mail-1',
    accountId: account.id,
    provider: 'gmail',
    threadId: 'thread-1',
    subject: 'Project Atlas review',
    fromAddr: 'sam@example.test',
    fromName: 'Sam Fixture',
    toAddrs: account.emailAddress,
    receivedAt: date,
    bodyText: 'Please review the Atlas milestone by Friday.',
    isRead: false,
    isStarred: false,
    hasAttachments: false,
    embeddingStatus: 'embedded',
    category: 'Work',
  },
  {
    id: 'mail-2',
    accountId: account.id,
    provider: 'gmail',
    threadId: 'thread-2',
    subject: 'Weekly science digest',
    fromAddr: 'digest@example.test',
    fromName: 'Science Digest',
    toAddrs: account.emailAddress,
    receivedAt: date,
    bodyText: 'This week in science: a new telescope.',
    isRead: true,
    isStarred: false,
    hasAttachments: false,
    embeddingStatus: 'embedded',
    category: 'Newsletter',
  },
];
export const subscription: SubscriptionInsight = {
  senderAddress: 'digest@example.test',
  senderDomain: 'example.test',
  frequency: 'weekly',
  emailCount: 12,
  firstSeen: date,
  lastSeen: date,
  hasUnsubscribe: true,
  category: 'newsletter',
  suggestedAction: 'unsubscribe',
  readRate: 0,
};
const totals = {
  totalOperations: 1,
  byAction: { archive: 1 },
  byAccount: { [account.id]: 1 },
  bySource: { manual: 1 },
};
export const plan: CleanupPlan = {
  id: 'fixture-plan',
  userId: account.emailAddress,
  accountIds: [account.id],
  createdAt: date,
  validUntil: '2099-01-01T00:00:00Z',
  planHash: '0'.repeat(64),
  accountStateEtags: { [account.id]: { kind: 'gmailHistory', historyId: '1' } },
  accountProviders: { [account.id]: 'gmail' },
  status: 'ready',
  totals,
  risk: { low: 1, medium: 0, high: 0 },
  warnings: [],
  operations: [
    {
      opKind: 'materialized',
      seq: 1,
      accountId: account.id,
      emailId: 'mail-2',
      action: { type: 'archive' },
      source: { type: 'manual' },
      target: null,
      reverseOp: null,
      risk: 'low',
      status: 'pending',
    },
  ],
};
const stats = {
  totalVectors: 2,
  collections: { emails: 2 },
  dimensions: 384,
  memoryBytes: 1024,
  indexType: 'hnsw',
};
const appConfig = {
  cache: Object.fromEntries(
    [
      'defaultStaleTimeMs',
      'clustersStaleTimeMs',
      'clustersRefetchIntervalMs',
      'clustersActiveStaleTimeMs',
      'clustersActiveRefetchIntervalMs',
      'clusteringStatusStaleTimeMs',
      'clusteringStatusRefetchIntervalMs',
      'dashboardAccountsRefetchIntervalMs',
      'dashboardEmbeddingRefetchIntervalMs',
      'embeddingActiveRefetchIntervalMs',
      'ingestionActiveRefetchIntervalMs',
      'ingestionActiveStaleTimeMs',
      'statsRefetchIntervalMs',
      'statsActiveRefetchIntervalMs',
    ].map((key) => [key, 60000]),
  ),
  network: { ingestionStartTimeoutMs: 3000, reclusterTimeoutMs: 3000, reembedTimeoutMs: 3000 },
  rules: { suggestionsPageSize: 10, suggestionsMinEmailCount: 1 },
};
appConfig.cache.defaultRetryCount = 0;

export class Mailbox {
  accounts: EmailAccount[] = [structuredClone(account)];
  emails = structuredClone(messages);
  rules: Rule[] = [];
  requests: Request[] = [];
  unexpected: string[] = [];
  failures = new Map<string, number>();
  settings: Record<string, unknown> = { llmProvider: 'builtin' };
  chatEvents: object[] = [
    { type: 'token', content: 'Atlas needs a review by Friday.' },
    { type: 'done', sessionId: 'fixture-session' },
  ];
  cleanupPlan = structuredClone(plan);
  applyEvents: object[] = [
    {
      type: 'snapshot',
      jobId: 'fixture-job',
      counts: { applied: 0, failed: 0, skipped: 0, pending: 1 },
      accountStates: {},
    },
    { type: 'opApplied', seq: 1, accountId: account.id, actionType: 'archive' },
    {
      type: 'finished',
      status: 'completed',
      counts: { applied: 1, failed: 0, skipped: 0, pending: 0 },
    },
  ];
  matching(method: string, path: string) {
    return this.requests.filter(
      (r) => r.method() === method && new URL(r.url()).pathname === `/api/v1/${path}`,
    );
  }
  response(method: string, path: string, request: Request): unknown {
    if (path === 'auth/accounts' && method === 'GET') return this.accounts;
    if (path === 'auth/imap/connect' && method === 'POST') {
      const connected = {
        ...account,
        provider: 'imap' as const,
        emailAddress: request.postDataJSON().email,
      };
      this.accounts = [connected];
      return connected;
    }
    if (path.startsWith('auth/accounts/') && method === 'PATCH') {
      Object.assign(this.accounts[0]!, request.postDataJSON());
      return {};
    }
    if (path === 'ai/settings') {
      if (method === 'PUT') this.settings = request.postDataJSON();
      return Object.fromEntries(Object.entries(this.settings).map(([k, v]) => [k, String(v)]));
    }
    if (path === 'ai/health')
      return { available: true, provider: 'builtin', model: 'fixture-model' };
    if (path === 'ai/config/app') return appConfig;
    if (path === 'vectors/health')
      return { status: 'ok', storeHealthy: true, embeddingAvailable: true, storeStats: stats };
    if (path === 'vectors/stats') return stats;
    if (path === 'vectors/search/hybrid') {
      const query = request.postDataJSON().text ?? request.postDataJSON().query ?? '';
      const results = /atlas/i.test(query)
        ? [
            {
              emailId: 'mail-1',
              score: 0.97,
              matchType: 'hybrid',
              metadata: { subject: messages[0]!.subject, from: messages[0]!.fromAddr },
            },
          ]
        : [];
      return { results, total: results.length, latencyMs: 3 };
    }
    if (path === 'emails' && method === 'GET') {
      const url = new URL(request.url());
      const emails = this.emails.filter(
        (e) => !url.searchParams.has('category') || e.category === url.searchParams.get('category'),
      );
      return { emails, total: emails.length };
    }
    if (path === 'emails/counts')
      return {
        total: 2,
        unread: 1,
        archivedCount: 0,
        spam_count: 0,
        trash_count: 0,
        sent_count: 0,
        byCategory: [],
      };
    if (path === 'emails/categories') return { categories: ['Work', 'Newsletter'] };
    if (path === 'emails/categories/enriched')
      return [{ name: 'Work', group: 'Work', emailCount: 1, unreadCount: 1 }];
    if (path === 'emails/labels/all' || path === 'emails/labels') return [];
    if (path === 'emails/send') return { messageId: 'fixture-sent' };
    for (const email of this.emails) {
      if (path === `emails/${email.id}`) return email;
      if (path === `emails/thread/${email.threadId}` || path === `emails/thread/${email.id}`)
        return {
          threadId: email.threadId,
          emails: [email],
          subject: email.subject,
          participants: [email.fromAddr],
          lastActivity: date,
        };
      if (path === `emails/${email.id}/attachments`) return [];
      if (path === `emails/${email.id}/reply`) return { messageId: 'fixture-reply' };
      if (path === `emails/${email.id}/read`) {
        email.isRead = request.postDataJSON().read;
        return {};
      }
      if (path === `emails/${email.id}/star`) {
        email.isStarred = !email.isStarred;
        return {};
      }
      if (path === `emails/${email.id}/archive`) {
        this.emails = this.emails.filter((e) => e.id !== email.id);
        return {};
      }
    }
    if (path === 'ingestion/progress')
      return { active: false, phase: null, total: 2, processed: 2 };
    if (path === 'ingestion/backfill-progress')
      return { active: false, total: 2, categorized: 2, failed: 0 };
    if (path === 'ingestion/embedding-status')
      return {
        totalEmails: 2,
        embeddingStatusSummary: {
          embeddedCount: 2,
          pendingCount: 0,
          failedCount: 0,
          staleCount: 0,
        },
      };
    if (path === 'ingestion/lock-status') return null;
    if (path === 'ingestion/start') return { job_id: 'fixture-ingestion' };
    if (path === 'clustering/clusters') return { clusters: [], total: 0 };
    if (path === 'clustering/status')
      return {
        clusterCount: 0,
        totalClusteredEmails: 0,
        isClustering: false,
        isIngesting: false,
        phase: null,
      };
    if (path === 'insights/subscriptions' || path === 'insights/recurring-senders')
      return [subscription];
    if (path === 'insights/topics') return [];
    if (path === 'insights/report')
      return {
        totalEmails: 2,
        categoryBreakdown: { Work: 1, Newsletter: 1 },
        topSenders: [{ sender: 'Sam Fixture', count: 1 }],
        subscriptionCount: 1,
        estimatedReadingHours: 1,
        readRate: 0.5,
      };
    if (path === 'insights/temporal')
      return {
        dailyVolume: [{ date: '2026-09-20', count: 2 }],
        categoryDaily: [],
        dayOfWeek: [],
        hourOfDay: [],
      };
    if (path === 'rules' && method === 'GET') return this.rules;
    if (path === 'rules' && method === 'POST') {
      const rule = {
        ...request.postDataJSON(),
        id: 'fixture-rule',
        matchCount: 0,
        createdAt: date,
      };
      this.rules.push(rule);
      return rule;
    }
    if (path === 'rules/validate') return { valid: true, errors: [], warnings: [] };
    if (path === 'rules/test')
      return {
        matchCount: 1,
        sampleMatches: [
          { id: 'mail-1', subject: messages[0]!.subject, from: messages[0]!.fromAddr },
        ],
      };
    if (path === 'rules/suggestions') return [];
    if (path === 'ai/chat/sessions') return [];
    if (path === 'ai/chat/confirm') return {};
    if (path === 'ai/model-catalog')
      return [
        {
          id: 'qwen3-1.7b-q4km',
          name: 'Fixture Small Model',
          params: '1.7B',
          contextSize: 2048,
          diskMb: 1000,
          quality: 'test',
          recommended: true,
          cached: true,
          toolCalling: true,
        },
      ];
    if (path === 'ai/embedding-catalog')
      return [
        {
          id: 'all-MiniLM-L6-v2',
          name: 'MiniLM',
          dimensions: 384,
          provider: 'onnx',
          description: 'Synthetic local model',
          downloadRequired: false,
        },
      ];
    if (path.startsWith('ai/model-status/'))
      return { status: 'ready', downloaded: true, progress: 100 };
    if (path === 'consent/gdpr') return { decisions: [] };
    if (path === 'cleanup/plan' && method === 'POST')
      return {
        planId: this.cleanupPlan.id,
        totals: this.cleanupPlan.totals,
        risk: this.cleanupPlan.risk,
        warnings: [],
      };
    if (path === 'cleanup/plan/fixture-plan') return this.cleanupPlan;
    if (path === 'cleanup/plan/fixture-plan/operations')
      return { items: this.cleanupPlan.operations, nextCursor: null };
    if (path === 'cleanup/plan/fixture-plan/audit') return { items: [] };
    if (path === 'cleanup/plans') return { items: [this.cleanupPlan], nextCursor: null };
    if (path === 'cleanup/apply/fixture-plan') return { jobId: 'fixture-job' };
    if (path === 'cleanup/telemetry') return {};
    this.unexpected.push(`${method} ${path}`);
    return { error: `Unconfigured synthetic endpoint: ${method} ${path}` };
  }
}

export const test = base.extend<{ mailbox: Mailbox }>({
  mailbox: [
    async ({ context }, use) => {
      const mailbox = new Mailbox();
      const pageErrors: string[] = [];
      context.on('page', (page) => page.on('pageerror', (error) => pageErrors.push(error.message)));
      await context.route('**/*', async (route) => {
        const request = route.request();
        const url = new URL(request.url());
        if (url.origin !== 'http://127.0.0.1:4173') {
          mailbox.unexpected.push(`External request blocked: ${url.origin}${url.pathname}`);
          await route.abort();
          return;
        }
        if (!url.pathname.startsWith('/api/')) {
          await route.continue();
          return;
        }
        mailbox.requests.push(request);
        const path = url.pathname.replace('/api/v1/', '');
        const failure = mailbox.failures.get(path);
        if (failure) {
          await route.fulfill({
            status: failure,
            json: { error: 'Synthetic service unavailable' },
          });
          return;
        }
        const events =
          path === 'ai/chat/stream'
            ? mailbox.chatEvents
            : path === 'cleanup/apply/fixture-job/stream'
              ? mailbox.applyEvents
              : path === 'ingestion/status'
                ? [
                    {
                      jobId: 'fixture-ingestion',
                      phase: 'complete',
                      total: 2,
                      processed: 2,
                      embedded: 2,
                      categorized: 2,
                      failed: 0,
                      etaSeconds: null,
                      emailsPerSecond: 2,
                    },
                  ]
                : null;
        if (events) {
          await route.fulfill({
            status: 200,
            contentType: 'text/event-stream',
            body: events.map((event) => `data: ${JSON.stringify(event)}\n\n`).join(''),
          });
          return;
        }
        const response = mailbox.response(request.method(), path, request);
        await route.fulfill({
          status: mailbox.unexpected.includes(`${request.method()} ${path}`) ? 501 : 200,
          json: response,
        });
      });
      await use(mailbox);
      expect(
        mailbox.unexpected,
        'Every backend and external request must have an explicit fixture',
      ).toEqual([]);
      expect(pageErrors, 'No uncaught browser errors').toEqual([]);
    },
    { auto: true },
  ],
});
export { expect };
