import type { SearchQuery, SearchResponse } from '@emailibrium/types';
import { api } from './client.js';

export async function searchEmails(query: SearchQuery): Promise<SearchResponse> {
  return api.post('search', { json: query }).json<SearchResponse>();
}

export async function findSimilar(emailId: string): Promise<SearchResponse> {
  return api.get(`search/similar/${emailId}`).json<SearchResponse>();
}

export async function classifyEmail(request: {
  emailId: string;
  category?: string;
}): Promise<{ category: string; confidence: number }> {
  return api
    .post('search/classify', { json: request })
    .json<{ category: string; confidence: number }>();
}
