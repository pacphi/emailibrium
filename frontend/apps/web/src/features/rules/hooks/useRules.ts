import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getRules,
  createRule,
  updateRule,
  deleteRule,
  getRuleSuggestions,
  validateRule,
  testRule,
  runRule,
} from '@emailibrium/api';
import type { RuleValidationResult, RuleTestResult, RunRuleResult } from '@emailibrium/api';
import type { Rule, RuleSuggestion } from '@emailibrium/types';

export function useRulesQuery() {
  return useQuery<Rule[]>({
    queryKey: ['rules'],
    queryFn: getRules,
    staleTime: 30_000,
  });
}

export function useRuleSuggestionsQuery() {
  return useQuery<RuleSuggestion[]>({
    queryKey: ['ruleSuggestions'],
    queryFn: () => getRuleSuggestions(),
    staleTime: 60_000,
  });
}

export function useCreateRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: createRule,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rules'] });
    },
  });
}

export function useUpdateRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, rule }: { id: string; rule: Partial<Omit<Rule, 'id' | 'createdAt'>> }) =>
      updateRule(id, rule),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rules'] });
    },
  });
}

export function useDeleteRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteRule,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rules'] });
    },
  });
}

export function useToggleRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, isActive }: { id: string; isActive: boolean }) =>
      updateRule(id, { isActive }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rules'] });
    },
  });
}

export function useValidateRule() {
  return useMutation<RuleValidationResult, Error, Omit<Rule, 'id' | 'matchCount' | 'createdAt'>>({
    mutationFn: validateRule,
  });
}

export function useTestRule() {
  return useMutation<RuleTestResult, Error, Omit<Rule, 'id' | 'matchCount' | 'createdAt'>>({
    mutationFn: testRule,
  });
}

export function useRunRule() {
  const queryClient = useQueryClient();
  return useMutation<RunRuleResult, Error, string>({
    mutationFn: runRule,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['rules'] });
    },
  });
}
