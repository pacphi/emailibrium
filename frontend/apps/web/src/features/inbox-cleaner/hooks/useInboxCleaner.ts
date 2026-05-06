import { useState, useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import type {
  SubscriptionInsight,
  ArchiveStrategy,
  CleanupArchiveStrategy,
  CleanupClusterAction,
  CleanupPlan,
  CreatePlanResponse,
  PlanId,
  WizardSelections,
} from '@emailibrium/types';
import {
  getSubscriptions,
  buildPlan as apiBuildPlan,
  getPlan as apiGetPlan,
  refreshPlanAccount as apiRefreshAccount,
  cancelPlan as apiCancelPlan,
} from '@emailibrium/api';

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

// USERID CONVENTION: This wizard hook accepts an optional `userId` argument.
// Phase A backend handlers require `?userId=` on every cleanup endpoint
// (see backend/src/cleanup/api/plan.rs). The rest of the API derives the
// user from the auth header, so callers will typically pass the current
// session's userId explicitly here. Phase D will remove the requirement
// and we can drop this argument.
export interface UseInboxCleanerOptions {
  userId?: string | null;
}

export function useInboxCleaner(options: UseInboxCleanerOptions = {}) {
  const userId = options.userId ?? null;

  const [currentStep, setCurrentStep] = useState<WizardStep>(1);
  const [selectedSubscriptions, setSelectedSubscriptions] = useState<Set<string>>(new Set());
  const [clusterSelections, setClusterSelections] = useState<Map<string, ClusterAction>>(new Map());
  const [archiveStrategy, setArchiveStrategy] = useState<ArchiveStrategy>('instant');
  const [suggestedRules, setSuggestedRules] = useState<SuggestedRule[]>([]);

  // Phase B plan state.
  const [currentPlanId, setCurrentPlanId] = useState<PlanId | null>(null);
  const [currentPlanSummary, setCurrentPlanSummary] = useState<CreatePlanResponse | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [isBuildingPlan, setIsBuildingPlan] = useState(false);

  const subscriptionsQuery = useQuery<SubscriptionInsight[]>({
    queryKey: ['subscriptions'],
    queryFn: () => getSubscriptions(),
    staleTime: 60_000,
  });

  const subscriptions = useMemo(() => subscriptionsQuery.data ?? [], [subscriptionsQuery.data]);

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

  // -------------------------------------------------------------------------
  // Phase B plan actions
  // -------------------------------------------------------------------------

  // Translate the wizard's local cluster-action vocabulary into the cleanup
  // domain vocabulary. The wizard exposes a richer choice ("archive-old" vs
  // "archive-all"); the Phase A plan domain only distinguishes
  // archive/deleteSoft/deletePermanent/label. We map conservatively and
  // drop "keep"/"review" rows.
  const mapClusterAction = useCallback((action: ClusterAction): CleanupClusterAction | null => {
    switch (action) {
      case 'archive-old':
      case 'archive-all':
        return 'archive';
      case 'delete-all':
        return 'deleteSoft';
      case 'keep':
      case 'review':
        return null;
    }
  }, []);

  // Translate the user-preference ArchiveStrategy ('instant'|'delayed'|'manual')
  // into the cleanup domain ArchiveStrategy. The two vocabularies don't
  // overlap, so we default to 'olderThan90d' and let Step 3 pick a real
  // value when present.
  const mapArchiveStrategy = useCallback((s: ArchiveStrategy): CleanupArchiveStrategy | null => {
    // Phase B: no fine-grained mapping yet. 'instant' is the closest to
    // "archive everything ready now", which we model as olderThan30d.
    if (s === 'instant') return 'olderThan30d';
    if (s === 'delayed') return 'olderThan90d';
    return null; // 'manual' → no auto archive strategy in the plan
  }, []);

  const collectSelections = useCallback((): WizardSelections => {
    const subs = subscriptions.filter((s) => selectedSubscriptions.has(s.senderAddress));
    return {
      // TODO(phase-d): SubscriptionInsight does not currently expose
      // accountId. Phase A's PlanBuilder uses the user's account scope to
      // resolve senders to accounts, so an empty accountId is acceptable
      // for now; once the insight DTO grows accountId we can pass it here.
      subscriptions: subs.map((s) => ({ sender: s.senderAddress, accountId: '' })),
      clusterActions: Array.from(clusterSelections.entries())
        .map(([clusterId, action]) => {
          const mapped = mapClusterAction(action);
          if (!mapped) return null;
          return { clusterId, action: mapped, accountId: '' };
        })
        .filter(
          (c): c is { clusterId: string; action: CleanupClusterAction; accountId: string } =>
            c !== null,
        ),
      ruleSelections: suggestedRules
        .filter((r) => r.enabled)
        .map((r) => ({ ruleId: r.id, accountId: '' })),
      archiveStrategy: mapArchiveStrategy(archiveStrategy),
      accountIds: [],
    };
  }, [
    subscriptions,
    selectedSubscriptions,
    clusterSelections,
    suggestedRules,
    archiveStrategy,
    mapClusterAction,
    mapArchiveStrategy,
  ]);

  const buildPlan = useCallback(async (): Promise<PlanId> => {
    if (!userId) {
      const msg = 'userId required to build a cleanup plan';
      setPlanError(msg);
      throw new Error(msg);
    }
    setIsBuildingPlan(true);
    setPlanError(null);
    try {
      const selections = collectSelections();
      const resp = await apiBuildPlan(userId, selections);
      setCurrentPlanId(resp.planId);
      setCurrentPlanSummary(resp);
      return resp.planId;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setPlanError(msg);
      throw e;
    } finally {
      setIsBuildingPlan(false);
    }
  }, [userId, collectSelections]);

  const refreshAccount = useCallback(
    async (accountId: string): Promise<void> => {
      if (!userId || !currentPlanId) return;
      await apiRefreshAccount(currentPlanId, userId, accountId);
    },
    [userId, currentPlanId],
  );

  const cancelCurrentPlan = useCallback(async (): Promise<void> => {
    if (!userId || !currentPlanId) return;
    await apiCancelPlan(currentPlanId, userId);
    setCurrentPlanId(null);
    setCurrentPlanSummary(null);
  }, [userId, currentPlanId]);

  const loadPlan = useCallback(
    async (planId: PlanId): Promise<CleanupPlan> => {
      if (!userId) throw new Error('userId required to load a cleanup plan');
      const plan = await apiGetPlan(userId, planId);
      setCurrentPlanId(plan.id);
      return plan;
    },
    [userId],
  );

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
    // Phase B plan state + actions
    currentPlanId,
    currentPlanSummary,
    planError,
    isBuildingPlan,
    buildPlan,
    refreshAccount,
    cancelCurrentPlan,
    loadPlan,
  };
}
