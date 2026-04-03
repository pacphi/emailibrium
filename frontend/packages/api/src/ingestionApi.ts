import type {
  IngestionProgress,
  PipelineActivity,
  PipelineBusyResponse,
  PipelineSource,
} from '@emailibrium/types';
import type { SSEStream } from './sse.js';
import { api } from './client.js';
import { createSSEStream } from './sse.js';

/** Thrown when a pipeline is already running for the requested account (HTTP 409). */
export class PipelineBusyError extends Error {
  public readonly activity: PipelineBusyResponse;

  constructor(body: PipelineBusyResponse) {
    super(body.message);
    this.name = 'PipelineBusyError';
    this.activity = body;
  }
}

export async function startIngestion(
  accountId: string,
  source: PipelineSource = 'manual_sync',
): Promise<{ jobId: string }> {
  const resp = await api.post('ingestion/start', {
    json: { account_id: accountId, source },
    timeout: 300_000, // 5 minutes — sync fetches all emails from provider
    throwHttpErrors: false,
  });

  if (resp.status === 409) {
    const body = (await resp.json()) as PipelineBusyResponse;
    throw new PipelineBusyError(body);
  }
  if (!resp.ok) {
    const text = await resp.text().catch(() => resp.statusText);
    throw new Error(`Ingestion start failed (${resp.status}): ${text}`);
  }
  return resp.json<{ jobId: string }>();
}

/** Check if a pipeline is currently active for the given account. */
export async function getPipelineLockStatus(accountId: string): Promise<PipelineActivity | null> {
  return api
    .get('ingestion/lock-status', { searchParams: { account_id: accountId } })
    .json<PipelineActivity | null>();
}

export async function pauseIngestion(jobId: string): Promise<void> {
  await api.post('ingestion/pause', { json: { job_id: jobId } });
}

export async function resumeIngestion(jobId: string): Promise<void> {
  await api.post('ingestion/resume', { json: { job_id: jobId } });
}

export function createIngestionStream(jobId: string): SSEStream<IngestionProgress> {
  return createSSEStream<IngestionProgress>(`/api/v1/ingestion/${jobId}/stream`);
}

export interface EmbeddingStatus {
  totalEmails: number;
  embeddingStatusSummary: {
    embeddedCount: number;
    pendingCount: number;
    failedCount: number;
    staleCount: number;
  };
}

export async function getEmbeddingStatus(): Promise<EmbeddingStatus> {
  return api.get('ingestion/embedding-status').json<EmbeddingStatus>();
}

export interface IngestionProgressResponse {
  active: boolean;
  jobId?: string;
  phase: string | null;
  total?: number;
  processed?: number;
  embedded?: number;
  categorized?: number;
  failed?: number;
  etaSeconds?: number | null;
  emailsPerSecond?: number;
}

export async function getIngestionProgress(): Promise<IngestionProgressResponse> {
  return api.get('ingestion/progress').json<IngestionProgressResponse>();
}

export type ReembedMode = 'all' | 'failed' | 'stale';

export interface ReembedResponse {
  emailsReset: number;
  mode: string;
  message: string;
  ingestionTriggered: boolean;
}

export async function triggerReembed(
  mode: ReembedMode = 'all',
  timeoutMs?: number,
): Promise<ReembedResponse> {
  return api
    .post('ai/reembed', {
      json: { mode },
      timeout: timeoutMs ?? 60_000,
    })
    .json<ReembedResponse>();
}
