import { useState, useCallback } from 'react';
import { useInboxCleaner } from './hooks/useInboxCleaner';
import type { WizardStep } from './hooks/useInboxCleaner';
import { IngestionProgressScreen } from './IngestionProgress';
import { Step2Subscriptions } from './Step2Subscriptions';
import { Step3Topics } from './Step3Topics';
import { Step4Rules } from './Step4Rules';
import { CleanupProgress } from './CleanupProgress';
import type { CleanupAction, CleanupState } from './CleanupProgress';
import type { Cluster } from '@emailibrium/types';
import { useGenerativeRouter } from '../../services/ai/useGenerativeRouter';

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

export function InboxCleaner() {
  const wizard = useInboxCleaner();
  const router = useGenerativeRouter();
  const [ingestionJobId, setIngestionJobId] = useState<string | null>(null);
  const [showCleanupProgress, setShowCleanupProgress] = useState(false);
  const [cleanupActions, setCleanupActions] = useState<CleanupAction[]>([]);
  const [cleanupState, setCleanupState] = useState<CleanupState>('running');
  const [cleanupErrors] = useState<string[]>([]);

  // Placeholder clusters (would come from an API query in production)
  const [clusters] = useState<Cluster[]>([]);
  const [clustersLoading] = useState(false);

  const handleIngestionComplete = useCallback(() => {
    wizard.goNext();
  }, [wizard]);

  const handleExecuteCleanup = useCallback(() => {
    setShowCleanupProgress(true);

    const actions: CleanupAction[] = [];
    if (wizard.selectedSubscriptions.size > 0) {
      actions.push({
        type: 'unsubscribe',
        total: wizard.selectedSubscriptions.size,
        completed: 0,
        failed: 0,
      });
    }

    // Count archive/delete actions from cluster selections
    let archiveCount = 0;
    let deleteCount = 0;
    wizard.clusterSelections.forEach((action, _clusterId) => {
      const cluster = clusters.find((c) => c.id === _clusterId);
      const count = cluster?.emailCount ?? 0;
      if (action === 'archive-old' || action === 'archive-all') archiveCount += count;
      if (action === 'delete-all') deleteCount += count;
    });

    if (archiveCount > 0) {
      actions.push({ type: 'archive', total: archiveCount, completed: 0, failed: 0 });
    }
    if (deleteCount > 0) {
      actions.push({ type: 'delete', total: deleteCount, completed: 0, failed: 0 });
    }

    setCleanupActions(actions);

    // Simulate progress (in production this would be driven by SSE/API)
    let tick = 0;
    const interval = setInterval(() => {
      tick++;
      setCleanupActions((prev) =>
        prev.map((a) => ({
          ...a,
          completed: Math.min(a.total, Math.round((tick / 10) * a.total)),
        })),
      );
      if (tick >= 10) {
        clearInterval(interval);
        setCleanupState('done');
      }
    }, 500);
  }, [wizard.selectedSubscriptions, wizard.clusterSelections, clusters]);

  const handleCleanupDone = useCallback(() => {
    setShowCleanupProgress(false);
    // Navigate away or reset wizard
  }, []);

  // Cleanup progress overlay
  if (showCleanupProgress) {
    return (
      <div className="p-6 max-w-2xl mx-auto">
        <h2 className="text-lg font-bold text-gray-900 dark:text-gray-100 mb-6 text-center">
          Executing Cleanup
        </h2>
        <CleanupProgress
          actions={cleanupActions}
          state={cleanupState}
          onDone={handleCleanupDone}
          errors={cleanupErrors}
        />
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
                  onClick={() => setIngestionJobId('job-' + Date.now())}
                  className="px-6 py-2.5 text-sm font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 transition-colors"
                >
                  Begin Ingestion
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
            <button
              onClick={handleExecuteCleanup}
              disabled={
                wizard.selectedSubscriptions.size === 0 &&
                wizard.clusterSelections.size === 0 &&
                wizard.suggestedRules.filter((r) => r.enabled).length === 0
              }
              className="px-5 py-2 text-sm font-medium text-white bg-green-600 rounded-md hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              Execute Cleanup
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
