import type { DeviceCodePoll, DeviceCodeStart, EmailAccount, ImapConfig } from '@emailibrium/types';
import { api } from './client.js';

export async function connectGmail(): Promise<{ authUrl: string }> {
  return api.post('auth/gmail/connect').json<{ authUrl: string }>();
}

export async function connectOutlook(): Promise<{ authUrl: string }> {
  return api.post('auth/outlook/connect').json<{ authUrl: string }>();
}

/** Start an Outlook sign-in with a code typed at Microsoft's device login page. */
export async function startOutlookDeviceCode(email: string): Promise<DeviceCodeStart> {
  return api.post('auth/outlook/device-code', { json: { email } }).json<DeviceCodeStart>();
}

/** Poll an Outlook device-code sign-in once. */
export async function pollOutlookDeviceCode(flowId: string): Promise<DeviceCodePoll> {
  return api
    .post(`auth/outlook/device-code/${encodeURIComponent(flowId)}/poll`)
    .json<DeviceCodePoll>();
}

export async function connectImap(config: ImapConfig): Promise<EmailAccount> {
  return api.post('auth/imap/connect', { json: config }).json<EmailAccount>();
}

export async function getAccounts(): Promise<EmailAccount[]> {
  return api.get('auth/accounts').json<EmailAccount[]>();
}

export async function disconnectAccount(id: string): Promise<void> {
  await api.delete(`auth/accounts/${id}`);
}

export async function updateAccount(id: string, changes: Record<string, unknown>): Promise<void> {
  await api.patch(`auth/accounts/${id}`, { json: changes });
}

export async function removeAccountLabels(
  id: string,
): Promise<{ messagesProcessed: number; labelsDeleted: number }> {
  return api.post(`auth/accounts/${id}/remove-labels`).json();
}

export async function unarchiveAccount(id: string): Promise<{ messagesProcessed: number }> {
  return api.post(`auth/accounts/${id}/unarchive`).json();
}
