export type EmbeddingStatus = 'pending' | 'embedded' | 'failed' | 'stale';

export interface Email {
  id: string;
  accountId: string;
  provider: string;
  messageId?: string;
  threadId?: string;
  subject: string;
  fromAddr: string;
  fromName?: string;
  toAddrs: string;
  ccAddrs?: string;
  receivedAt: string;
  bodyText?: string;
  bodyHtml?: string;
  labels?: string;
  isRead: boolean;
  isStarred: boolean;
  hasAttachments: boolean;
  embeddingStatus: EmbeddingStatus;
  category: string;
  categoryConfidence?: number;
}

export interface EmailThread {
  threadId: string;
  emails: Email[];
  subject: string;
  participants: string[];
  lastActivity: string;
}
