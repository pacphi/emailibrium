import { useState, useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { SubscriptionInsight, ArchiveStrategy } from '@emailibrium/types';
import { getSubscriptions } from '@emailibrium/api';

export type WizardStep = 1 | 2 | 3 | 4;

export type ClusterAction = 'archive-old' | 'archive-all' | 'delete-all' | 'keep' | 'review';

export interface ClusterSelection {
  clusterId: string;
  action: ClusterAction;
}

export interface SuggestedRule {
  id: string;
  description: string;
  enabled: boolean;
  estimatedMatches: number;
}

export interface CleanupSummary {
  subscriptionsSelected: number;
  emailsToClean: number;
  hoursSaved: number;
}

export function useInboxCleaner() {
  const [currentStep, setCurrentStep] = useState<WizardStep>(1);
  const [selectedSubscriptions, setSelectedSubscriptions] = useState<Set<string>>(new Set());
  const [clusterSelections, setClusterSelections] = useState<Map<string, ClusterAction>>(new Map());
  const [archiveStrategy, setArchiveStrategy] = useState<ArchiveStrategy>('instant');
  const [suggestedRules, setSuggestedRules] = useState<SuggestedRule[]>([]);

  const subscriptionsQuery = useQuery<SubscriptionInsight[]>({
    queryKey: ['subscriptions'],
    queryFn: () => getSubscriptions(),
    staleTime: 60_000,
  });

  const subscriptions = subscriptionsQuery.data ?? [];

  const categorized = useMemo(() => {
    const neverOpened: SubscriptionInsight[] = [];
    const rarelyOpened: SubscriptionInsight[] = [];
    const regularlyOpened: SubscriptionInsight[] = [];

    for (const sub of subscriptions) {
      // Use suggestedAction as proxy for open rate
      if (sub.suggestedAction === 'unsubscribe') {
        neverOpened.push(sub);
      } else if (sub.suggestedAction === 'archive' || sub.suggestedAction === 'digest') {
        rarelyOpened.push(sub);
      } else {
        regularlyOpened.push(sub);
      }
    }

    return { neverOpened, rarelyOpened, regularlyOpened };
  }, [subscriptions]);

  const summary = useMemo<CleanupSummary>(() => {
    const selected = subscriptions.filter((s) => selectedSubscriptions.has(s.senderAddress));
    const emailsToClean = selected.reduce((sum, s) => sum + s.emailCount, 0);
    // Rough estimate: 30 seconds per email to manually process
    const hoursSaved = Math.round((emailsToClean * 0.5) / 60);

    return {
      subscriptionsSelected: selected.length,
      emailsToClean,
      hoursSaved,
    };
  }, [subscriptions, selectedSubscriptions]);

  const goNext = useCallback(() => {
    setCurrentStep((s) => (s < 4 ? ((s + 1) as WizardStep) : s));
  }, []);

  const goBack = useCallback(() => {
    setCurrentStep((s) => (s > 1 ? ((s - 1) as WizardStep) : s));
  }, []);

  const goToStep = useCallback((step: WizardStep) => {
    setCurrentStep(step);
  }, []);

  const toggleSubscription = useCallback((senderAddress: string) => {
    setSelectedSubscriptions((prev) => {
      const next = new Set(prev);
      if (next.has(senderAddress)) {
        next.delete(senderAddress);
      } else {
        next.add(senderAddress);
      }
      return next;
    });
  }, []);

  const selectAllInSection = useCallback((subs: SubscriptionInsight[]) => {
    setSelectedSubscriptions((prev) => {
      const next = new Set(prev);
      for (const sub of subs) {
        next.add(sub.senderAddress);
      }
      return next;
    });
  }, []);

  const deselectAllInSection = useCallback((subs: SubscriptionInsight[]) => {
    setSelectedSubscriptions((prev) => {
      const next = new Set(prev);
      for (const sub of subs) {
        next.delete(sub.senderAddress);
      }
      return next;
    });
  }, []);

  const setClusterAction = useCallback((clusterId: string, action: ClusterAction) => {
    setClusterSelections((prev) => {
      const next = new Map(prev);
      next.set(clusterId, action);
      return next;
    });
  }, []);

  const toggleRule = useCallback((ruleId: string) => {
    setSuggestedRules((prev) =>
      prev.map((r) => (r.id === ruleId ? { ...r, enabled: !r.enabled } : r)),
    );
  }, []);

  return {
    currentStep,
    goNext,
    goBack,
    goToStep,
    subscriptions,
    categorized,
    isLoadingSubscriptions: subscriptionsQuery.isLoading,
    subscriptionsError: subscriptionsQuery.error,
    selectedSubscriptions,
    toggleSubscription,
    selectAllInSection,
    deselectAllInSection,
    clusterSelections,
    setClusterAction,
    archiveStrategy,
    setArchiveStrategy,
    suggestedRules,
    setSuggestedRules,
    toggleRule,
    summary,
  };
}
