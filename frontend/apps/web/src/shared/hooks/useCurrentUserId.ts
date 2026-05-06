import { useQuery } from '@tanstack/react-query';
import { getAccounts } from '@emailibrium/api';

/**
 * Derives the current user's identity from their first active connected account.
 *
 * The cleanup API requires a `userId` query param (a Phase A deviation; Phase D
 * will derive it server-side from the auth header). We use the first active
 * account's email address as the stable identifier — this is what any plan
 * previously created via InboxCleaner will also use.
 *
 * Returns `null` while loading or when no accounts are connected.
 */
export function useCurrentUserId(): string | null {
  const { data } = useQuery({
    queryKey: ['accounts'],
    queryFn: getAccounts,
    staleTime: 60_000,
  });

  if (!data || data.length === 0) return null;
  const active = data.find((a) => a.isActive) ?? data[0];
  return active?.emailAddress ?? null;
}
