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
import { useSyncStore } from '@/shared/stores/syncStore';
import { useCurrentUserId } from '@/shared/hooks/useCurrentUserId';

// Lazy-loaded feature components
const CommandCenter = lazy(() =>
  import('@/features/command-center/CommandCenter').then((m) => ({ default: m.CommandCenter })),
);
const InboxCleaner = lazy(() =>
  import('@/features/inbox-cleaner/InboxCleaner').then((m) => ({ default: m.InboxCleaner })),
);
const CleanupHistory = lazy(() =>
  import('@/features/inbox-cleaner/history/CleanupHistory').then((m) => ({
    default: m.CleanupHistory,
  })),
);
const CleanupHistoryDetail = lazy(() =>
  import('@/features/inbox-cleaner/history/CleanupHistoryDetail').then((m) => ({
    default: m.CleanupHistoryDetail,
  })),
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
  const startSync = useSyncStore((s) => s.startSync);

  useEffect(() => {
    if (status === 'connected') {
      // Auto-start sync for all active accounts after OAuth success.
      // The sync runs in the background via the global store — state persists
      // across navigation so the Dashboard will show progress when it mounts.
      startSync();
    }
  }, [status, startSync]);

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

// userId is derived from the first active connected account's email address.
// This is the stable identifier used by the cleanup API (Phase A deviation;
// Phase D will derive it server-side from the auth header instead).
function InboxCleanerPage() {
  const userId = useCurrentUserId();
  return (
    <Suspense fallback={LoadingFallback}>
      <InboxCleaner userId={userId} />
    </Suspense>
  );
}

function CleanupHistoryPage() {
  const userId = useCurrentUserId();
  return (
    <Suspense fallback={LoadingFallback}>
      <CleanupHistory userId={userId} />
    </Suspense>
  );
}

function CleanupHistoryDetailPage() {
  const userId = useCurrentUserId();
  const { planId } = cleanupHistoryDetailRoute.useParams();
  return (
    <Suspense fallback={LoadingFallback}>
      <CleanupHistoryDetail userId={userId} planId={planId} />
    </Suspense>
  );
}

const inboxCleanerRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/inbox-cleaner',
  component: InboxCleanerPage,
});

const cleanupHistoryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/cleanup/history',
  component: CleanupHistoryPage,
});

const cleanupHistoryDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/cleanup/history/$planId',
  component: CleanupHistoryDetailPage,
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
  cleanupHistoryRoute,
  cleanupHistoryDetailRoute,
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
