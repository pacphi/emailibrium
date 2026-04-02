import type { Email, EmailThread, Attachment } from '@emailibrium/types';
import { api } from './client.js';

export interface GetEmailsParams {
  accountId?: string;
  category?: string;
  label?: string;
  isRead?: boolean;
  isStarred?: boolean;
  isSpam?: boolean;
  isTrash?: boolean;
  folder?: string;
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

export async function markEmailRead(id: string, read: boolean): Promise<void> {
  await api.post(`emails/${id}/read`, { json: { read } });
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

export async function getCategories(): Promise<{ categories: string[] }> {
  return api.get('emails/categories').json<{ categories: string[] }>();
}

export async function getLabels(accountId: string): Promise<FolderOrLabel[]> {
  return api.get('emails/labels', { searchParams: { accountId } }).json<FolderOrLabel[]>();
}

// --- Gap 4: Cross-account label aggregation ---

export interface AggregatedLabel {
  name: string;
  kind: string;
  isSystem: boolean;
  emailCount: number;
  unreadCount: number;
  accountIds: string[];
}

export async function getAllLabels(): Promise<AggregatedLabel[]> {
  return api.get('emails/labels/all').json<AggregatedLabel[]>();
}

// --- Gap 5: Enriched categories ---

export interface EnrichedCategory {
  name: string;
  group: string;
  emailCount: number;
  unreadCount: number;
}

export async function getEnrichedCategories(): Promise<EnrichedCategory[]> {
  return api.get('emails/categories/enriched').json<EnrichedCategory[]>();
}

// --- Gap 6: Accurate email counts ---

export interface EmailCounts {
  total: number;
  unread: number;
  spam_count: number;
  trash_count: number;
  sent_count: number;
  byCategory: Array<{ category: string; total: number; unread: number }>;
}

export async function getEmailCounts(): Promise<EmailCounts> {
  return api.get('emails/counts').json<EmailCounts>();
}

export async function moveEmail(
  id: string,
  body: { accountId: string; targetId: string; kind: 'folder' | 'label' },
): Promise<void> {
  await api.post(`emails/${id}/move`, { json: body });
}

export async function markAsSpam(id: string): Promise<void> {
  await api.post(`emails/${id}/spam`);
}

export async function unmarkSpam(id: string): Promise<void> {
  await api.post(`emails/${id}/unspam`);
}

export async function restoreEmail(id: string): Promise<void> {
  await api.post(`emails/${id}/restore`);
}

export async function emptyTrash(): Promise<void> {
  await api.delete('emails/trash');
}

export async function permanentDelete(id: string): Promise<void> {
  await api.delete(`emails/${id}`, { searchParams: { permanent: 'true' } });
}

export async function getAttachments(emailId: string): Promise<Attachment[]> {
  return api.get(`emails/${emailId}/attachments`).json<Attachment[]>();
}

export function getAttachmentDownloadUrl(emailId: string, attachmentId: string): string {
  return `/api/v1/emails/${emailId}/attachments/${attachmentId}`;
}

export function getAttachmentsZipUrl(emailId: string): string {
  return `/api/v1/emails/${emailId}/attachments/zip`;
}
