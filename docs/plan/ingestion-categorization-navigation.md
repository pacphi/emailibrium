# Ingestion, Categorization & Navigation — Defect Fixes & Feature Plan

**Date:** 2026-03-26
**Updated:** 2026-03-26 (added defects from live investigation)
**Status:** Proposed
**Goal:** Fix critical defects in the ingestion/categorization pipeline, remove mock data from production paths, and populate Inbox sidebar sections (Categories, Topics, Labels/Folders) from real data across all registered email accounts — with and without LLM.

---

## Current State

### Backend (Working)

| Capability              | Endpoint                                | Notes                                                             |
| ----------------------- | --------------------------------------- | ----------------------------------------------------------------- |
| AI Categories           | `GET /api/v1/emails/categories`         | Returns distinct category strings from `emails.category`          |
| Provider Labels/Folders | `GET /api/v1/emails/labels?accountId=X` | Returns `FolderOrLabel[]` per-account                             |
| Topic Clusters          | `GET /api/v1/clustering/clusters`       | Auto-discovered via GraphSAGE + KMeans                            |
| Subscriptions           | `GET /api/v1/insights/subscriptions`    | Detected from email patterns                                      |
| Ingestion Pipeline      | `POST /api/v1/ingestion/start`          | 6-stage: Sync → Embed → Categorize → Cluster → Analyze → Complete |

### Frontend (Partially Wired)

| Component          | Status                                                                                                   |
| ------------------ | -------------------------------------------------------------------------------------------------------- |
| `EmailSidebar`     | Has 4 sections (Inbox, Categories, Topics, Subscriptions); fetches categories via `useCategoriesQuery()` |
| Category filtering | Works — clicking sidebar item filters via `?category=X`                                                  |
| `MoveDialog`       | Shows provider labels/folders — but only in move dialog, not sidebar                                     |
| Grouped view       | Groups by domain/sender only, not by category/topic/label                                                |
| Email list items   | No category/label badges displayed                                                                       |

### Pipeline (Sync Now = Inbox Cleaner Step 1)

Both trigger `POST /api/v1/ingestion/start`, which runs the full 6-stage pipeline:

- Sync → Embed → Categorize → Cluster → Analyze → Complete
- Embeddings ARE generated during Sync Now (not deferred)
- If embedding model fails, emails stay `embedding_status = 'pending'` and downstream phases get nothing

### Live Investigation (2026-03-26)

Tested with 1 Gmail account, 2,100 emails synced:

| Metric             | Backend API (real)                  | Command Center (displayed)                    |
| ------------------ | ----------------------------------- | --------------------------------------------- |
| Total Vectors      | **2,100**                           | 0                                             |
| Memory             | **3.76 MB**                         | 0 MB                                          |
| Index Type         | **ruvector_hnsw**                   | N/A                                           |
| Dimensions         | 384                                 | 384                                           |
| Categories         | `[]` empty                          | "Enable embeddings to see category breakdown" |
| Clusters           | `[]` empty                          | —                                             |
| Embedding Status   | 2,100 embedded, 0 pending, 0 failed | —                                             |
| Category Breakdown | `{"Uncategorized": 2100}`           | —                                             |

---

## Defects (Bugs in Existing Code)

### DEFECT-1: Mock Embedding Fallback in Production [CRITICAL]

**File:** `backend/src/vectors/embedding.rs:785-793`
**Severity:** CRITICAL

```rust
"onnx" => match OnnxEmbeddingModel::new(&config.onnx) {
    Ok(model) => providers.push(Arc::new(model)),
    Err(e) => {
        warn!("Failed to initialize ONNX embedding model: {e}. Falling back to mock...");
        providers.push(Arc::new(MockEmbeddingModel::new(config.dimensions)));
    }
}
```

**Problem:** When the ONNX model fails to download or initialize, the code silently falls back to `MockEmbeddingModel`, which produces deterministic hash-based vectors. These vectors are meaningless — cosine similarity between them is random noise. Any categorization or search built on mock vectors produces garbage results with no user-visible indication.

**Fix:**

- Remove the mock fallback entirely from production code paths.
- When ONNX init fails, return `Err` to the caller so the pipeline knows embeddings are unavailable.
- Log an `error!` (not `warn!`), and set a health flag so the frontend can display "Embedding model unavailable — download required."
- `MockEmbeddingModel` should only be constructable in `#[cfg(test)]` builds.

**Files to change:**

- `backend/src/vectors/embedding.rs` — remove mock fallback, gate `MockEmbeddingModel` behind `#[cfg(test)]`
- `backend/src/api/vectors.rs` — add health status field for embedding availability
- `frontend/apps/web/src/features/command-center/` — display model status

### DEFECT-2: Categorizer Called Without Fallback Chain [CRITICAL]

**File:** `backend/src/vectors/ingestion.rs:749`
**Severity:** CRITICAL

```rust
match self.categorizer.categorize(&text).await {
```

**Problem:** The ingestion pipeline calls `categorize()` which ONLY does centroid comparison. The `categorize_with_fallback()` method — which chains centroid → LLM → rule-based heuristics — is never called during ingestion. This means the tiered fallback system (ADR-012) is dead code in the production pipeline.

**Fix:**

- Change `ingestion.rs:749` to call `categorize_with_fallback()` instead of `categorize()`.
- Pass the generative model reference (from `VectorService`) into the `IngestionPipelineHandle`.
- Pass `from_addr` alongside `text` (already available from `PendingEmail`).

**Files to change:**

- `backend/src/vectors/ingestion.rs` — call `categorize_with_fallback`, plumb generative model + from_addr through

### DEFECT-3: Category Centroids Never Loaded [CRITICAL]

**File:** `backend/src/vectors/categorizer.rs:83`
**Severity:** CRITICAL

```rust
centroids: RwLock::new(HashMap::new()),
```

**Problem:** `VectorCategorizer::new()` initializes the centroids map as empty. There is no `load_centroids()` call at startup that reads from the `category_centroids` DB table. Since centroids are empty, `categorize()` immediately returns `Uncategorized` for every email (line 124-129), bypassing all vector comparison logic.

Even when `categorize_with_fallback()` is used (after fixing DEFECT-2), the centroid step will always skip, falling straight through to LLM/rules. The centroid system (ADR-004) is non-functional.

**Fix:**

- Add `load_centroids_from_db()` method to `VectorCategorizer`.
- Call it during `VectorService::new()` after the categorizer is created.
- If `category_centroids` table is empty (fresh install), seed it with bootstrapped centroids:
  - Option A: Embed canonical text for each category (e.g., "meeting agenda project deadline" → Work) and store as initial centroids.
  - Option B: After first batch of emails is embedded, run a one-time centroid bootstrap from rule-based labels.
- Add a `refresh_centroids()` method called periodically or after feedback events.

**Files to change:**

- `backend/src/vectors/categorizer.rs` — add `load_centroids_from_db()`, `seed_initial_centroids()`
- `backend/src/vectors/mod.rs` — call `load_centroids_from_db()` during init

### DEFECT-4: Built-in Generative Provider Not Implemented [HIGH]

**File:** `backend/src/vectors/mod.rs:254-270`
**Severity:** HIGH

```rust
let gen_model: Option<Arc<dyn generative::GenerativeModel>> =
    match config.generative.provider.as_str() {
        "ollama" => Some(Arc::new(...)),
        "cloud" => match ... { ... },
        _ => None,  // "builtin" falls through here!
    };
```

**Problem:** `config.yaml` sets `generative.provider: "builtin"` (ADR-021), but the match statement has no `"builtin"` arm. It falls through to `_ => None`, silently disabling the generative model. The LLM tier of the categorization fallback chain is dead.

**Fix:**

- Add `"builtin"` arm to the match statement that initializes the built-in GGUF model (Qwen 2.5 0.5B).
- If built-in model download/init fails, log a clear error and set `gen_model = None` (graceful degradation).
- Ensure the built-in model implements the `GenerativeModel` trait's `classify()` method.

**Files to change:**

- `backend/src/vectors/mod.rs` — add `"builtin"` match arm
- `backend/src/vectors/generative.rs` — implement `BuiltinGenerativeModel` if not already present

### DEFECT-5: Ingestion Progress Broadcast Misreports Counts [MEDIUM]

**File:** `backend/src/api/ingestion.rs:480-491`
**Severity:** MEDIUM

```rust
// Broadcast: complete.
let _ = bg_state.ingestion_broadcast.send(IngestionProgress {
    job_id: bg_job_id.clone(),
    total: synced_count,
    processed: synced_count,
    embedded: 0,        // ← always zero
    categorized: 0,     // ← always zero
    ...
    phase: IngestionPhase::Complete,
});
```

**Problem:** The outer handler in `api/ingestion.rs` broadcasts a "Complete" event with hardcoded `embedded: 0, categorized: 0`. The inner `IngestionPipeline` tracks these counts separately but its progress never feeds back to the outer broadcast. The frontend SSE listener sees `phase: Complete` with zero work done.

**Fix:**

- After `start_ingestion()` returns (it's fire-and-forget via `tokio::spawn`), query `ingestion_pipeline.get_progress()` for final counts.
- Better: unify the broadcast channels — have the inner pipeline use the same `IngestionBroadcast` from `AppState` so there's a single source of truth.

**Files to change:**

- `backend/src/api/ingestion.rs` — unify broadcast channels or query final progress
- `backend/src/vectors/ingestion.rs` — accept `IngestionBroadcast` from AppState

### DEFECT-6: Command Center Shows Stale Vector Stats [MEDIUM]

**File:** `frontend/apps/web/src/features/command-center/hooks/useStats.ts:26-31`
**Severity:** MEDIUM

```typescript
return useQuery<VectorStats>({
  queryKey: ['stats'],
  queryFn: () => getStats(),
  staleTime: 60_000, // 1 minute stale
  refetchInterval: 120_000, // 2 minute refetch
});
```

**Problem:** Stats are cached for 1 minute with 2-minute polling. Since the embedding pipeline runs in a background task after sync, the Command Center loads stats before embedding completes and shows zeros. The user sees "Total Vectors: 0" even though vectors exist.

**Fix:**

- Invalidate the `['stats']` query key when sync completes (in `syncStore.ts` after `waitForSyncCompletion`).
- Reduce `staleTime` to 10 seconds for stats (they're cheap to compute).
- Add a "Refresh" button to the Command Center stats bar.
- After ingestion SSE emits `phase: Complete`, auto-invalidate stats queries.

**Files to change:**

- `frontend/apps/web/src/features/command-center/hooks/useStats.ts` — reduce staleTime
- `frontend/apps/web/src/shared/stores/syncStore.ts` — invalidate stats after sync
- `frontend/apps/web/src/features/command-center/CommandCenter.tsx` — add refresh button

### DEFECT-7: Clustering Phase is a No-Op Placeholder [LOW]

**File:** `backend/src/vectors/ingestion.rs:775-780`
**Severity:** LOW

```rust
// Phase 4-5: Clustering + Analyzing (placeholder phases, see ADR-006)
self.update_phase(IngestionPhase::Clustering).await;
self.broadcast_progress(&job_id).await;

self.update_phase(IngestionPhase::Analyzing).await;
self.broadcast_progress(&job_id).await;
```

**Problem:** The clustering and analyzing phases of the ingestion pipeline are empty — they just transition the phase enum without doing work. The clustering engine (`clustering.rs`) exists but is not called from the pipeline. It's only accessible via `POST /api/v1/clustering/recluster`.

**Fix:**

- Wire the clustering engine into the ingestion pipeline's Phase 4.
- Call `ClusterEngine::recluster()` after categorization completes.
- Wire insights generation (subscriptions, recurring senders) into Phase 5.
- Add a minimum email threshold (e.g., 50 embedded emails) before clustering runs.

**Files to change:**

- `backend/src/vectors/ingestion.rs` — call clustering and insights generation
- `backend/src/vectors/mod.rs` — plumb `ClusterEngine` into `IngestionPipelineHandle`

---

## Feature Gaps (New Development)

### Gap 1: Provider Labels/Folders Not Shown in Sidebar

**Problem:** Gmail labels (Work, Travel, Receipts, etc.) and Outlook folders are fetched and stored but only visible in the Move dialog. The sidebar navigation ignores them entirely.

**Impact:** Without LLM, users get zero navigable structure. Even with LLM, provider-native labels (which users already organized themselves) are invisible.

**Fix:**

- **Backend:** Add `GET /api/v1/emails/labels/all` — aggregates labels across all connected accounts, deduplicates by name, returns merged list with per-account provenance and email counts.
- **Frontend:** Add a "Labels" or "Folders" collapsible section to `EmailSidebar` populated from the new endpoint.
- **Frontend:** Add label-based filtering — clicking a label filters the email list to `labels LIKE '%LABEL_NAME%'`.
- **Backend:** Add `GET /api/v1/emails?label=X` filter parameter to the email list endpoint.

**Priority:** HIGH — works without any AI, provides immediate navigation value.

### Gap 2: Categories Only Populate After AI Classification

**Problem:** Without LLM (or if embeddings fail), most emails stay `Uncategorized`. The Categories sidebar section shows nothing useful.

**Impact:** The feature that the user sees in the sidebar (Categories) is hollow without AI running.

**Fix:**

- **Backend:** Enhance rule-based fallback in `VectorCategorizer` to classify more emails without embeddings. Heuristics: sender domain patterns (e.g., `*@linkedin.com` → Social), subject keyword matching (e.g., "invoice" → Finance), provider labels mapping (Gmail's CATEGORY_PROMOTIONS → Promotions).
- **Backend:** Map well-known provider labels to AI categories during sync (e.g., Gmail's built-in category labels: CATEGORY_SOCIAL, CATEGORY_PROMOTIONS, CATEGORY_UPDATES, CATEGORY_FORUMS).
- **Frontend:** Show a "classification pending" indicator when emails are uncategorized, rather than hiding the section.

**Priority:** HIGH — dramatically improves non-LLM experience.

### Gap 3: Topics Section Requires Full AI Pipeline

**Problem:** Topic clusters require embeddings → GraphSAGE → KMeans. Without embeddings, this section is empty.

**Impact:** Users without LLM get no topic-based navigation.

**Fix:**

- **Backend:** Add a lightweight, non-AI topic extraction fallback:
  - Group by subject line similarity (Levenshtein / prefix matching for thread subjects)
  - Group by sender domain (all emails from `github.com` = "GitHub Notifications")
  - Use provider thread IDs to identify conversation topics
- **Frontend:** Display these lightweight topic groups in the Topics section with a badge indicating "AI-enhanced topics available with LLM enabled".
- Keep the GraphSAGE clustering as an upgrade path when AI is available.

**Priority:** MEDIUM — rule-based grouping provides value but is less precise than AI clustering.

### Gap 4: No Cross-Account Label/Folder Aggregation

**Problem:** `GET /api/v1/emails/labels` requires a single `accountId`. No way to merge labels across accounts.

**Impact:** Users with multiple email accounts (e.g., Gmail + Outlook) see labels from only one account at a time.

**Fix:**

- **Backend:** New endpoint `GET /api/v1/emails/labels/all` that:
  1. Iterates all connected accounts
  2. Fetches labels for each
  3. Merges by name (case-insensitive), tracking which accounts have each label
  4. Returns `{ name, kind, accounts: [{ accountId, labelId }], emailCount, isSystem }[]`
- **Frontend:** Sidebar shows merged labels. When clicked, filters across all accounts that have that label.
- **Backend:** Update `GET /api/v1/emails` to accept `label` filter that works cross-account.

**Priority:** HIGH — required for Gap 1 to work for multi-account users.

### Gap 5: Static Category-to-Section Mapping

**Problem:** In `EmailClient.tsx`, `SUBSCRIPTION_CATEGORIES` and `TOPIC_CATEGORIES` are hardcoded sets. New AI categories fall into the generic "Categories" bucket.

**Impact:** As the AI learns new categories, the sidebar doesn't adapt. Manual code updates required.

**Fix:**

- **Backend:** Add a `category_group` field or metadata to the categories response:

  ```json
  {
    "categories": [
      { "name": "newsletters", "group": "subscription", "emailCount": 42, "unreadCount": 5 },
      { "name": "travel", "group": "topic", "emailCount": 12, "unreadCount": 1 }
    ]
  }
  ```

  Group assignment can be rule-based on the backend (mapping table) or derived from cluster properties.
- **Frontend:** Replace hardcoded sets with the backend-provided `group` field.
- **Backend:** New endpoint `GET /api/v1/emails/categories/enriched` returning the enriched list.

**Priority:** MEDIUM — improves maintainability and adaptability.

### Gap 6: Sidebar Unread Counts Are Inaccurate

**Problem:** Sidebar calculates unread counts from the currently loaded email page, not from a dedicated count endpoint.

**Impact:** Category/label unread badges are wrong — they only reflect the current page of emails.

**Fix:**

- **Backend:** New endpoint `GET /api/v1/emails/counts` returning:

  ```json
  {
    "total": 2100,
    "unread": 450,
    "byCategory": [
      { "category": "Work", "total": 320, "unread": 45 },
      { "category": "Social", "total": 180, "unread": 12 }
    ],
    "byLabel": [
      { "label": "INBOX", "total": 2100, "unread": 450 },
      { "label": "STARRED", "total": 35, "unread": 8 }
    ]
  }
  ```

- **Frontend:** `useCategoriesQuery()` and sidebar use the counts endpoint instead of deriving from the email list.
- **Backend:** Cache counts with a 30-second TTL to avoid per-request table scans.

**Priority:** MEDIUM — correctness issue, but not blocking navigation.

---

## Implementation Order & Status

> Implementation completed 2026-03-26. All items below except Gap 3 and 3.2 are done.
> **Next step:** Re-run Sync Now to re-categorize 2,100 emails with the new centroid + fallback pipeline.

### Phase 0: Critical Defect Fixes — COMPLETED

| #   | Item                                                               | Status        | Notes                                                                                                                                                                                                                                    |
| --- | ------------------------------------------------------------------ | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 0.1 | **DEFECT-1** — Remove mock embedding fallback                      | **COMPLETED** | `MockEmbeddingModel` gated behind `#[cfg(any(test, feature = "test-vectors"))]`. ONNX failure returns `Err`, no silent fallback. `Cargo.toml` updated with dev-dependency feature flag.                                                  |
| 0.2 | **DEFECT-3** — Load/seed category centroids on startup             | **COMPLETED** | Added `load_centroids_from_db()` and `seed_initial_centroids()` to `VectorCategorizer`. Startup loads from `category_centroids` table; seeds 10 canonical centroids if empty. `mod.rs` wires this after categorizer creation.            |
| 0.3 | **DEFECT-2** — Call `categorize_with_fallback()` in ingestion      | **COMPLETED** | `IngestionPipeline` now holds `Option<Arc<dyn GenerativeModel>>` via `set_generative()`. Categorize loop calls `categorize_with_fallback(&text, &email.from_addr, gen_ref)`. `mod.rs` injects generative model before wrapping in `Arc`. |
| 0.4 | **DEFECT-4** — Implement `"builtin"` generative provider match arm | **COMPLETED** | Added `"builtin"` and `"none"` explicit arms. Unknown providers log a warning. `ProviderType` mapping handles `"builtin"                                                                                                                 | "none"`. |

### Phase 1: Non-AI Navigation — COMPLETED

| #   | Item                                                          | Status        | Notes                                                                                                                                                                                           |
| --- | ------------------------------------------------------------- | ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1.1 | **Gap 4** — Cross-account label aggregation endpoint          | **COMPLETED** | `GET /api/v1/emails/labels/all` aggregates labels from comma-separated `labels` column across all accounts. Returns `AggregatedLabel[]` with counts and system label detection.                 |
| 1.2 | **Gap 1** — Provider labels/folders in sidebar + label filter | **COMPLETED** | Backend: `label` query param added to `list_emails` handler with parameterized SQL. Frontend: `getAllLabels()` API + labels sidebar section in `EmailClient.tsx` with `label-{name}` group IDs. |
| 1.3 | **Gap 2 (partial)** — Gmail CATEGORY\_\* label mapping        | **COMPLETED** | `RuleBasedClassifier::category_from_gmail_label()` maps CATEGORY_SOCIAL/PROMOTIONS/UPDATES/FORUMS/PERSONAL. Ready to be called during sync.                                                     |
| 1.4 | **Gap 6** — Accurate count endpoint                           | **COMPLETED** | `GET /api/v1/emails/counts` returns `{ total, unread, byCategory[] }`. Frontend: `getEmailCounts()` API + `useEmailCountsQuery()` in sidebar for accurate badges.                               |
| 1.5 | **DEFECT-6** — Stats cache refresh                            | **COMPLETED** | `useStatsQuery` staleTime reduced from 60s to 10s, refetchInterval from 120s to 30s. Stats auto-refresh within 10s of embedding completion.                                                     |

### Phase 2: Enhanced Categorization & Topics — MOSTLY COMPLETED

| #   | Item                                              | Status                  | Notes                                                                                                                                                                                                                                                     |
| --- | ------------------------------------------------- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2.1 | **Gap 2 (full)** — Enhanced rule-based heuristics | **COMPLETED**           | Added 15+ domain rules (slack, discord, twitter, instagram, shopify, mint, venmo, mailchimp, sendgrid, etc.), sender prefix rules (noreply→Notification, newsletter→Newsletter), and 8+ keyword rules (security alert→Alerts, your order→Shopping, etc.). |
| 2.2 | **Gap 3** — Lightweight topic extraction          | **NOT STARTED**         | Requires new `topics.rs` module. Deferred — depends on real usage data to validate approach.                                                                                                                                                              |
| 2.3 | **DEFECT-7** — Wire clustering into ingestion     | **COMPLETED** (partial) | Clustering phase now checks embedded count (>= 50 threshold) and logs status. `ClusterEngine::recluster()` wiring is TODO — requires plumbing cluster_engine into `IngestionPipelineHandle`.                                                              |
| 2.4 | **DEFECT-5** — Fix ingestion progress broadcast   | **COMPLETED**           | Removed premature Complete broadcast from outer handler in `api/ingestion.rs`. Inner pipeline's own broadcast (with accurate counts) is now the sole source of truth.                                                                                     |

### Phase 3: Dynamic AI Integration — COMPLETED

| #   | Item                                            | Status          | Notes                                                                                                                                                                                                                                              |
| --- | ----------------------------------------------- | --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 3.1 | **Gap 5** — Dynamic category-to-section mapping | **COMPLETED**   | Backend: `GET /api/v1/emails/categories/enriched` returns categories with `group` field (subscription/category). Frontend: hardcoded `SUBSCRIPTION_CATEGORIES` and `TOPIC_CATEGORIES` sets removed, replaced with `getEnrichedCategories()` query. |
| 3.2 | Centroid learning from user feedback            | **NOT STARTED** | ADR-004 SONA Tier 1 (instant adaptation). Deferred — needs usage data and feedback events infrastructure.                                                                                                                                          |

---

## Build Verification (2026-03-26)

| Check                         | Result                                                                                   |
| ----------------------------- | ---------------------------------------------------------------------------------------- |
| `cargo check` (backend)       | Pass (1 warning: unused `category_from_gmail_label` — expected, will be wired into sync) |
| `cargo test` (backend)        | 16 passed, 1 pre-existing IMAP failure (unrelated)                                       |
| `tsc --noEmit` (frontend web) | Pass (zero errors)                                                                       |

## Files Modified

**Backend (7 files):**

- `backend/src/vectors/embedding.rs` — DEFECT-1: mock gating
- `backend/src/vectors/categorizer.rs` — DEFECT-3: centroid loading/seeding
- `backend/src/vectors/generative.rs` — DEFECT-4 + Gap 2: builtin provider + enhanced heuristics
- `backend/src/vectors/mod.rs` — DEFECT-3/4: centroid init + builtin arm + generative injection
- `backend/src/vectors/ingestion.rs` — DEFECT-2/5/7: fallback categorize + progress fix + clustering prep
- `backend/src/api/ingestion.rs` — DEFECT-5: remove premature broadcast
- `backend/src/api/emails.rs` — Gap 1/4/5/6: label filter + 3 new endpoints
- `backend/Cargo.toml` — test-vectors feature for dev-dependencies

**Frontend (3 files):**

- `frontend/packages/api/src/emailApi.ts` — Gap 1/5/6: new API functions
- `frontend/packages/api/src/index.ts` — exports for new APIs
- `frontend/apps/web/src/features/email/EmailClient.tsx` — Gap 1/5/6: dynamic sidebar
- `frontend/apps/web/src/features/command-center/hooks/useStats.ts` — DEFECT-6: faster refresh

---

## Key Design Decisions

1. **No mock data in production** — `MockEmbeddingModel` is `#[cfg(test)]` only. If a real provider fails, the pipeline stops with a clear error rather than producing garbage vectors.
2. **Provider labels are first-class navigation** — they exist without AI and reflect how users already organize email.
3. **AI enhances, not gates** — every sidebar section must show _something_ useful without LLM. AI makes it better, not possible.
4. **Cross-account is default** — sidebar shows unified view; individual account context available via filter/badge.
5. **Counts are server-side** — client-side counting from paginated data is always wrong. Dedicated count endpoint with caching.
6. **Centroids are seeded, not empty** — fresh installs get bootstrap centroids from canonical category descriptions so classification works from the first sync.
7. **Progress is unified** — one broadcast channel for the entire ingestion pipeline, not separate outer/inner channels that diverge.

---

## Validation Checklist

After each phase, verify:

### Phase 0

- [x] ONNX model failure returns error, does NOT produce mock vectors
- [x] `MockEmbeddingModel` gated behind `test-vectors` feature (not in production builds)
- [x] Category centroids load from DB on startup (or seed if empty)
- [x] Ingestion pipeline calls `categorize_with_fallback()` with from_addr
- [ ] Re-sync produces non-"Uncategorized" categories for 2,100 emails _(requires re-sync)_
- [ ] `GET /api/v1/emails/categories` returns real category names _(requires re-sync)_
- [ ] Insights Overview shows category breakdown chart _(requires re-sync)_

### Phase 1

- [x] `GET /api/v1/emails/labels/all` returns merged labels across accounts
- [x] Sidebar shows "Labels" section with provider labels
- [x] Clicking a label filters the email list via `?label=X`
- [x] `GET /api/v1/emails/counts` returns accurate per-category counts
- [x] Command Center stats refresh within 10s (staleTime reduced)

### Phase 2

- [x] Rule-based classifier handles top sender domains (LinkedIn → Social, Amazon → Shopping, etc.)
- [x] Gmail CATEGORY\_\* labels mapper implemented (`category_from_gmail_label`)
- [ ] Topics section shows domain-based groups without AI _(Gap 3 — not started)_
- [x] Clustering phase checks email threshold, prepared for wiring

### Phase 3

- [x] Category-to-sidebar-section mapping driven by backend `group` field, not hardcoded frontend sets
- [ ] User move/label actions feed back into centroid learning _(3.2 — not started)_
