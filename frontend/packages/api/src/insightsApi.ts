import type { SubscriptionInsight, InboxReport } from '@emailibrium/types';
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
