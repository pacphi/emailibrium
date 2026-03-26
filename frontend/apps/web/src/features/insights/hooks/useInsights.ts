import { useQuery } from '@tanstack/react-query';
import { getSubscriptions, getRecurringSenders, getInboxReport, getTemporalInsights } from '@emailibrium/api';
import type { SubscriptionInsight, InboxReport, TemporalInsights } from '@emailibrium/types';

export function useInboxReport() {
  return useQuery<InboxReport>({
    queryKey: ['inbox-report'],
    queryFn: getInboxReport,
    staleTime: 60_000,
  });
}

export function useSubscriptions() {
  return useQuery<SubscriptionInsight[]>({
    queryKey: ['subscriptions'],
    queryFn: getSubscriptions,
    staleTime: 60_000,
  });
}

export function useRecurringSenders() {
  return useQuery<SubscriptionInsight[]>({
    queryKey: ['recurring-senders'],
    queryFn: getRecurringSenders,
    staleTime: 60_000,
  });
}

export function useTemporalInsights() {
  return useQuery<TemporalInsights>({
    queryKey: ['temporal-insights'],
    queryFn: getTemporalInsights,
    staleTime: 60_000,
  });
}
