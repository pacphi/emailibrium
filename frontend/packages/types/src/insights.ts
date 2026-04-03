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
  readRate?: number;
  /** RFC 2369 List-Unsubscribe header (if captured from email headers). */
  listUnsubscribe?: string;
  /** RFC 8058 List-Unsubscribe-Post header (if captured from email headers). */
  listUnsubscribePost?: string;
}

export interface InboxReport {
  totalEmails: number;
  categoryBreakdown: Record<string, number>;
  topSenders: Array<{ sender: string; count: number }>;
  subscriptionCount: number;
  estimatedReadingHours: number;
  readRate?: number;
}

export interface DailyCount {
  date: string;
  count: number;
}

export interface CategoryDailyCount {
  date: string;
  category: string;
  count: number;
}

export interface DayOfWeekCount {
  day: number;
  count: number;
}

export interface HourOfDayCount {
  hour: number;
  count: number;
}

export interface TemporalInsights {
  dailyVolume: DailyCount[];
  categoryDaily: CategoryDailyCount[];
  dayOfWeek: DayOfWeekCount[];
  hourOfDay: HourOfDayCount[];
}
