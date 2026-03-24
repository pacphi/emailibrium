import { useState, useCallback } from 'react';
import type { ArchiveStrategy, EmailAccount, Provider } from '@emailibrium/types';
import { ProviderSelector } from './ProviderSelector';
import { GmailConnect } from './GmailConnect';
import { OutlookConnect } from './OutlookConnect';
import { ImapConnect } from './ImapConnect';
import { ConnectedAccounts } from './ConnectedAccounts';
import { ArchiveStrategyPicker } from './ArchiveStrategyPicker';

type OnboardingStep = 'welcome' | 'connect' | 'accounts' | 'strategy';

const STEP_ORDER: OnboardingStep[] = ['welcome', 'connect', 'accounts', 'strategy'];

function StepIndicator({ currentStep }: { currentStep: OnboardingStep }) {
  const currentIndex = STEP_ORDER.indexOf(currentStep);
  return (
    <div
      className="flex items-center justify-center gap-2 mb-8"
      role="navigation"
      aria-label="Onboarding progress"
    >
      {STEP_ORDER.map((step, index) => (
        <span
          key={step}
          className={`w-2.5 h-2.5 rounded-full transition-colors ${
            index <= currentIndex
              ? 'bg-indigo-600 dark:bg-indigo-400'
              : 'bg-gray-300 dark:bg-gray-600'
          }`}
          aria-current={step === currentStep ? 'step' : undefined}
          aria-label={`Step ${index + 1}: ${step}`}
        />
      ))}
    </div>
  );
}

export function OnboardingFlow() {
  const [step, setStep] = useState<OnboardingStep>('welcome');
  const [selectedProvider, setSelectedProvider] = useState<Provider | 'other' | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<EmailAccount[]>([]);
  const [archiveStrategy, setArchiveStrategy] = useState<ArchiveStrategy>('delayed');

  const handleProviderSelect = useCallback((provider: Provider | 'other') => {
    setSelectedProvider(provider);
    setStep('connect');
  }, []);

  const handleBackToProviders = useCallback(() => {
    setSelectedProvider(null);
    setStep('welcome');
  }, []);

  const handleAccountConnected = useCallback(() => {
    // In a real app, we would fetch the updated accounts list from the API.
    // For now, simulate adding a connected account.
    const mockAccount: EmailAccount = {
      id: crypto.randomUUID(),
      provider: (selectedProvider === 'other' ? 'imap' : selectedProvider) as Provider,
      emailAddress: 'user@example.com',
      archiveStrategy: 'delayed',
      syncDepth: '30d',
      labelPrefix: 'emailibrium/',
      isActive: true,
      emailCount: 0,
    };
    setConnectedAccounts((prev) => [...prev, mockAccount]);
    setStep('accounts');
  }, [selectedProvider]);

  const handleDisconnect = useCallback((accountId: string) => {
    setConnectedAccounts((prev) => prev.filter((a) => a.id !== accountId));
  }, []);

  const handleFinish = useCallback(() => {
    // Navigate to the main app. In production this would persist the strategy
    // and redirect via the router.
    window.location.href = '/command-center';
  }, []);

  return (
    <div className="min-h-screen flex flex-col items-center justify-center px-4 py-12 bg-gray-50 dark:bg-gray-900">
      <div className="w-full max-w-2xl">
        <StepIndicator currentStep={step} />

        {/* Step 1: Welcome */}
        {step === 'welcome' && (
          <div className="space-y-8">
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
          </div>
        )}

        {/* Step 2: Connect */}
        {step === 'connect' && selectedProvider === 'gmail' && (
          <GmailConnect onBack={handleBackToProviders} />
        )}
        {step === 'connect' && selectedProvider === 'outlook' && (
          <OutlookConnect onBack={handleBackToProviders} />
        )}
        {step === 'connect' && (selectedProvider === 'imap' || selectedProvider === 'other') && (
          <ImapConnect onBack={handleBackToProviders} onConnected={handleAccountConnected} />
        )}

        {/* Step 3: Connected accounts */}
        {step === 'accounts' && (
          <ConnectedAccounts
            accounts={connectedAccounts}
            onAddAnother={handleBackToProviders}
            onContinue={() => setStep('strategy')}
            onDisconnect={handleDisconnect}
          />
        )}

        {/* Step 4: Archive strategy */}
        {step === 'strategy' && (
          <div className="space-y-8">
            <ArchiveStrategyPicker value={archiveStrategy} onChange={setArchiveStrategy} />
            <div className="flex justify-center">
              <button
                type="button"
                onClick={handleFinish}
                className="px-8 py-3 rounded-lg bg-indigo-600 text-white font-medium
                  hover:bg-indigo-700 transition-colors"
              >
                Finish Setup
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
