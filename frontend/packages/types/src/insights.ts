export type RecurrencePattern =
  | 'daily'
  | 'weekly'
  | 'biweekly'
  | 'monthly'
  | 'quarterly'
  | 'irregular';

export type SubscriptionCategory =
  | 'newsletter'
  | 'marketing'
  | 'notification'
  | 'receipt'
  | 'social'
  | 'unknown';

export type SuggestedAction = 'keep' | 'unsubscribe' | 'archive' | 'digest';

export interface SubscriptionInsight {
  senderAddress: string;
  senderDomain: string;
  frequency: RecurrencePattern;
  emailCount: number;
  firstSeen: string;
  lastSeen: string;
  hasUnsubscribe: boolean;
  category: SubscriptionCategory;
  suggestedAction: SuggestedAction;
}

export interface InboxReport {
  totalEmails: number;
  categoryBreakdown: Record<string, number>;
  topSenders: Array<{ sender: string; count: number }>;
  subscriptionCount: number;
  estimatedReadingHours: number;
}
