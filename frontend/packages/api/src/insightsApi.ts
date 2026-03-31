import type { SubscriptionInsight, InboxReport, TemporalInsights } from '@emailibrium/types';
import { api } from './client.js';

export async function getSubscriptions(): Promise<SubscriptionInsight[]> {
  return api.get('insights/subscriptions').json<SubscriptionInsight[]>();
}

export async function getRecurringSenders(): Promise<SubscriptionInsight[]> {
  return api.get('insights/recurring-senders').json<SubscriptionInsight[]>();
}

export async function getInboxReport(): Promise<InboxReport> {
  return api.get('insights/report').json<InboxReport>();
}

export async function getTemporalInsights(): Promise<TemporalInsights> {
  return api.get('insights/temporal').json<TemporalInsights>();
}

export interface TopicCluster {
  id: string;
  name: string;
  /** "category" or "subscription" — matches sidebar group prefixes. */
  group: string;
  emailCount: number;
  unreadCount: number;
  dateRange: { start: string; end: string };
  topSenders: string[];
  sampleSubjects: string[];
}

export async function getTopicClusters(): Promise<TopicCluster[]> {
  return api.get('insights/topics').json<TopicCluster[]>();
}
