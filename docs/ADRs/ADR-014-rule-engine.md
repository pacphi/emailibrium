# ADR-014: Hybrid Rule Engine with Semantic Conditions

- **Status**: Accepted
- **Date**: 2026-03-24
- **Implements**: R-03 (Predecessor Recommendations)
- **Related**: ADR-004 (Adaptive Learning), DDD-007 (Rules Domain)

## Context

Emailibrium's Rules Studio UI supports AI-suggested semantic conditions, but there is no backend rule execution engine. The predecessor repository provides a complete structural rule engine (parser, validator, processor), while the current repo has unique vector similarity infrastructure. Users need automated email actions ("archive newsletters older than 7 days", "star emails about budgets from finance team") that combine deterministic structural matching with fuzzy semantic matching.

## Decision

Implement a hybrid rule engine that supports both structural conditions (field-based matching with operators) and semantic conditions (vector similarity against a natural language description). Rules are defined as JSON, parsed from natural language via an NL parser, validated for contradictions and loops, and executed by a priority-ordered processor.

### Rule Structure

```json
{
  "id": "rule-uuid",
  "name": "Archive old newsletters",
  "priority": 10,
  "conditions": {
    "operator": "AND",
    "clauses": [
      { "type": "structural", "field": "category", "op": "eq", "value": "newsletter" },
      { "type": "structural", "field": "age_days", "op": "gt", "value": 7 },
      { "type": "semantic", "description": "marketing or promotional content", "threshold": 0.75 }
    ]
  },
  "actions": [{ "type": "archive" }, { "type": "label", "value": "auto-archived" }],
  "enabled": true
}
```

### Key Components

1. **NL Parser**: Converts natural language ("archive newsletters older than a week") into the JSON condition structure using the generative AI tier (Ollama/Cloud) with ONNX fallback to template matching.
2. **Rule Validator**: Detects contradictions (two rules with conflicting actions on overlapping conditions), circular dependencies (rule A triggers rule B which triggers rule A), and unreachable rules (lower-priority rule fully shadowed by higher-priority one).
3. **Rule Processor**: Evaluates rules in priority order against incoming emails. Structural conditions use field comparisons. Semantic conditions compute cosine similarity between the email embedding and a pre-computed condition embedding, compared against the configured threshold.
4. **Test Runner**: Allows users to test a rule against a sample of existing emails before activation (`TestRule` command).

### Semantic Condition Evaluation

Semantic conditions embed the `description` field using the active embedding provider (ADR-002) at rule creation time. At evaluation time, the pre-computed condition vector is compared against the email's existing embedding via cosine similarity. This adds zero embedding cost per email evaluation since email embeddings already exist in the vector store.

## Consequences

### Positive

- Combines deterministic rules (exact field matching) with fuzzy semantic rules (vector similarity), which is more powerful than regex-only or keyword-only engines
- Semantic conditions like "emails about budgets from finance team" work without users specifying exact keywords
- Pre-computed condition embeddings mean rule evaluation adds only a cosine similarity computation per semantic clause (sub-millisecond)
- NL parser lowers the barrier to rule creation for non-technical users
- Validator prevents common rule authoring errors before they affect the inbox

### Negative

- Semantic conditions depend on embedding quality; low-quality embeddings produce unreliable matches
- NL parser requires generative AI (Tier 1+); Tier 0 users are limited to structural conditions or template-based NL parsing
- Condition embedding must be recomputed if the embedding model changes (tied to ADR-013 reindex lifecycle)
- Rule validator adds complexity; contradiction detection for semantic conditions is approximate (based on embedding similarity between condition descriptions)

## Alternatives Considered

### Structural-Only Rule Engine (Predecessor Pattern)

- **Pros**: Simpler, fully deterministic, no AI dependency
- **Cons**: Cannot express fuzzy conditions ("emails about project updates"), limited to exact field matching
- **Verdict**: Rejected. The vector infrastructure exists; not using it for rules wastes the platform's core differentiator.

### LLM-Only Rule Evaluation

- **Pros**: Maximum flexibility, understands any natural language rule
- **Cons**: Expensive per-email LLM call, high latency, non-deterministic, unavailable at Tier 0
- **Verdict**: Rejected. Pre-computed embeddings with cosine similarity achieve similar semantic matching at negligible cost.
