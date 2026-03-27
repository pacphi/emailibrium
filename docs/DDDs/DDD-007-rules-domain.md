# DDD-007: Rules Domain (Core)

| Field   | Value        |
| ------- | ------------ |
| Status  | Accepted     |
| Date    | 2026-03-24   |
| Type    | Core Domain  |
| Context | Rules Engine |

## Overview

The Rules bounded context manages user-defined automation rules that act on emails. It owns the rule lifecycle (create, validate, test, execute), the hybrid condition model (structural + semantic), and the priority-ordered evaluation engine. This is a **core domain** because automated rule processing is a primary user-facing capability that directly drives inbox management.

## Strategic Classification

| Aspect              | Value                                                     |
| ------------------- | --------------------------------------------------------- |
| Domain type         | Core                                                      |
| Investment priority | High (primary inbox automation feature)                   |
| Complexity driver   | Semantic condition evaluation, contradiction detection    |
| Change frequency    | Medium (new condition types, new action types)            |
| Risk                | False positive rule matches, conflicting rules, data loss |

---

## Aggregates

### 1. RuleAggregate

Manages the lifecycle and configuration of a single automation rule.

**Root Entity: Rule**

| Field         | Type               | Description                                       |
| ------------- | ------------------ | ------------------------------------------------- |
| id            | RuleId             | Unique rule identifier                            |
| name          | String             | Human-readable rule name                          |
| description   | Option\<String\>   | Optional explanation of rule purpose              |
| priority      | u32                | Evaluation order (lower number = higher priority) |
| conditions    | ConditionGroup     | Tree of AND/OR condition clauses                  |
| actions       | Vec\<Action\>      | Ordered list of actions to execute on match       |
| enabled       | bool               | Whether the rule is active                        |
| match_count   | u64                | Number of emails this rule has matched            |
| last_match_at | Option\<DateTime\> | Timestamp of most recent match                    |
| created_at    | DateTime           | When the rule was created                         |
| updated_at    | DateTime           | When the rule was last modified                   |

**Invariants:**

- Rule names must be unique per user.
- Priority values must be unique across enabled rules. Inserting a rule at an existing priority shifts lower-priority rules down.
- A rule must have at least one condition and at least one action.
- Semantic conditions require a pre-computed embedding vector. If the embedding model changes (ADR-013), semantic conditions are re-embedded.
- Deleting a rule that is referenced by another rule's exception list is forbidden until the reference is removed.

**Commands:**

- `CreateRule { name, priority, conditions, actions }` -- creates a new rule with validation
- `UpdateRule { rule_id, name, priority, conditions, actions }` -- modifies an existing rule
- `DeleteRule { rule_id }` -- removes a rule permanently
- `ValidateRule { rule_id }` -- checks for contradictions, unreachable conditions, and circular dependencies against all other enabled rules
- `TestRule { rule_id, sample_size }` -- runs the rule against a sample of existing emails and returns match results without executing actions
- `EnableRule { rule_id }` -- activates a disabled rule
- `DisableRule { rule_id }` -- deactivates a rule without deleting it

---

## Domain Events

| Event            | Fields                                                  | Published When                                 |
| ---------------- | ------------------------------------------------------- | ---------------------------------------------- |
| RuleCreated      | rule_id, name, priority, condition_count, action_count  | New rule successfully created and validated    |
| RuleUpdated      | rule_id, changed_fields                                 | Existing rule modified                         |
| RuleDeleted      | rule_id, name                                           | Rule permanently removed                       |
| RuleEnabled      | rule_id                                                 | Disabled rule activated                        |
| RuleDisabled     | rule_id                                                 | Active rule deactivated                        |
| RuleMatched      | rule_id, email_id, matched_conditions, actions_executed | Rule matched an email and actions were applied |
| RuleTestResult   | rule_id, sample_size, match_count, sample_matches       | TestRule completed with results                |
| ValidationFailed | rule_id, violations                                     | ValidateRule found contradictions or issues    |

### Event Consumers

| Event       | Consumed By        | Purpose                                         |
| ----------- | ------------------ | ----------------------------------------------- |
| RuleMatched | Email Intelligence | Updates email metadata with applied actions     |
| RuleMatched | Ingestion          | Executes provider-side actions (archive, label) |
| RuleMatched | Learning           | Feeds rule match data into SONA for adaptation  |
| RuleCreated | Search             | Indexes rule for rule search/discovery          |

---

## Value Objects

### ConditionGroup

```rust
struct ConditionGroup {
    operator: LogicalOperator,  // AND | OR
    clauses: Vec<Clause>,
}
```

### Clause

```rust
enum Clause {
    Structural {
        field: EmailField,       // subject, sender, category, age_days, size_bytes, has_attachment, label
        op: ComparisonOp,        // eq, neq, contains, not_contains, gt, lt, gte, lte, matches_regex
        value: ClauseValue,      // String, Number, Boolean
    },
    Semantic {
        description: String,     // Natural language description (e.g., "emails about project budgets")
        embedding: Vec<f32>,     // Pre-computed embedding of the description
        threshold: f32,          // Cosine similarity threshold (0.0-1.0, default 0.75)
    },
    Nested(ConditionGroup),      // Allows arbitrary nesting of AND/OR groups
}
```

### Action

```rust
enum Action {
    Label { label: String },
    RemoveLabel { label: String },
    Archive,
    Delete,
    MarkRead,
    MarkUnread,
    Star,
    Unstar,
    Move { folder: String },
    Forward { to: EmailAddress },
}
```

---

## Domain Services

### RuleParser

Converts natural language rule descriptions into structured `ConditionGroup` and `Action` definitions.

**Responsibilities:**

- Parses NL input using generative AI (Tier 1/2) or template matching (Tier 0 fallback)
- Extracts structural conditions from explicit criteria ("from: alice@example.com", "older than 7 days")
- Extracts semantic conditions from fuzzy criteria ("about project updates", "marketing content")
- Maps action verbs to `Action` variants ("archive", "label as important", "move to archive")
- Returns a parsed rule structure for user confirmation before creation

### RuleValidator

Validates a rule against the full set of enabled rules to detect conflicts.

**Checks performed:**

- **Contradiction detection**: Two rules with overlapping conditions but conflicting actions (e.g., one archives, another stars the same emails)
- **Circular dependency**: Rule A's action triggers conditions that match Rule B, whose action triggers conditions that match Rule A
- **Shadowing**: A higher-priority rule matches a strict superset of a lower-priority rule's conditions, making the lower rule unreachable
- **Semantic overlap**: Two semantic conditions with high embedding similarity (>0.9) but different thresholds, indicating potential redundancy

### RuleProcessor

Evaluates all enabled rules against incoming emails in priority order.

**Responsibilities:**

- Loads enabled rules sorted by priority at startup and on `RuleCreated`/`RuleUpdated`/`RuleDeleted` events
- For each incoming email, evaluates conditions top-to-bottom
- Short-circuits: stops evaluating a rule's conditions on first non-matching clause (AND) or first matching clause (OR)
- Executes matched actions in order
- Configurable: stop-on-first-match (default) or evaluate-all-rules
- Emits `RuleMatched` event for each match

---

## Context Map Integration

```text
Email Intelligence --[Published Language]--> Rules
  Events: EmailEmbedded (provides embedding for semantic condition evaluation)
  Purpose: Rules uses existing email embeddings for semantic matching

Ingestion --[Customer/Supplier]--> Rules
  Direction: Ingestion triggers rule evaluation after email processing
  Purpose: Newly ingested emails are evaluated against active rules

Rules --[Published Language]--> Ingestion
  Events: RuleMatched (with provider-side actions: archive, label, delete)
  Purpose: Ingestion executes provider-side actions on the email server

Rules --[Published Language]--> Learning
  Events: RuleMatched
  Purpose: Learning tracks rule effectiveness for SONA optimization

AI Providers --[Customer/Supplier]--> Rules
  Direction: Rules (customer) uses AI Providers for NL parsing and semantic embedding
  Purpose: NL parser and condition embedding generation
```

---

## Ubiquitous Language

| Term                     | Definition                                                                                           |
| ------------------------ | ---------------------------------------------------------------------------------------------------- |
| **Rule**                 | A user-defined automation that matches emails by conditions and applies actions                      |
| **Structural condition** | A condition that matches on exact email field values (sender, subject, age, category)                |
| **Semantic condition**   | A condition that matches on vector similarity between an email embedding and a description embedding |
| **Priority**             | A numeric ordering that determines which rules are evaluated first (lower = higher priority)         |
| **Contradiction**        | Two rules with overlapping conditions that apply conflicting actions to the same emails              |
| **Shadowing**            | A higher-priority rule that fully subsumes a lower-priority rule, making it unreachable              |
| **Rule match**           | When all conditions in a rule evaluate to true for a given email                                     |

---

## Boundaries

- This context does NOT generate email embeddings. That belongs to **Email Intelligence**.
- This context does NOT execute provider-side email operations (Gmail API, Outlook API). That belongs to **Ingestion** (triggered by `RuleMatched` events).
- This context does NOT manage SONA learning or feedback. That belongs to **Learning**.
- This context DOES own:
  - Rule CRUD and lifecycle management
  - Condition parsing (NL and structured)
  - Rule validation (contradictions, loops, shadowing)
  - Rule evaluation against emails
  - Semantic condition embedding management
  - Rule test execution
