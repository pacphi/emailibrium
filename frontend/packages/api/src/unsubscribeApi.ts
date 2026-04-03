import type {
  UnsubscribeRequest,
  UnsubscribeTarget,
  UnsubscribeResult,
  UnsubscribePreview,
} from '@emailibrium/types';
import { api } from './client.js';

export async function batchUnsubscribe(request: UnsubscribeRequest): Promise<UnsubscribeResult> {
  return api.post('unsubscribe', { json: request }).json<UnsubscribeResult>();
}

export async function undoUnsubscribe(batchId: string): Promise<{ status: string }> {
  return api.post(`unsubscribe/undo/${batchId}`).json<{ status: string }>();
}

export async function previewUnsubscribe(
  targets: UnsubscribeTarget[],
): Promise<UnsubscribePreview[]> {
  const resp = await api
    .post('unsubscribe/preview', {
      json: { subscriptions: targets, engagement_rates: {} },
    })
    .json<{ previews: UnsubscribePreview[]; total: number }>();
  return resp.previews;
}
