export type SearchMode = 'hybrid' | 'semantic' | 'keyword';

export interface SearchQuery {
  text: string;
  mode: SearchMode;
  filters?: SearchFilters;
  limit?: number;
}

export interface SearchFilters {
  dateFrom?: string;
  dateTo?: string;
  senders?: string[];
  categories?: string[];
  hasAttachment?: boolean;
  isRead?: boolean;
  accounts?: string[];
}

export interface SearchResult {
  emailId: string;
  score: number;
  matchType: string;
  vectorRank?: number;
  ftsRank?: number;
  metadata: Record<string, string>;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  latencyMs: number;
  mode: SearchMode;
}
