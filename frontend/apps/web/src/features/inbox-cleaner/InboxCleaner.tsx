import { useState, useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useInboxCleaner } from './hooks/useInboxCleaner';
import type { WizardStep } from './hooks/useInboxCleaner';
import { IngestionProgressScreen } from './IngestionProgress';
import { Step2Subscriptions } from './Step2Subscriptions';
import { Step3Topics } from './Step3Topics';
import { Step4Rules } from './Step4Rules';
import type { PipelineActivity, PlanId } from '@emailibrium/types';
import {
  getAccounts,
  getPipelineLockStatus,
  startIngestion,
  PipelineBusyError,
  getIngestionProgress,
  getClusters,
} from '@emailibrium/api';
import { useGenerativeRouter } from '../../services/ai/useGenerativeRouter';
import { CleanupReview } from './review/CleanupReview';

const stepLabels: Record<WizardStep, string> = {
  1: 'Connect & Ingest',
  2: 'Review Subscriptions',
  3: 'Clean Topics',
  4: 'Set Rules',
};

interface StepIndicatorProps {
  currentStep: WizardStep;
  onStepClick: (step: WizardStep) => void;
}

function StepIndicator({ currentStep, onStepClick }: StepIndicatorProps) {
  const steps: WizardStep[] = [1, 2, 3, 4];

  return (
    <nav className="flex items-center justify-center" aria-label="Wizard steps">
      {steps.map((step, index) => {
        const isComplete = step < currentStep;
        const isCurrent = step === currentStep;
        const isClickable = step <= currentStep;

        return (
          <div key={step} className="flex items-center">
            <button
              onClick={() => isClickable && onStepClick(step)}
              disabled={!isClickable}
              className={`flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium transition-colors ${
                isCurrent
                  ? 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300'
                  : isComplete
                    ? 'text-green-600 dark:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/20 cursor-pointer'
                    : 'text-gray-400 dark:text-gray-500 cursor-not-allowed'
              }`}
            >
              <span
                className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold ${
                  isCurrent
                    ? 'bg-indigo-600 text-white'
                    : isComplete
                      ? 'bg-green-500 text-white'
                      : 'bg-gray-200 text-gray-500 dark:bg-gray-700 dark:text-gray-400'
                }`}
              >
                {isComplete ? (
                  <svg
                    className="w-3.5 h-3.5"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={3}
                      d="M5 13l4 4L19 7"
                    />
                  </svg>
                ) : (
                  step
                )}
              </span>
              <span className="hidden sm:inline">{stepLabels[step]}</span>
            </button>
            {index < steps.length - 1 && (
              <div
                className={`w-8 h-0.5 mx-1 ${
                  step < currentStep ? 'bg-green-400' : 'bg-gray-200 dark:bg-gray-700'
                }`}
              />
            )}
          </div>
        );
      })}
    </nav>
  );
}

export interface InboxCleanerProps {
  /** Current authenticated userId. Required to build/review/apply cleanup plans. */
  userId?: string | null;
}

export function InboxCleaner({ userId = null }: InboxCleanerProps = {}) {
  const wizard = useInboxCleaner({ userId });
  const router = useGenerativeRouter();
  // Phase B: review-step state. Set when user advances from Step 4.
  const [reviewPlanId, setReviewPlanId] = useState<PlanId | null>(null);
  const [isAdvancingToReview, setIsAdvancingToReview] = useState(false);
  const [advanceError, setAdvanceError] = useState<string | null>(null);
  const [ingestionJobId, setIngestionJobId] = useState<string | null>(null);
  const [pipelineConflict, setPipelineConflict] = useState<PipelineActivity | null>(null);
  const [checkingLock, setCheckingLock] = useState(false);

  const clustersQuery = useQuery({
    queryKey: ['inbox-cleaner-clusters'],
    queryFn: getClusters,
    staleTime: 5 * 60 * 1000,
  });
  const clusters = clustersQuery.data ?? [];
  const clustersLoading = clustersQuery.isLoading;

  const handleIngestionComplete = useCallback(() => {
    setIngestionJobId(null); // clear so returning to Step 1 shows "Begin Ingestion"
    wizard.goNext();
  }, [wizard]);

  /** Pre-flight check: ensure no pipeline is already running for any active account. */
  const handleBeginIngestion = useCallback(async () => {
    setPipelineConflict(null);
    setCheckingLock(true);
    try {
      const accounts = await getAccounts();
      const active = accounts.filter((a) => a.isActive);
      for (const acct of active) {
        const activity = await getPipelineLockStatus(acct.id);
        if (activity) {
          setPipelineConflict(activity);
          setCheckingLock(false);
          return;
        }
      }
      if (active.length === 0) {
        setCheckingLock(false);
        return;
      }

      const { jobId } = await startIngestion(active[0]!.id, 'inbox_clean');

      // The pipeline runs asynchronously and may complete in milliseconds for
      // already-synced accounts. Poll the REST endpoint for up to 3 seconds
      // before falling back to the SSE progress screen (which is only needed
      // for long-running pipelines with actual emails to process).
      const POLL_INTERVAL = 300;
      const POLL_MAX_MS = 3_000;
      const deadline = Date.now() + POLL_MAX_MS;

      while (Date.now() < deadline) {
        await new Promise<void>((r) => setTimeout(r, POLL_INTERVAL));
        try {
          const snapshot = await getIngestionProgress();
          // active:false  → no job in memory (fresh state)
          // phase:complete → job finished but current_job not yet cleared
          if (!snapshot.active || snapshot.phase === 'complete') {
            wizard.goNext();
            setCheckingLock(false);
            return;
          }
        } catch {
          // Transient error; keep polling.
        }
      }

      // Pipeline still running after 3 s — hand off to the SSE progress screen.
      setIngestionJobId(jobId);
    } catch (e) {
      if (e instanceof PipelineBusyError) {
        setPipelineConflict({
          jobId: e.activity.existingJobId,
          accountId: '',
          phase: e.activity.existingPhase,
          startedAt: e.activity.startedAt,
          source: e.activity.existingSource,
        });
      }
      // Other errors (network, auth): surface nothing, user can retry.
    }
    setCheckingLock(false);
  }, [wizard]);

  // Phase C: build a plan and advance to the Review step. The Phase B
  // simulator (setInterval-driven CleanupProgress fake) was removed once
  // CleanupReview / useCleanupApply wired the real SSE apply path.
  const handleContinueToReview = useCallback(async () => {
    setAdvanceError(null);
    setIsAdvancingToReview(true);
    try {
      const planId = await wizard.buildPlan();
      setReviewPlanId(planId);
    } catch (e) {
      setAdvanceError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsAdvancingToReview(false);
    }
  }, [wizard]);

  const handleReviewCancel = useCallback(() => {
    setReviewPlanId(null);
  }, []);

  // Phase C: review screen owns the apply lifecycle (useCleanupApply) and
  // renders <CleanupProgress> internally once the user clicks Apply.
  if (reviewPlanId && userId) {
    return (
      <div className="p-6 max-w-5xl mx-auto">
        <CleanupReview planId={reviewPlanId} userId={userId} onCancel={handleReviewCancel} />
      </div>
    );
  }

  return (
    <div className="p-6 max-w-3xl mx-auto space-y-6">
      {/* Header */}
      <div>
        <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100">Inbox Cleaner</h1>
        <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
          Analyze and clean your inbox in four steps.
        </p>
        {router.provider === 'builtin' && (
          <span className="mt-1 inline-block rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400">
            Powered by built-in AI (local)
          </span>
        )}
      </div>

      {/* AI-powered classification integration point:
       * When router.provider !== 'none', use router.classify() to get
       * AI-powered email classification during ingestion (Step 1) or
       * subscription categorisation (Step 2). The classify call accepts
       * { subject, sender, bodyPreview, categories } and returns
       * { category, confidence, reasoning }. */}

      {/* Step indicator */}
      <StepIndicator currentStep={wizard.currentStep} onStepClick={wizard.goToStep} />

      {/* Summary bar */}
      {wizard.currentStep > 1 && (
        <div className="flex items-center justify-between rounded-lg bg-indigo-50 dark:bg-indigo-900/20 border border-indigo-200 dark:border-indigo-800 px-4 py-3">
          <div className="flex items-center gap-6 text-sm">
            <span className="text-indigo-700 dark:text-indigo-300">
              <span className="font-semibold">{wizard.summary.subscriptionsSelected}</span>{' '}
              subscriptions
            </span>
            <span className="text-indigo-700 dark:text-indigo-300">
              <span className="font-semibold">{wizard.summary.emailsToClean.toLocaleString()}</span>{' '}
              emails to clean
            </span>
            <span className="text-indigo-700 dark:text-indigo-300">
              <span className="font-semibold">{wizard.summary.hoursSaved}</span> hrs/month saved
            </span>
          </div>
        </div>
      )}

      {/* Step content */}
      <div className="min-h-[300px]">
        {wizard.currentStep === 1 && (
          <div>
            {ingestionJobId ? (
              <IngestionProgressScreen
                jobId={ingestionJobId}
                onComplete={handleIngestionComplete}
                onCancel={() => setIngestionJobId(null)}
              />
            ) : (
              <div className="rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-8 text-center space-y-4">
                {/* Pipeline conflict alert */}
                {pipelineConflict && (
                  <div className="rounded-lg border border-amber-300 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 px-4 py-3 text-left mb-4">
                    <p className="text-sm font-medium text-amber-800 dark:text-amber-300">
                      Pipeline already active
                    </p>
                    <p className="text-sm text-amber-700 dark:text-amber-400 mt-1">
                      A{' '}
                      <strong>
                        {pipelineConflict.source === 'manual_sync'
                          ? 'manual sync'
                          : pipelineConflict.source === 'inbox_clean'
                            ? 'previous Inbox Clean'
                            : pipelineConflict.source === 'onboarding'
                              ? 'onboarding sync'
                              : pipelineConflict.source === 'poll'
                                ? 'background sync'
                                : pipelineConflict.source}
                      </strong>{' '}
                      operation is running ({pipelineConflict.phase} phase). Please wait for it to
                      complete before starting Inbox Clean.
                    </p>
                    <button
                      onClick={() => setPipelineConflict(null)}
                      className="mt-2 text-xs text-amber-600 dark:text-amber-400 underline hover:no-underline"
                    >
                      Dismiss
                    </button>
                  </div>
                )}

                <div className="w-16 h-16 rounded-full bg-indigo-100 dark:bg-indigo-900/30 mx-auto flex items-center justify-center">
                  <svg
                    className="w-8 h-8 text-indigo-600 dark:text-indigo-400"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
                    />
                  </svg>
                </div>
                <h3 className="text-base font-semibold text-gray-900 dark:text-gray-100">
                  Start Inbox Analysis
                </h3>
                <p className="text-sm text-gray-500 dark:text-gray-400 max-w-sm mx-auto">
                  We will sync your emails, generate embeddings, categorize messages, cluster
                  topics, and analyze patterns.
                </p>
                <button
                  onClick={handleBeginIngestion}
                  disabled={checkingLock}
                  className="px-6 py-2.5 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 disabled:opacity-50 disabled:cursor-wait transition-colors"
                >
                  {checkingLock ? 'Checking...' : 'Begin Ingestion'}
                </button>
              </div>
            )}
          </div>
        )}

        {wizard.currentStep === 2 && (
          <Step2Subscriptions
            categorized={wizard.categorized}
            selectedSubscriptions={wizard.selectedSubscriptions}
            onToggle={wizard.toggleSubscription}
            onSelectAll={wizard.selectAllInSection}
            onDeselectAll={wizard.deselectAllInSection}
            isLoading={wizard.isLoadingSubscriptions}
            error={wizard.subscriptionsError}
          />
        )}

        {wizard.currentStep === 3 && (
          <Step3Topics
            clusters={clusters}
            clusterSelections={wizard.clusterSelections}
            onSetAction={wizard.setClusterAction}
            isLoading={clustersLoading}
          />
        )}

        {wizard.currentStep === 4 && (
          <Step4Rules
            suggestedRules={wizard.suggestedRules}
            onToggleRule={wizard.toggleRule}
            archiveStrategy={wizard.archiveStrategy}
            onArchiveStrategyChange={wizard.setArchiveStrategy}
          />
        )}
      </div>

      {/* Navigation buttons */}
      <div className="flex items-center justify-between pt-4 border-t border-gray-200 dark:border-gray-700">
        <button
          onClick={wizard.goBack}
          disabled={wizard.currentStep === 1}
          className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${
            wizard.currentStep === 1
              ? 'text-gray-300 dark:text-gray-600 cursor-not-allowed'
              : 'text-gray-700 dark:text-gray-300 border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700'
          }`}
        >
          Back
        </button>

        <div className="flex gap-3">
          {wizard.currentStep < 4 && wizard.currentStep > 1 && (
            <button
              onClick={wizard.goNext}
              className="px-5 py-2 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 transition-colors"
            >
              Next
            </button>
          )}

          {wizard.currentStep === 4 && (
            <div className="flex flex-col items-end gap-1">
              <button
                onClick={handleContinueToReview}
                disabled={
                  isAdvancingToReview ||
                  !userId ||
                  (wizard.selectedSubscriptions.size === 0 &&
                    wizard.clusterSelections.size === 0 &&
                    wizard.suggestedRules.filter((r) => r.enabled).length === 0)
                }
                aria-disabled={
                  isAdvancingToReview ||
                  !userId ||
                  (wizard.selectedSubscriptions.size === 0 &&
                    wizard.clusterSelections.size === 0 &&
                    wizard.suggestedRules.filter((r) => r.enabled).length === 0)
                }
                title={!userId ? 'Sign in required to build a cleanup plan' : undefined}
                className="px-5 py-2 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                {isAdvancingToReview ? 'Building plan…' : 'Continue to Review'}
              </button>
              {advanceError && (
                <p className="text-xs text-red-600 dark:text-red-400" role="alert">
                  {advanceError}
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
