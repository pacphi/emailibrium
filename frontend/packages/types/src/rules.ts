export interface Rule {
  id: string;
  name: string;
  conditions: RuleCondition[];
  actions: RuleAction[];
  isActive: boolean;
  matchCount: number;
  accuracy: number;
  createdAt: string;
}

export interface RuleCondition {
  field: string;
  operator: string;
  value: string;
}

export interface RuleAction {
  type: string;
  value?: string;
}

export interface RuleSuggestion {
  rule: Rule;
  reason: string;
  estimatedMatches: number;
}
