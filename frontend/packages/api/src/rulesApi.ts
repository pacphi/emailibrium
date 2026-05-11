import type { Rule, RuleSuggestion } from '@emailibrium/types';
import { api } from './client.js';

export async function getRules(): Promise<Rule[]> {
  return api.get('rules').json<Rule[]>();
}

export async function getRule(id: string): Promise<Rule> {
  return api.get(`rules/${id}`).json<Rule>();
}

export async function createRule(
  rule: Omit<Rule, 'id' | 'matchCount' | 'createdAt'>,
): Promise<Rule> {
  return api.post('rules', { json: rule }).json<Rule>();
}

export async function updateRule(
  id: string,
  rule: Partial<Omit<Rule, 'id' | 'createdAt'>>,
): Promise<Rule> {
  return api.put(`rules/${id}`, { json: rule }).json<Rule>();
}

export async function deleteRule(id: string): Promise<void> {
  await api.delete(`rules/${id}`);
}

export async function getRuleSuggestions(offset = 0): Promise<RuleSuggestion[]> {
  return api.get('rules/suggestions', { searchParams: { offset } }).json<RuleSuggestion[]>();
}

export interface RuleValidationResult {
  valid: boolean;
  errors: Array<{ field: string; message: string }>;
  warnings: Array<{ field: string; message: string }>;
}

export async function validateRule(
  rule: Omit<Rule, 'id' | 'matchCount' | 'createdAt'>,
): Promise<RuleValidationResult> {
  return api.post('rules/validate', { json: rule }).json<RuleValidationResult>();
}

export interface RuleTestResult {
  matchCount: number;
  sampleMatches: Array<{
    emailId: string;
    subject: string;
    from: string;
    receivedAt: string;
  }>;
}

export interface RunRuleResult {
  matchCount: number;
  executedCount: number;
  sampleMatches: Array<{
    emailId: string;
    subject: string;
    from: string;
    receivedAt: string;
  }>;
}

export async function testRule(
  rule: Omit<Rule, 'id' | 'matchCount' | 'createdAt'>,
): Promise<RuleTestResult> {
  return api.post('rules/test', { json: rule }).json<RuleTestResult>();
}

export async function runRule(id: string): Promise<RunRuleResult> {
  return api.post(`rules/${id}/run`).json<RunRuleResult>();
}
