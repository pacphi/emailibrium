# DDD-002: Search Domain

| Field   | Value       |
| ------- | ----------- |
| Status  | Accepted    |
| Date    | 2026-03-23  |
| Type    | Core Domain |
| Context | Search      |

## Overview

The Search bounded context handles user-initiated queries against the email corpus. It orchestrates hybrid search (FTS5 + HNSW vector), applies SONA-learned re-ranking, tracks user interactions with results, and publishes interaction events consumed by the Learning domain.

## Aggregates

### 1. SearchQueryAggregate

Represents a user search with mode, filters, and result set.

**Root Entity: SearchQuery**

| Field        | Type                | Description                        |
| ------------ | ------------------- | ---------------------------------- |
| id           | SearchQueryId       | Unique query identifier            |
| text         | String              | Raw query text entered by the user |
| mode         | SearchMode          | Hybrid, Semantic, or Keyword       |
| filters      | SearchFilters       | Applied filters                    |
| results      | Vec\<ScoredResult\> | Ordered result set                 |
| result_count | u32                 | Total results returned             |
| latency_ms   | u64                 | End-to-end query latency           |
| created_at   | DateTime            | Query timestamp                    |

**Invariants:**

- Query text must not be empty (minimum 1 character after trimming).
- Filters are validated at construction: date ranges must have start <= end; sender addresses must be syntactically valid.
- Result set is immutable after query execution; re-queries produce new SearchQuery instances.

**Commands:**

- `ExecuteSearch { text, mode, filters }` -- runs the search pipeline
- `RefineSearch { query_id, additional_filters }` -- creates a new query with added filters

### 2. SearchInteractionAggregate

Tracks user interactions with search results for SONA learning.

**Root Entity: SearchInteraction**

| Field           | Type                     | Description                                |
| --------------- | ------------------------ | ------------------------------------------ |
| id              | InteractionId            | Unique interaction identifier              |
| query_id        | SearchQueryId            | The originating query                      |
| result_email_id | EmailId                  | The email the user interacted with         |
| rank            | u32                      | Position of the result in the list         |
| clicked         | bool                     | Whether the user clicked/opened the result |
| feedback        | Option\<SearchFeedback\> | Explicit relevance feedback (if provided)  |
| interacted_at   | DateTime                 | Interaction timestamp                      |

**Invariants:**

- An interaction must reference an existing SearchQuery.
- Rank must be within the bounds of the query's result set.
- Feedback is optional; most interactions only record click data.

**Commands:**

- `RecordClick { query_id, email_id, rank }` -- records a click on a result
- `ProvideFeedback { query_id, email_id, feedback }` -- records explicit relevance feedback

## Domain Events

| Event                  | Fields                                             | Published When                            |
| ---------------------- | -------------------------------------------------- | ----------------------------------------- |
| SearchExecuted         | query_id, mode, result_count, latency_ms           | A search query completes                  |
| SearchResultClicked    | query_id, email_id, rank                           | User clicks a search result               |
| SearchFeedbackProvided | query_id, email_id, feedback (relevant/irrelevant) | User provides explicit relevance feedback |

### Event Consumers

| Event                  | Consumed By | Purpose                            |
| ---------------------- | ----------- | ---------------------------------- |
| SearchExecuted         | Learning    | Query volume and mode analytics    |
| SearchResultClicked    | Learning    | Implicit relevance signal for SONA |
| SearchFeedbackProvided | Learning    | Explicit relevance signal for SONA |

## Value Objects

### SearchFilters

| Field          | Type                | Description                       |
| -------------- | ------------------- | --------------------------------- |
| date_range     | Option\<DateRange\> | Start and end dates               |
| senders        | Vec\<EmailAddress\> | Filter by sender                  |
| labels         | Vec\<Label\>        | Filter by label                   |
| categories     | Vec\<Category\>     | Filter by classification category |
| has_attachment | Option\<bool\>      | Attachment presence filter        |
| is_read        | Option\<bool\>      | Read status filter                |
| accounts       | Vec\<AccountId\>    | Filter by email account           |

### ScoredResult

| Field      | Type             | Description                                  |
| ---------- | ---------------- | -------------------------------------------- |
| email_id   | EmailId          | The matched email                            |
| score      | f32              | Final fused score                            |
| match_type | MatchType        | Vector, Keyword, or Both                     |
| highlights | Vec\<Highlight\> | Text snippets with matched terms highlighted |

### SearchMode

```
enum SearchMode {
    Hybrid,    -- FTS5 + HNSW with RRF fusion (default)
    Semantic,  -- HNSW vector search only
    Keyword,   -- FTS5 full-text search only
}
```

### RRFScore

```
RRFScore(f32)
```

A Reciprocal Rank Fusion score computed as `sum(1 / (k + rank))` across multiple result lists. The constant k defaults to 60.

### SearchFeedback

```
enum SearchFeedback {
    Relevant,
    Irrelevant,
}
```

## Domain Services

### QueryEmbedder

Embeds search query text on-the-fly for vector search.

**Responsibilities:**

- Generates a query embedding using the same model as email embeddings for consistency.
- Caches recent query embeddings to avoid redundant computation on repeated queries.
- Handles query expansion (optional): enriches short queries with contextual terms.

### ResultFuser

Fuses vector search and FTS5 keyword search results using Reciprocal Rank Fusion.

**Algorithm:**

1. Receive ranked lists from vector search and keyword search.
2. For each email appearing in any list, compute `RRF_score = sum(1 / (60 + rank_in_list))`.
3. Sort by descending RRF score.
4. Deduplicate (same email from multiple collections).
5. Return fused, scored result set.

### SONAReranker

Applies learned SONA weights to re-rank search results.

**Responsibilities:**

- Fetches current SONA weights from the Learning context (via read model or cache).
- Applies user-specific preference vectors as a re-ranking signal.
- Maintains a control group: 10% of queries are NOT re-ranked (for A/B evaluation).
- Falls back to RRF-only scoring if SONA weights are unavailable.

### MultiCollectionSearcher

Searches across multiple vector collections and merges results.

**Collections searched:**

- `email_text` -- email body text embeddings
- `image_text` -- OCR text from inline images
- `image_visual` -- CLIP visual embeddings from images
- `attachment_text` -- extracted text from attachments (PDF, DOCX, XLSX)

**Responsibilities:**

- Parallel search across all relevant collections.
- Collection-weighted scoring (email_text has highest weight by default).
- Cross-collection deduplication (same email, different assets).

## Context Map

### Upstream Dependencies

| Context            | Dependency                                | What Search Consumes                                                                                                |
| ------------------ | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Email Intelligence | Vector store, embeddings, classifications | Uses EmbeddingPipeline to embed queries; uses VectorStore for similarity search; uses FTS5 index for keyword search |

### Downstream Consumers

| Context  | Relationship       | What Search Publishes                                                |
| -------- | ------------------ | -------------------------------------------------------------------- |
| Learning | Published Language | SearchResultClicked, SearchFeedbackProvided events for SONA training |

## Ubiquitous Language

| Term                | Definition                                                                         |
| ------------------- | ---------------------------------------------------------------------------------- |
| **Hybrid search**   | A search combining FTS5 keyword matching and HNSW vector similarity, fused via RRF |
| **Semantic search** | Vector-only search based on meaning rather than exact keywords                     |
| **Keyword search**  | Traditional full-text search using FTS5                                            |
| **RRF**             | Reciprocal Rank Fusion -- a method for combining multiple ranked lists             |
| **Re-rank**         | Adjust result ordering using learned SONA weights after initial fusion             |
| **Collection**      | A named partition of the vector store (email_text, image_text, etc.)               |
| **Control group**   | The 10% of queries not affected by SONA re-ranking, used for evaluation            |

## Boundaries

- This context does NOT generate embeddings for emails (that is Email Intelligence). It only embeds search queries on-the-fly.
- This context does NOT own the vector store or FTS5 index (those are owned by Email Intelligence). It queries them as a consumer.
- This context does NOT process feedback into model updates (that is Learning). It only publishes interaction events.
- This context DOES own query execution, result fusion, re-ranking, and interaction tracking.
