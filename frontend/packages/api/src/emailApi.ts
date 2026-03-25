import type { Email, EmailThread } from '@emailibrium/types';
import { api } from './client.js';

export interface GetEmailsParams {
  accountId?: string;
  category?: string;
  isRead?: boolean;
  limit?: number;
  offset?: number;
}

export interface SendEmailDraft {
  to: string;
  cc?: string;
  bcc?: string;
  subject: string;
  bodyText?: string;
  bodyHtml?: string;
  accountId: string;
}

export async function getEmails(
  params?: GetEmailsParams,
): Promise<{ emails: Email[]; total: number }> {
  return api
    .get('emails', { searchParams: params as Record<string, string | number | boolean> })
    .json<{ emails: Email[]; total: number }>();
}

export async function getEmail(id: string): Promise<Email> {
  return api.get(`emails/${id}`).json<Email>();
}

export async function getThread(threadId: string): Promise<EmailThread> {
  return api.get(`emails/thread/${threadId}`).json<EmailThread>();
}

export async function archiveEmail(id: string): Promise<void> {
  await api.post(`emails/${id}/archive`);
}

export async function starEmail(id: string): Promise<void> {
  await api.post(`emails/${id}/star`);
}

export async function deleteEmail(id: string): Promise<void> {
  await api.delete(`emails/${id}`);
}

export async function sendEmail(draft: SendEmailDraft): Promise<{ messageId: string }> {
  return api.post('emails/send', { json: draft }).json<{ messageId: string }>();
}

export async function replyToEmail(
  id: string,
  body: { bodyText?: string; bodyHtml?: string },
): Promise<{ messageId: string }> {
  return api.post(`emails/${id}/reply`, { json: body }).json<{ messageId: string }>();
}

export async function forwardEmail(id: string, to: string): Promise<{ messageId: string }> {
  return api.post(`emails/${id}/forward`, { json: { to } }).json<{ messageId: string }>();
}

export interface FolderOrLabel {
  id: string;
  name: string;
  kind: 'folder' | 'label';
  isSystem: boolean;
}

export async function getLabels(accountId: string): Promise<FolderOrLabel[]> {
  return api.get('emails/labels', { searchParams: { accountId } }).json<FolderOrLabel[]>();
}

export async function moveEmail(
  id: string,
  body: { accountId: string; targetId: string; kind: 'folder' | 'label' },
): Promise<void> {
  await api.post(`emails/${id}/move`, { json: body });
}
