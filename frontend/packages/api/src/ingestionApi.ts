import type { IngestionProgress } from '@emailibrium/types';
import type { SSEStream } from './sse.js';
import { api } from './client.js';
import { createSSEStream } from './sse.js';

export async function startIngestion(accountId: string): Promise<{ jobId: string }> {
  return api.post(`ingestion/${accountId}/start`).json<{ jobId: string }>();
}

export async function pauseIngestion(jobId: string): Promise<void> {
  await api.post(`ingestion/${jobId}/pause`);
}

export async function resumeIngestion(jobId: string): Promise<void> {
  await api.post(`ingestion/${jobId}/resume`);
}

export function createIngestionStream(jobId: string): SSEStream<IngestionProgress> {
  return createSSEStream<IngestionProgress>(`/api/v1/ingestion/${jobId}/stream`);
}
