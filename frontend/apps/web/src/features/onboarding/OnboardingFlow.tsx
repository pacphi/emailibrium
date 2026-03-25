import { useState, useCallback, useEffect } from 'react';
import type { ArchiveStrategy, EmailAccount, Provider } from '@emailibrium/types';
import { ProviderSelector } from './ProviderSelector';
import { GmailConnect } from './GmailConnect';
import { OutlookConnect } from './OutlookConnect';
import { ImapConnect } from './ImapConnect';
import { ConnectedAccounts } from './ConnectedAccounts';
import { ArchiveStrategyPicker } from './ArchiveStrategyPicker';
import { AISetup, type AiSetupState } from './AISetup';
import { SetupComplete } from './SetupComplete';

type OnboardingStep = 'welcome' | 'connect' | 'accounts' | 'ai-setup' | 'strategy' | 'complete';

const STEP_ORDER: OnboardingStep[] = [
  'welcome',
  'connect',
  'accounts',
  'ai-setup',
  'strategy',
  'complete',
];

const STEP_LABELS: Record<OnboardingStep, string> = {
  welcome: 'Email',
  connect: 'Connect',
  accounts: 'Accounts',
  'ai-setup': 'AI',
  strategy: 'Strategy',
  complete: 'Done',
};

function StepIndicator({ currentStep }: { currentStep: OnboardingStep }) {
  const currentIndex = STEP_ORDER.indexOf(currentStep);
  return (
    <div
      className="flex items-center justify-center gap-3 mb-8"
      role="navigation"
      aria-label="Onboarding progress"
    >
      {STEP_ORDER.map((step, index) => (
        <div key={step} className="flex flex-col items-center gap-1">
          <span
            className={`w-2.5 h-2.5 rounded-full transition-colors ${
              index <= currentIndex
                ? 'bg-indigo-600 dark:bg-indigo-400'
                : 'bg-gray-300 dark:bg-gray-600'
            }`}
            aria-current={step === currentStep ? 'step' : undefined}
            aria-label={`Step ${index + 1}: ${STEP_LABELS[step]}`}
          />
          <span
            className={`text-[10px] font-medium transition-colors ${
              index <= currentIndex
                ? 'text-indigo-600 dark:text-indigo-400'
                : 'text-gray-400 dark:text-gray-500'
            }`}
            aria-hidden="true"
          >
            {STEP_LABELS[step]}
          </span>
        </div>
      ))}
    </div>
  );
}

type BackendStatus = 'checking' | 'online' | 'offline';

function ServerHealthBadge({ status }: { status: BackendStatus }) {
  if (status === 'checking') {
    return (
      <div
        className="flex items-center justify-center gap-2 mb-4 px-3 py-1.5 rounded-full
          bg-yellow-50 border border-yellow-200 dark:bg-yellow-900/20 dark:border-yellow-800
          text-xs text-yellow-700 dark:text-yellow-400"
        role="status"
        aria-live="polite"
      >
        <svg className="animate-spin w-3 h-3" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle
            className="opacity-25"
            cx="12"
            cy="12"
            r="10"
            stroke="currentColor"
            strokeWidth="4"
          />
          <path
            className="opacity-75"
            fill="currentColor"
            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
          />
        </svg>
        Checking backend...
      </div>
    );
  }
  if (status === 'online') {
    return (
      <div
        className="flex items-center justify-center gap-2 mb-4 px-3 py-1.5 rounded-full
          bg-green-50 border border-green-200 dark:bg-green-900/20 dark:border-green-800
          text-xs text-green-700 dark:text-green-400"
        role="status"
        aria-live="polite"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-green-500" aria-hidden="true" />
        Backend connected
      </div>
    );
  }
  return (
    <div className="flex flex-col items-center gap-1 mb-4" role="alert" aria-live="polite">
      <div
        className="flex items-center gap-2 px-3 py-1.5 rounded-full
          bg-red-50 border border-red-200 dark:bg-red-900/20 dark:border-red-800
          text-xs text-red-700 dark:text-red-400"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-red-500" aria-hidden="true" />
        Backend offline — start the server first
      </div>
      <code className="text-[10px] text-gray-500 dark:text-gray-400 font-mono">make dev</code>
    </div>
  );
}

export function OnboardingFlow() {
  const [step, setStep] = useState<OnboardingStep>('welcome');
  const [selectedProvider, setSelectedProvider] = useState<Provider | 'other' | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<EmailAccount[]>([]);
  const [archiveStrategy, setArchiveStrategy] = useState<ArchiveStrategy>('delayed');
  const [aiSetup, setAiSetup] = useState<AiSetupState | null>(null);
  const [backendStatus, setBackendStatus] = useState<BackendStatus>('checking');

  // Server health check on mount
  useEffect(() => {
    const controller = new AbortController();
    fetch('/api/v1/vectors/health', { signal: controller.signal })
      .then((res) => {
        setBackendStatus(res.ok ? 'online' : 'offline');
      })
      .catch(() => {
        setBackendStatus('offline');
      });
    return () => controller.abort();
  }, []);

  const handleProviderSelect = useCallback((provider: Provider | 'other') => {
    setSelectedProvider(provider);
    setStep('connect');
  }, []);

  const handleBackToProviders = useCallback(() => {
    setSelectedProvider(null);
    setStep('welcome');
  }, []);

  const handleProviderConnected = useCallback((account: EmailAccount) => {
    setConnectedAccounts((prev) => [...prev, account]);
    setStep('accounts');
  }, []);

  const handleDisconnect = useCallback((accountId: string) => {
    setConnectedAccounts((prev) => prev.filter((a) => a.id !== accountId));
  }, []);

  const handleSkipEmail = useCallback(() => {
    setStep('ai-setup');
  }, []);

  const handleAiContinue = useCallback((state: AiSetupState) => {
    setAiSetup(state);
    setStep('strategy');
  }, []);

  const handleAiSkip = useCallback(() => {
    setStep('strategy');
  }, []);

  const handleFinish = useCallback(() => {
    window.location.href = '/command-center';
  }, []);

  const handleGoToSettings = useCallback(() => {
    window.location.href = '/settings';
  }, []);

  return (
    <div className="min-h-screen flex flex-col items-center justify-center px-4 py-12 bg-gray-50 dark:bg-gray-900">
      <div className="w-full max-w-2xl">
        <StepIndicator currentStep={step} />

        {/* Step 1: Welcome */}
        {step === 'welcome' && (
          <div className="space-y-8">
            <ServerHealthBadge status={backendStatus} />
            <div className="text-center space-y-3">
              <h1 className="text-3xl font-bold text-gray-900 dark:text-gray-100">
                Take control of your inbox
              </h1>
              <p className="text-lg text-gray-600 dark:text-gray-400 max-w-md mx-auto">
                Emailibrium uses AI to organize, prioritize, and tame your email. Connect an account
                to get started.
              </p>
            </div>
            <ProviderSelector onSelect={handleProviderSelect} />
            <div className="text-center">
              <button
                type="button"
                onClick={handleSkipEmail}
                className="text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400
                  dark:hover:text-gray-200 transition-colors"
              >
                Skip — I&apos;ll connect email later
              </button>
            </div>
          </div>
        )}

        {/* Step 2: Connect */}
        {step === 'connect' && selectedProvider === 'gmail' && (
          <GmailConnect onBack={handleBackToProviders} onConnected={handleProviderConnected} />
        )}
        {step === 'connect' && selectedProvider === 'outlook' && (
          <OutlookConnect onBack={handleBackToProviders} onConnected={handleProviderConnected} />
        )}
        {step === 'connect' && (selectedProvider === 'imap' || selectedProvider === 'other') && (
          <ImapConnect onBack={handleBackToProviders} onConnected={handleProviderConnected} />
        )}

        {/* Step 3: Connected accounts */}
        {step === 'accounts' && (
          <ConnectedAccounts
            accounts={connectedAccounts}
            onAddAnother={handleBackToProviders}
            onContinue={() => setStep('ai-setup')}
            onDisconnect={handleDisconnect}
          />
        )}

        {/* Step 4: AI Setup */}
        {step === 'ai-setup' && <AISetup onContinue={handleAiContinue} onSkip={handleAiSkip} />}

        {/* Step 5: Archive strategy */}
        {step === 'strategy' && (
          <div className="space-y-8">
            <ArchiveStrategyPicker value={archiveStrategy} onChange={setArchiveStrategy} />
            <div className="flex justify-center">
              <button
                type="button"
                onClick={() => setStep('complete')}
                className="px-8 py-3 rounded-lg bg-indigo-600 text-white font-medium
                  hover:bg-indigo-700 transition-colors"
              >
                Continue
              </button>
            </div>
          </div>
        )}

        {/* Step 6: Setup Complete */}
        {step === 'complete' && (
          <SetupComplete
            accountCount={connectedAccounts.length}
            aiSetup={aiSetup}
            archiveStrategy={archiveStrategy}
            onLaunch={handleFinish}
            onSettings={handleGoToSettings}
          />
        )}
      </div>
    </div>
  );
}
