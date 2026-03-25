import type {
  UnsubscribeRequest,
  UnsubscribeResult,
  UnsubscribePreview,
} from '@emailibrium/types';
import { api } from './client.js';

export async function batchUnsubscribe(
  request: UnsubscribeRequest,
): Promise<UnsubscribeResult> {
  return api
    .post('unsubscribe', { json: request })
    .json<UnsubscribeResult>();
}

export async function undoUnsubscribe(batchId: string): Promise<{ restored: number }> {
  return api
    .post(`unsubscribe/undo/${batchId}`)
    .json<{ restored: number }>();
}

export async function previewUnsubscribe(
  subscriptionIds: string[],
): Promise<UnsubscribePreview[]> {
  return api
    .post('unsubscribe/preview', { json: { subscriptionIds } })
    .json<UnsubscribePreview[]>();
}
