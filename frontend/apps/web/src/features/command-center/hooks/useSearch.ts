import { useQuery } from '@tanstack/react-query';
import { searchEmails } from '@emailibrium/api';
import type { SearchQuery, SearchResponse } from '@emailibrium/types';

/**
 * TanStack Query hook for email search. Only runs when the query text
 * is non-empty. Results are cached for 60 seconds.
 */
export function useSearch(query: SearchQuery) {
  return useQuery<SearchResponse>({
    queryKey: ['search', query],
    queryFn: () => searchEmails(query),
    enabled: query.text.length > 0,
    staleTime: 60_000,
    placeholderData: (previousData) => previousData,
  });
}
