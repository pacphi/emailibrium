# DDD-004: Learning Domain (SONA Adaptive Learning)

| Field | Value |
|-------|-------|
| Status | Accepted |
| Date | 2026-03-23 |
| Type | Supporting Domain |
| Context | Learning |

## Overview

The Learning bounded context implements SONA (Self-Organizing Neural Adaptation), the adaptive learning system that personalizes Emailibrium to each user. It operates in three tiers -- instant feedback, session accumulation, and long-term consolidation -- to continuously improve classification accuracy and search relevance. It consumes feedback signals from Search and Email Intelligence, and publishes centroid updates back to Email Intelligence.

## Aggregates

### 1. LearningModelAggregate

Manages the SONA 3-tier learning state for a user.

**Root Entity: LearningModel**

| Field | Type | Description |
|-------|------|-------------|
| user_id | UserId | The user this model belongs to |
| tier1_state | Tier1State | Instant learning state (live centroid adjustments) |
| tier2_session | Tier2Session | Current session preference accumulator |
| tier3_weights | Tier3Weights | Long-term consolidated weights |
| last_consolidated | DateTime | Last Tier 3 consolidation timestamp |
| feedback_count | u32 | Total feedback events processed |
| active_since | DateTime | When the model was first created |

**Tier1State:**

| Field | Type | Description |
|-------|------|-------------|
| centroid_adjustments | HashMap\<Category, CentroidDelta\> | Pending centroid adjustments |
| pending_count | u32 | Adjustments awaiting minimum threshold |

**Tier2Session:**

| Field | Type | Description |
|-------|------|-------------|
| session_id | SessionId | Current session identifier |
| preference_vector | SessionPreference | Accumulated preference from this session |
| interaction_count | u32 | Interactions in this session |
| started_at | DateTime | Session start time |

**Tier3Weights:**

| Field | Type | Description |
|-------|------|-------------|
| category_centroids | HashMap\<Category, CategoryCentroid\> | Consolidated centroids |
| search_weights | Vec\<f32\> | SONA re-ranking weights |
| snapshots | Vec\<CentroidSnapshot\> | Historical snapshots for rollback |

**Invariants:**
- Minimum 10 feedback events before Tier 1 centroid updates take effect (prevents premature adaptation).
- Centroid drift must not exceed 20% from the initial position (alarm threshold -- see CentroidDriftAlarm event).
- Tier 2 session consolidation occurs when a session ends or after 30 minutes of inactivity.
- Tier 3 consolidation runs on a configurable schedule (default: hourly micro, daily full).
- 10% of queries must remain unaffected by SONA (control group for A/B evaluation).

**Commands:**
- `ProcessFeedback { email_id, action, embedding }` -- processes a user feedback signal
- `ConsolidateSession { session_id }` -- consolidates a session into Tier 2
- `ConsolidateLongTerm {}` -- runs Tier 3 consolidation
- `RollbackCentroids { snapshot_date }` -- rolls back to a previous centroid snapshot
- `ResetModel { user_id }` -- resets the learning model to default state

### 2. FeedbackAggregate

Captures and processes user feedback signals.

**Root Entity: UserFeedback**

| Field | Type | Description |
|-------|------|-------------|
| id | FeedbackId | Unique feedback identifier |
| email_id | EmailId | The email the feedback relates to |
| action | FeedbackAction | What the user did |
| embedding | Option\<EmbeddingVector\> | The email's embedding at feedback time |
| quality_score | FeedbackQuality | Computed quality of this feedback signal |
| timestamp | DateTime | When the feedback was recorded |

**Invariants:**
- Each feedback event must have a valid FeedbackAction.
- Quality score is computed at creation time based on the action type (explicit reclassification has higher quality than implicit archive).
- Feedback with quality_score < 0.1 is recorded but not used for centroid updates (noise filtering).

**Commands:**
- `RecordFeedback { email_id, action }` -- records a new feedback signal
- `BatchRecordFeedback { feedbacks }` -- records multiple feedback signals from a session

## Domain Events

| Event | Fields | Published When |
|-------|--------|----------------|
| FeedbackReceived | email_id, action, quality_score | User feedback is recorded |
| CentroidUpdated | category, shift_magnitude, new_position_hash | A category centroid is adjusted |
| SessionConsolidated | session_id, interactions_count | A session's preferences are consolidated |
| LongTermConsolidated | centroids_updated, clusters_refreshed | Tier 3 consolidation completes |
| CentroidDriftAlarm | category, drift_percentage | A centroid has drifted beyond the 20% threshold |
| LearningRolledBack | snapshot_date, reason | Centroids rolled back to a previous state |

### Event Consumers

| Event | Consumed By | Purpose |
|-------|-------------|---------|
| CentroidUpdated | Email Intelligence | Updates classification centroids |
| LongTermConsolidated | Email Intelligence | Triggers reclassification of borderline emails |
| CentroidDriftAlarm | Monitoring / Alerting | Alerts operators to potential degenerate learning |

### Events Consumed (from other contexts)

| Event | Published By | Purpose |
|-------|-------------|---------|
| SearchResultClicked | Search | Implicit relevance signal |
| SearchFeedbackProvided | Search | Explicit relevance signal |
| EmailClassified | Email Intelligence | Observes classification outcomes |
| ClassificationCorrected | Email Intelligence | Observes corrections for centroid adjustment |

## Value Objects

### FeedbackAction

```
enum FeedbackAction {
    Reclassify { from: Category, to: Category },  -- Explicit category correction
    MoveToGroup { group_id: GroupId },             -- Move email to a user group
    Star,                                          -- Star/flag an email
    Reply { delay_secs: u32 },                     -- Reply (delay indicates importance)
    Archive,                                       -- Archive an email
    Delete,                                        -- Delete an email
}
```

### FeedbackQuality

```
FeedbackQuality(f32) -- bounded [0.0, 1.0]
```

Computed based on action type:

| Action | Quality Score | Rationale |
|--------|-------------|-----------|
| Reclassify | 1.0 | Strongest explicit signal |
| MoveToGroup | 0.8 | Strong organizational signal |
| Star | 0.6 | Moderate importance signal |
| Reply (< 5 min) | 0.5 | Moderate urgency signal |
| Reply (> 5 min) | 0.3 | Weak urgency signal |
| Archive | 0.2 | Weak "not important" signal |
| Delete | 0.4 | Moderate "unwanted" signal |

### SessionPreference

| Field | Type | Description |
|-------|------|-------------|
| vector | Vec\<f32\> | Accumulated preference direction in embedding space |
| interaction_count | u32 | Number of interactions contributing to this vector |

### CentroidSnapshot

| Field | Type | Description |
|-------|------|-------------|
| category | Category | The category |
| vector | Vec\<f32\> | The centroid vector at snapshot time |
| timestamp | DateTime | When the snapshot was taken |

### LearningRate

```
LearningRate(f32)
```

Per ADR-004:
- Positive feedback (Reclassify, Star, Reply): alpha = 0.05
- Negative feedback (Archive, Delete): beta = 0.02

The asymmetric rates prevent negative feedback from destabilizing centroids too quickly.

## Domain Services

### InstantLearner (Tier 1)

Applies immediate centroid adjustments on explicit user feedback.

**Algorithm (EMA -- Exponential Moving Average):**
```
new_centroid = (1 - alpha) * current_centroid + alpha * feedback_embedding
```

**Responsibilities:**
- Processes FeedbackReceived events in real-time.
- Applies EMA update only after minimum feedback threshold (10 events) is met.
- Uses asymmetric learning rates (alpha for positive, beta for negative).
- Emits CentroidUpdated event after each adjustment.
- Guards against zero-vector feedback embeddings.

### SessionAccumulator (Tier 2)

Computes a session preference vector from accumulated interactions.

**Responsibilities:**
- Tracks all user interactions within a session (clicks, feedback, time spent).
- Computes a weighted preference vector: `pref = sum(quality_i * embedding_i) / sum(quality_i)`.
- Consolidates session preference into Tier 1 state when session ends.
- Handles session timeout (30 minutes of inactivity).
- Emits SessionConsolidated event.

### LongTermConsolidator (Tier 3)

Runs periodic consolidation jobs to stabilize the learning model.

**Schedule:**
- Hourly micro-consolidation: smooths Tier 1 adjustments, removes noise.
- Daily full consolidation: rebuilds centroids from all feedback, updates search weights.

**Responsibilities:**
- Aggregates all Tier 1 and Tier 2 updates since last consolidation.
- Applies decay to older feedback (recent feedback weighted more heavily).
- Recomputes category centroids from the full feedback history.
- Updates SONA search re-ranking weights.
- Takes centroid snapshot before applying changes.
- Emits LongTermConsolidated event.

### DegenerateDetector

Identifies and mitigates degenerate learning patterns.

**Detected Patterns:**
| Pattern | Detection | Mitigation |
|---------|-----------|------------|
| Position bias | Top-ranked results always clicked regardless of query | Weight clicks by 1/log(rank) |
| Feedback loop | Same emails repeatedly reinforced | Cap per-email feedback influence |
| Centroid collapse | Multiple centroids converging to same point | Trigger CentroidDriftAlarm |
| Category starvation | A category receiving no feedback, centroid going stale | Protect stale centroids from drift |

**Responsibilities:**
- Runs analysis on every Tier 3 consolidation.
- Emits CentroidDriftAlarm if any category drifts beyond 20%.
- Recommends rollback if degenerate patterns are severe.

### CentroidSnapshotManager

Manages daily centroid snapshots with rollback capability.

**Responsibilities:**
- Takes automatic snapshot before every Tier 3 consolidation.
- Retains snapshots for configurable duration (default: 30 days).
- Supports rollback to any snapshot date.
- Validates snapshot integrity (vector dimensions, category coverage).
- Emits LearningRolledBack event on rollback.

## Invariants Summary

| Invariant | Threshold | Action |
|-----------|-----------|--------|
| Minimum feedback before updates | 10 events | Buffer until threshold met |
| Maximum centroid drift | 20% from initial | CentroidDriftAlarm + operator review |
| Control group percentage | 10% of queries | Exclude from SONA re-ranking |
| Session timeout | 30 minutes inactivity | Auto-consolidate session |
| Snapshot retention | 30 days | Auto-prune older snapshots |

## Context Map

### Events Consumed

| Source Context | Events | Purpose |
|---------------|--------|---------|
| Search | SearchResultClicked, SearchFeedbackProvided | Implicit and explicit relevance signals |
| Email Intelligence | EmailClassified, ClassificationCorrected | Classification outcomes and corrections |

### Events Published

| Target Context | Events | Purpose |
|---------------|--------|---------|
| Email Intelligence | CentroidUpdated, LongTermConsolidated | Updated centroids for improved classification |
| Monitoring | CentroidDriftAlarm, LearningRolledBack | Operational alerts |

## Ubiquitous Language

| Term | Definition |
|------|------------|
| **SONA** | Self-Organizing Neural Adaptation -- the 3-tier adaptive learning system |
| **Tier 1** | Instant learning -- real-time centroid adjustments on explicit feedback |
| **Tier 2** | Session learning -- accumulated preference vector per user session |
| **Tier 3** | Long-term learning -- periodic consolidation and weight optimization |
| **Centroid drift** | How far a category centroid has moved from its initial position |
| **Feedback quality** | A score indicating how informative a user action is for learning |
| **Consolidation** | The process of aggregating accumulated feedback into stable model weights |
| **Rollback** | Reverting centroids to a previous snapshot to undo degenerate learning |
| **Control group** | The subset of queries not affected by SONA, used for A/B evaluation |
| **Degenerate learning** | Pathological patterns where the learning system degrades rather than improves |

## Boundaries

- This context does NOT execute search queries or display results (that is Search). It only consumes search interaction events.
- This context does NOT directly modify the classification engine (that is Email Intelligence). It publishes centroid updates that Intelligence consumes.
- This context does NOT manage user accounts or authentication (that is Account Management).
- This context DOES own the SONA learning model, feedback processing, centroid management, and degenerate detection.
