import type { SearchQuery, SearchResponse } from '@emailibrium/types';
import { api } from './client.js';

export async function searchEmails(query: SearchQuery): Promise<SearchResponse> {
  // Backend expects { query, mode, filters, limit } with field name "query" (not "text").
  const body = {
    query: query.text,
    mode: query.mode,
    filters: query.filters,
    limit: query.limit,
  };
  return api.post('vectors/search/hybrid', { json: body }).json<SearchResponse>();
}

export async function findSimilar(emailId: string): Promise<SearchResponse> {
  return api
    .post(`vectors/search/similar/${emailId}`, { json: {} })
    .json<SearchResponse>();
}

export async function classifyEmail(request: {
  emailId: string;
  category?: string;
}): Promise<{ category: string; confidence: number }> {
  return api
    .post('vectors/classify', { json: request })
    .json<{ category: string; confidence: number }>();
}
