import { useState, useCallback } from 'react';
import type {
  SearchMode,
  SearchQuery,
  SearchFilters as SearchFiltersType,
  SearchResult,
} from '@emailibrium/types';
import { useSearch } from './hooks/useSearch';
import { useDebounce } from './hooks/useDebounce';
import { SearchFilters } from './SearchFilters';

const SEARCH_MODES: { value: SearchMode; label: string }[] = [
  { value: 'hybrid', label: 'Hybrid' },
  { value: 'semantic', label: 'Semantic' },
  { value: 'keyword', label: 'Keyword' },
];

const PAGE_SIZE = 20;

export function SearchResults() {
  const [inputValue, setInputValue] = useState('');
  const [mode, setMode] = useState<SearchMode>('hybrid');
  const [filters, setFilters] = useState<SearchFiltersType>({});
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);

  const debouncedQuery = useDebounce(inputValue, 300);

  const query: SearchQuery = {
    text: debouncedQuery,
    mode,
    filters,
    limit: visibleCount,
  };

  const { data, isLoading, isError, error } = useSearch(query);

  const loadMore = useCallback(() => {
    setVisibleCount((prev) => prev + PAGE_SIZE);
  }, []);

  const hasMore = data ? visibleCount < data.total : false;

  return (
    <div className="flex flex-col gap-6 p-6 lg:flex-row">
      {/* Filters sidebar */}
      <SearchFilters filters={filters} onChange={setFilters} />

      {/* Main content */}
      <div className="min-w-0 flex-1">
        {/* Search input */}
        <div className="mb-4">
          <label htmlFor="search-input" className="sr-only">
            Search emails
          </label>
          <div className="relative">
            <SearchIcon />
            <input
              id="search-input"
              type="text"
              value={inputValue}
              onChange={(e) => {
                setInputValue(e.target.value);
                setVisibleCount(PAGE_SIZE);
              }}
              placeholder="Search emails by content, topic, or sender..."
              className="w-full rounded-xl border border-gray-200 bg-white py-3 pl-10 pr-4 text-sm text-gray-900 placeholder-gray-400 shadow-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500 dark:border-gray-700 dark:bg-gray-800 dark:text-white dark:placeholder-gray-500"
              aria-label="Search emails"
            />
            {isLoading && (
              <div
                className="absolute right-3 top-1/2 h-4 w-4 -translate-y-1/2 animate-spin rounded-full border-2 border-gray-300 border-t-indigo-600"
                aria-label="Loading"
              />
            )}
          </div>
        </div>

        {/* Mode toggle */}
        <div className="mb-4 flex items-center gap-2" role="group" aria-label="Search mode">
          {SEARCH_MODES.map(({ value, label }) => (
            <button
              key={value}
              type="button"
              onClick={() => setMode(value)}
              className={`rounded-lg px-3 py-1.5 text-sm font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 ${
                mode === value
                  ? 'bg-indigo-600 text-white'
                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'
              }`}
              aria-pressed={mode === value}
            >
              {label}
            </button>
          ))}
          {data && debouncedQuery.length > 0 && (
            <span className="ml-auto text-xs text-gray-400 dark:text-gray-500">
              {data.total} result{data.total !== 1 ? 's' : ''} in {data.latencyMs}ms
            </span>
          )}
        </div>

        {/* Error state */}
        {isError && (
          <div
            className="rounded-xl border border-red-200 bg-red-50 p-4 text-sm text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400"
            role="alert"
          >
            Failed to load search results:{' '}
            {error instanceof Error ? error.message : 'Unknown error'}
          </div>
        )}

        {/* Empty state */}
        {!isLoading && debouncedQuery.length > 0 && data && data.results.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <EmptyIcon />
            <h3 className="mt-4 text-lg font-medium text-gray-900 dark:text-white">
              No results found
            </h3>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
              Try adjusting your search terms or filters.
            </p>
          </div>
        )}

        {/* Initial state */}
        {debouncedQuery.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <SearchPromptIcon />
            <h3 className="mt-4 text-lg font-medium text-gray-900 dark:text-white">
              Search your emails
            </h3>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
              Type a query above to search across all your email content using {mode} search.
            </p>
          </div>
        )}

        {/* Results list */}
        {data && data.results.length > 0 && (
          <ul className="space-y-2" role="list" aria-label="Search results">
            {data.results.map((result) => (
              <SearchResultItem key={result.emailId} result={result} query={debouncedQuery} />
            ))}
          </ul>
        )}

        {/* Load more */}
        {hasMore && (
          <div className="mt-4 text-center">
            <button
              type="button"
              onClick={loadMore}
              className="rounded-lg border border-gray-200 bg-white px-6 py-2 text-sm font-medium text-gray-700 shadow-sm transition-colors hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600"
            >
              Load more results
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

/* Individual search result row */

interface SearchResultItemProps {
  result: SearchResult;
  query: string;
}

function SearchResultItem({ result, query }: SearchResultItemProps) {
  const subject = result.metadata?.subject ?? `Email ${result.emailId}`;
  const sender = result.metadata?.from_addr ?? result.metadata?.from ?? 'Unknown';
  const date = result.metadata?.date ?? '';
  const scorePercent = Math.round(result.score * 100);

  return (
    <li>
      <a
        href={`/email?id=${encodeURIComponent(result.emailId)}`}
        className="block rounded-xl border border-gray-200 bg-white p-4 shadow-sm transition-all hover:border-indigo-300 hover:shadow-md focus:outline-none focus:ring-2 focus:ring-indigo-500 dark:border-gray-700 dark:bg-gray-800 dark:hover:border-indigo-600"
      >
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0 flex-1">
            <h4 className="truncate text-sm font-semibold text-gray-900 dark:text-white">
              <HighlightText text={subject} highlight={query} />
            </h4>
            <p className="mt-0.5 truncate text-xs text-gray-500 dark:text-gray-400">
              <HighlightText text={sender} highlight={query} />
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <MatchTypeBadge matchType={result.matchType} />
            {date && <time className="text-xs text-gray-400 dark:text-gray-500">{date}</time>}
          </div>
        </div>

        {/* Relevance score bar */}
        <div className="mt-3 flex items-center gap-2">
          <div
            className="h-1.5 flex-1 overflow-hidden rounded-full bg-gray-100 dark:bg-gray-700"
            role="meter"
            aria-label={`Relevance: ${scorePercent}%`}
            aria-valuenow={scorePercent}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div
              className={`h-full rounded-full transition-all ${
                scorePercent >= 80
                  ? 'bg-green-500'
                  : scorePercent >= 50
                    ? 'bg-yellow-500'
                    : 'bg-gray-400'
              }`}
              style={{ width: `${scorePercent}%` }}
            />
          </div>
          <span className="text-xs text-gray-400 dark:text-gray-500">{scorePercent}%</span>
        </div>
      </a>
    </li>
  );
}

/* Highlight matching text in results */

interface HighlightTextProps {
  text: string;
  highlight: string;
}

function HighlightText({ text, highlight }: HighlightTextProps) {
  if (!highlight || highlight.length === 0) {
    return <>{text}</>;
  }

  const escapedHighlight = highlight.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const regex = new RegExp(`(${escapedHighlight})`, 'gi');
  const parts = text.split(regex);

  return (
    <>
      {parts.map((part, index) => {
        const isMatch = regex.test(part);
        regex.lastIndex = 0; // Reset regex state
        return isMatch ? (
          <mark
            key={index}
            className="rounded bg-yellow-200 px-0.5 text-inherit dark:bg-yellow-700/50"
          >
            {part}
          </mark>
        ) : (
          <span key={index}>{part}</span>
        );
      })}
    </>
  );
}

/* Match type badge */

function MatchTypeBadge({ matchType }: { matchType: string }) {
  const colorMap: Record<string, string> = {
    semantic: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-300',
    keyword: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300',
    hybrid: 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-300',
  };

  const colors =
    colorMap[matchType.toLowerCase()] ??
    'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300';

  return (
    <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${colors}`}>
      {matchType}
    </span>
  );
}

/* Icons */

function SearchIcon() {
  return (
    <svg
      className="absolute left-3 top-1/2 h-5 w-5 -translate-y-1/2 text-gray-400"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
      />
    </svg>
  );
}

function EmptyIcon() {
  return (
    <svg
      className="h-16 w-16 text-gray-300 dark:text-gray-600"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={1}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
      />
    </svg>
  );
}

function SearchPromptIcon() {
  return (
    <svg
      className="h-16 w-16 text-gray-300 dark:text-gray-600"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={1}
      aria-hidden="true"
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
      />
    </svg>
  );
}
