import { lazy, Suspense, useEffect } from 'react';
import {
  createRouter,
  createRoute,
  createRootRoute,
  RouterProvider,
  Outlet,
  Navigate,
} from '@tanstack/react-router';
import { Layout } from './Layout';
import { getAccounts, startIngestion } from '@emailibrium/api';

// Lazy-loaded feature components
const CommandCenter = lazy(() =>
  import('@/features/command-center/CommandCenter').then((m) => ({ default: m.CommandCenter })),
);
const InboxCleaner = lazy(() =>
  import('@/features/inbox-cleaner/InboxCleaner').then((m) => ({ default: m.InboxCleaner })),
);
const InsightsExplorer = lazy(() =>
  import('@/features/insights/InsightsExplorer').then((m) => ({ default: m.InsightsExplorer })),
);
const EmailClient = lazy(() =>
  import('@/features/email/EmailClient').then((m) => ({ default: m.EmailClient })),
);
const RulesStudio = lazy(() =>
  import('@/features/rules/RulesStudio').then((m) => ({ default: m.RulesStudio })),
);
const Settings = lazy(() =>
  import('@/features/settings/Settings').then((m) => ({ default: m.Settings })),
);
const ChatInterface = lazy(() =>
  import('@/features/chat/ChatInterface').then((m) => ({ default: m.ChatInterface })),
);
const OnboardingFlow = lazy(() =>
  import('@/features/onboarding/OnboardingFlow').then((m) => ({ default: m.OnboardingFlow })),
);

const LoadingFallback = <div>Loading...</div>;

// Root route with layout
const rootRoute = createRootRoute({
  component: () => (
    <Layout>
      <Outlet />
    </Layout>
  ),
});

function OAuthReturnHandler() {
  const params = new URLSearchParams(window.location.search);
  const status = params.get('status');

  useEffect(() => {
    if (status === 'connected') {
      // Auto-start ingestion for all active accounts after OAuth success.
      getAccounts()
        .then((accounts) => {
          const active = accounts.filter((a) => a.isActive);
          return Promise.all(active.map((a) => startIngestion(a.id)));
        })
        .catch(() => {});
    }
  }, [status]);

  return <Navigate to="/command-center" />;
}

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: OAuthReturnHandler,
});

const commandCenterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/command-center',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <CommandCenter />
    </Suspense>
  ),
});

const inboxCleanerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inbox-cleaner',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <InboxCleaner />
    </Suspense>
  ),
});

const insightsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/insights',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <InsightsExplorer />
    </Suspense>
  ),
});

const emailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/email',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <EmailClient />
    </Suspense>
  ),
});

const rulesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/rules',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <RulesStudio />
    </Suspense>
  ),
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <Settings />
    </Suspense>
  ),
});

const chatRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/chat',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <ChatInterface />
    </Suspense>
  ),
});

const onboardingRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/onboarding',
  component: () => (
    <Suspense fallback={LoadingFallback}>
      <OnboardingFlow />
    </Suspense>
  ),
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  commandCenterRoute,
  inboxCleanerRoute,
  insightsRoute,
  emailRoute,
  rulesRoute,
  chatRoute,
  settingsRoute,
  onboardingRoute,
]);

const router = createRouter({ routeTree });

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

export function AppRouter() {
  return <RouterProvider router={router} />;
}
