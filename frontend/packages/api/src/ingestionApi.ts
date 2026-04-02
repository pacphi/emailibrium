import type { IngestionProgress } from '@emailibrium/types';
import type { SSEStream } from './sse.js';
import { api } from './client.js';
import { createSSEStream } from './sse.js';

export async function startIngestion(accountId: string): Promise<{ jobId: string }> {
  return api
    .post('ingestion/start', {
      json: { account_id: accountId },
      timeout: 300_000, // 5 minutes — sync fetches all emails from provider
    })
    .json<{ jobId: string }>();
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
