import type { EmailAccount, ImapConfig } from '@emailibrium/types';
import { api } from './client.js';

export async function connectGmail(): Promise<{ authUrl: string }> {
  return api.post('auth/gmail/connect').json<{ authUrl: string }>();
}

export async function connectOutlook(): Promise<{ authUrl: string }> {
  return api.post('auth/outlook/connect').json<{ authUrl: string }>();
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
