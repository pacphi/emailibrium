import { api } from './client.js';

export async function bulkArchive(emailIds: string[]): Promise<{ count: number }> {
  return api
    .post('actions/bulk-archive', { json: { emailIds } })
    .json<{ count: number }>();
}

export async function bulkDelete(emailIds: string[]): Promise<{ count: number }> {
  return api
    .post('actions/bulk-delete', { json: { emailIds } })
    .json<{ count: number }>();
}

export async function bulkLabel(
  emailIds: string[],
  label: string,
): Promise<{ count: number }> {
  return api
    .post('actions/bulk-label', { json: { emailIds, label } })
    .json<{ count: number }>();
}

export async function unsubscribe(subscriptionId: string): Promise<void> {
  await api.post(`actions/unsubscribe/${subscriptionId}`);
}
