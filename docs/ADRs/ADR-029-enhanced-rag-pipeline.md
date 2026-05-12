# ADR-029: Enhanced RAG Pipeline — The Default Chat Path

- **Status:** Proposed
- **Date:** 2026-04-04
- **Deciders:** Chris Phillipson
- **Context:** ADR-028 introduces MCP tool-calling for the chat. However, the default setup (Qwen3-1.7B, no cloud keys) operates in RAG-only mode. This ADR proposes enhancements to make the RAG-only path significantly more powerful — capable of handling diverse natural language queries across 100k+ emails with high accuracy, without requiring tool-calling models.

---

## 1. Problem Statement

The current RAG pipeline has several limitations:

1. **No query understanding**: "emails from Alice last week about the budget" is treated as a single search string — no structured filters are extracted
2. **Flat BM25 scoring**: FTS5 uses unweighted `rank` — a subject match scores the same as a body match
3. **No re-ranking**: RRF fusion is the final scoring step — no cross-encoder to refine relevance
4. **Aggressive body truncation**: Embedding text truncates at 400 chars, losing 50-80% of email content
5. **No thread awareness**: Multiple emails from the same thread can dominate results
6. **No query type routing**: Aggregation queries ("how many emails from marketing?") go through the same RAG path as factual queries
7. **No temporal parsing**: "last week" and "this month" are treated as keywords, not date filters
8. **Limited hallucination prevention**: The system prompt says "don't make things up" but has no structural enforcement

**Goal:** Make the RAG-only chat path robust enough that users with a 1.7B local model can effectively search, query, and reason about their entire email corpus using natural language.

---

## 2. Architecture: Enhanced RAG Pipeline

```text
                    Natural Language Query
                            │
                    ┌───────▼──────────┐
                    │  Query Under-    │
                    │  standing Layer  │
                    │                  ��
                    │  1. Rule parser  │ ← <1ms: regex, chrono-english
                    │  2. LLM fallback │ ← 300ms: Qwen 3 + GBNF grammar
                    └───────┬──────────┘
                            │
                    ┌───────▼──────────┐
                    │  ParsedQuery     │
                    │  - semantic_text  │
                    │  - fts_keywords   │
                    │  - filters        │
                    │  - query_type     │
                    └───────┬──────────┘
                            │
              ┌─────────────┼─────────────┐
              │             │             │
     ┌────────▼───┐  ┌─────▼─────┐  ┌────▼────────┐
     │ FTS5       │  │ Vector    │  │ SQL Direct  │
     │ (weighted  │  │ (optional │  │ (aggregation│
     │  bm25)     │  │  HyDE)    │  │  queries)   │
     │ pre-filter │  │ post-filt │  │             │
     └────────┬───┘  └─────┬─────┘  └────┬────────┘
              │             │             │
              └──────┬──────┘             │
                     │                    │
              ┌──────▼──────┐             │
              │ Weighted    │             │
              │ RRF Fusion  │             │
              │ (adaptive k │             │
              │  + weights) │             │
              └──────┬──────┘             │
                     │                    │
              ┌──────▼──────┐             │
              │ Cross-Enc.  │             │
              │ Re-ranker   │             │
              │ (BGE ONNX)  │             │
              └──────┬──────┘             │
                     │                    │
              ┌──────▼──────┐             │
              │ Thread      │             │
              │ Collapse    │             │
              └──────┬──────┘             │
                     │                    │
              ┌──────▼──────┐      ┌──────▼──────┐
              │ Extractive  │      │ SQL Result  │
              │ Context     │      │ Formatting  │
              │ Builder     │      │             │
              └──────┬──────┘      └──────┬──────┘
                     │                    │
                     └────────┬───────────┘
                              │
                     ┌────────▼────────┐
                     │  LLM Generation │
                     │  (mandatory     │
                     │   citation)     │
                     └─────────────────┘
```

---

## 3. Component Details

### 3.1 Query Understanding Layer

A two-tier parser translates natural language into structured queries.

**Tier 1: Rule-Based Parser (<1ms)**

Handles ~70-80% of queries with perfect reliability:

```rust
/// Parsed result from the query understanding layer.
pub struct ParsedQuery {
    /// Semantic text for vector search (structured parts stripped)
    pub semantic_text: Option<String>,
    /// Keywords for FTS5 search
    pub fts_keywords: Option<String>,
    /// Structured filters extracted from the query
    pub filters: SearchFilters,
    /// Type of query (determines routing strategy)
    pub query_type: QueryType,
    /// Confidence score from parser (0.0-1.0)
    pub parse_confidence: f32,
    /// Which parser produced this result
    pub parse_source: ParseSource,
}

pub enum QueryType {
    /// "what did Alice say about the budget?" → hybrid search + re-rank
    Factual,
    /// "find the email with invoice #12345" → FTS5-dominant
    NeedleInHaystack,
    /// "recent emails about project X" → date-filtered + recency boost
    Temporal,
    /// "how many emails from marketing this month?" → SQL aggregation
    Aggregation,
    /// "emails from Alice OR Bob about Q2" → boolean parse + search
    Boolean,
    /// Ambiguous / conversational → full hybrid with HyDE
    Semantic,
}

pub enum ParseSource {
    RuleBased,
    LlmConstrained,
    Hybrid,
}
```

Rule-based patterns:

| Pattern                                 | Extraction                      | Example                 |
| --------------------------------------- | ------------------------------- | ----------------------- |
| `from:X` or "from X", "sent by X"       | `filters.senders`               | "emails from Alice"     |
| `to:X` or "to X"                        | `filters.recipients` (new)      | "emails I sent to Bob"  |
| `subject:X` or "about X", "regarding X" | `fts_keywords` (subject-scoped) | "about the budget"      |
| `has:attachment`, "with attachments"    | `filters.has_attachment = true` | "PDFs from marketing"   |
| `is:unread`, "unread"                   | `filters.is_read = false`       | "unread emails"         |
| `label:X`, "starred", "important"       | `filters.categories`            | "important emails"      |
| Temporal expressions                    | `filters.date_from/to`          | "last week", "in March" |
| "how many", "count", "total"            | `query_type = Aggregation`      | "how many from Alice?"  |
| Quoted phrases                          | FTS5 phrase match               | `"quarterly report"`    |

**Temporal Resolution** via `chrono-english` + custom range handler:

```rust
// New dependency: chrono-english = "0.1"
fn resolve_temporal(expr: &str, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    match expr.to_lowercase().as_str() {
        "today" => Some((start_of_day(now), now)),
        "yesterday" => Some((start_of_day(now - Duration::days(1)), end_of_day(now - Duration::days(1)))),
        "this week" => Some((start_of_week(now), now)),
        "last week" => Some((start_of_week(now - Duration::weeks(1)), end_of_week(now - Duration::weeks(1)))),
        "this month" => Some((start_of_month(now), now)),
        "last month" => Some((start_of_prev_month(now), end_of_prev_month(now))),
        "recent" => Some((now - Duration::days(7), now)),
        s if s.starts_with("in ") => resolve_month_name(&s[3..], now),
        s if s.ends_with(" ago") => resolve_relative_ago(s, now),
        _ => chrono_english::parse_date_string(expr, now, Dialect::Us)
                .ok().map(|d| (d, d)),
    }
}
```

**Confidence scoring**: `parse_confidence = matched_tokens / total_tokens`. If <0.6, fall through to LLM.

**Tier 2: LLM Fallback (100-500ms)**

For queries where rules match <60% of tokens, use the local LLM with GBNF grammar-constrained generation:

```bnf
# GBNF grammar for email search query parsing
root      ::= "{" ws members ws "}"
members   ::= pair ("," ws pair)*
pair      ::= key ws ":" ws value
key       ::= "\"semantic_query\"" | "\"from_contains\"" | "\"subject_contains\""
            | "\"date_after\"" | "\"date_before\"" | "\"has_attachment\""
            | "\"is_read\"" | "\"categories\""
value     ::= string | "true" | "false" | "null" | array
string    ::= "\"" [^"\\]* "\""
array     ::= "[" ws string ("," ws string)* ws "]" | "[" ws "]"
ws        ::= [ \t\n]*
```

Grammar-constrained generation guarantees 100% syntactically valid JSON. The `gbnf` crate converts JSON Schema to GBNF automatically. The LLM produces structured filters; unparsed semantic content is routed to vector search.

### 3.2 FTS5 Optimization

**Weighted BM25 scoring** — the single highest-impact change with minimal effort:

```sql
-- Set persistent rank configuration (run once, persists across connections)
INSERT INTO email_fts(email_fts, rank)
    VALUES('rank', 'bm25(0.0, 10.0, 5.0, 3.0, 1.0, 2.0)');
```

Column weights: `id=0, subject=10, from_name=5, from_addr=3, body_text=1, labels=2`

**Pre-filtered FTS5 queries** when structural filters are present:

```sql
-- Pattern: CTE pre-filter + weighted FTS5
WITH candidates AS (
    SELECT rowid, id FROM emails
    WHERE received_at >= ?1 AND received_at <= ?2
      AND (?3 IS NULL OR from_addr IN (SELECT value FROM json_each(?3)))
)
SELECT c.id, -bm25(email_fts, 0.0, 10.0, 5.0, 3.0, 1.0, 2.0) AS score
FROM email_fts
JOIN candidates c ON email_fts.rowid = c.rowid
WHERE email_fts MATCH ?4
ORDER BY bm25(email_fts, 0.0, 10.0, 5.0, 3.0, 1.0, 2.0)
LIMIT ?5
```

**FTS5 advanced features** to expose:

```sql
-- Column-scoped search: restrict to subject and from_name
WHERE email_fts MATCH '{subject from_name} : meeting agenda'

-- NEAR operator for proximity
WHERE email_fts MATCH 'body_text : NEAR(budget approval, 5)'

-- Phrase matching
WHERE email_fts MATCH '"quarterly report"'
```

**Prefix indexes** for type-ahead (requires FTS5 table recreation):

```sql
-- Migration: recreate FTS5 with prefix indexes
CREATE VIRTUAL TABLE IF NOT EXISTS email_fts USING fts5(
    id, subject, from_name, from_addr, body_text, labels,
    content='emails', content_rowid='rowid',
    tokenize='porter unicode61',
    prefix='2 3'
);
```

### 3.3 Weighted Reciprocal Rank Fusion

Replace the current equal-weight RRF with adaptive weights:

```rust
pub fn weighted_rrf(
    vector_results: &[(String, f32)],
    fts_results: &[(String, f32)],
    k: u32,
    vector_weight: f32,
    fts_weight: f32,
) -> Vec<(String, f32)> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for (rank_0, (id, _)) in vector_results.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) +=
            vector_weight / (k as f32 + (rank_0 + 1) as f32);
    }
    for (rank_0, (id, _)) in fts_results.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) +=
            fts_weight / (k as f32 + (rank_0 + 1) as f32);
    }
    // sort by score descending...
}
```

**Adaptive weight selection** by query type:

| Query Type       | k   | FTS Weight | Vector Weight | Rationale                |
| ---------------- | --- | ---------- | ------------- | ------------------------ |
| NeedleInHaystack | 30  | 1.5        | 0.5           | Exact match matters most |
| Factual          | 40  | 1.0        | 1.0           | Balanced                 |
| Temporal         | 40  | 0.8        | 1.2           | Semantic + date filter   |
| Semantic         | 40  | 0.7        | 1.3           | Concept matching         |
| Boolean          | 30  | 1.3        | 0.7           | Keyword precision        |

### 3.4 Cross-Encoder Re-ranking

The **highest-impact accuracy improvement**: +40-60% across benchmarks.

Add a re-ranking stage between RRF fusion and final result delivery:

```rust
// New dependency: fastembed = "4" (provides TextRerank)
// OR: ort = "2" with corto-ai/bge-reranker-base-onnx model

pub struct CrossEncoderReranker {
    model: fastembed::TextRerank,  // BGE-reranker-base via ONNX
}

impl CrossEncoderReranker {
    /// Re-rank RRF results using cross-encoder scoring.
    /// Input: ~50 candidates from RRF. Output: top-k re-ranked.
    pub fn rerank(
        &self,
        query: &str,
        candidates: Vec<FusedResult>,
        top_k: usize,
    ) -> Vec<FusedResult> {
        // Build (query, document_text) pairs
        // Score with cross-encoder
        // Sort by cross-encoder score
        // Return top_k
    }
}
```

**Pipeline position**: After RRF fusion, before SONA re-ranking.

```text
FTS5 + Vector → RRF fusion (50 candidates) → Cross-encoder re-rank (top 10)
    → SONA preference boost → Thread collapse → Context builder
```

**Model choice**: `BAAI/bge-reranker-base` (278M params, ONNX-exported). Runs in ~20-50ms for 50 candidates on CPU. The `fastembed-rs` crate provides this out of the box via `RerankerModel::BGERerankerBase`.

### 3.5 Improved Embedding Strategy

**Metadata-prefixed embedding text** (supported by ECIR 2026 research showing +7-19% recall):

```rust
// Current (rag.rs line ~1063):
fn prepare_email_text(subject: &str, from_addr: &str, body: &str) -> String {
    format!("{subject}\nFrom: {from_addr}\n{body_truncated_400}")
}

// Enhanced:
fn prepare_email_text(email: &EmailRow) -> String {
    let sender = match &email.from_name {
        Some(name) => format!("{name} <{}>", email.from_addr),
        None => email.from_addr.clone(),
    };
    format!(
        "[Email] Subject: {} | From: {} | Date: {} | Category: {}\nBody: {}",
        email.subject,
        sender,
        email.received_at,
        email.category,
        truncate_body(&email.body_text, 1500)  // increased from 400 to 1500 chars
    )
}
```

Key changes:

- Explicit field labels for embedding model comprehension
- Date and category included for temporal/categorical clustering in vector space
- Body budget increased from 400 to 1500 chars (most embedding models handle 512 tokens ≈ 2000+ chars)
- `from_name` included alongside `from_addr`

### 3.6 Thread-Aware Search

**Add `thread_key` to emails table:**

```sql
-- Migration: add thread awareness
ALTER TABLE emails ADD COLUMN thread_key TEXT;
CREATE INDEX idx_emails_thread_key ON emails(thread_key);
```

Thread key derivation at ingestion from `References`/`In-Reply-To` headers or provider-native thread IDs (Gmail `X-GM-THRID`, Outlook `conversationId`).

**Thread collapsing** in search results — when multiple emails from the same thread match, keep only the highest-scoring one:

```rust
fn collapse_threads(results: Vec<FusedResult>) -> Vec<FusedResult> {
    let mut best_per_thread: HashMap<String, FusedResult> = HashMap::new();
    for result in results {
        let key = result.thread_key.clone().unwrap_or(result.email_id.clone());
        best_per_thread.entry(key)
            .and_modify(|existing| {
                if result.score > existing.score { *existing = result.clone(); }
            })
            .or_insert(result);
    }
    let mut collapsed: Vec<_> = best_per_thread.into_values().collect();
    collapsed.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    collapsed
}
```

### 3.7 Extractive Context Builder

Replace full-body context injection with **sentence-level extraction**:

```rust
/// Extract the most relevant sentences from an email body for the context window.
fn extract_passages(
    body: &str,
    query_embedding: &[f32],
    embedding_pipeline: &EmbeddingPipeline,
    max_tokens: usize,
) -> Vec<String> {
    let sentences = split_sentences(body);
    let scored: Vec<(usize, f32)> = sentences.iter()
        .enumerate()
        .map(|(i, s)| {
            let emb = embedding_pipeline.embed_text(s);
            (i, cosine_similarity(&emb, query_embedding))
        })
        .collect();

    // Select top sentences within token budget, preserve original order
    let mut selected = select_within_budget(scored, &sentences, max_tokens);
    selected.sort_by_key(|(idx, _)| *idx);
    selected.into_iter().map(|(_, s)| s).collect()
}
```

**Context format** for the LLM:

```text
--- Email [ID: abc123] ---
From: Alice <alice@example.com>
Subject: Q2 Budget Review
Date: 2026-03-15
Key passages:
  - "The total Q2 spending was $1.2M, up 15% from Q1."
  - "Marketing requested an additional $200K for the product launch."
```

### 3.8 Aggregation Query Routing

Aggregation queries bypass the RAG pipeline entirely and use SQL:

```rust
match parsed_query.query_type {
    QueryType::Aggregation => {
        // Route to SQL aggregation
        let sql = build_aggregation_sql(&parsed_query);
        let result = db.query(&sql).await?;
        return format_aggregation_result(result);
    }
    _ => {
        // Continue to hybrid search pipeline
    }
}
```

Example: "how many emails from marketing this month?" →

```sql
SELECT COUNT(*) as count, category
FROM emails
WHERE from_addr LIKE '%marketing%'
  AND received_at >= '2026-04-01'
GROUP BY category
```

### 3.9 Hallucination Prevention

**Mandatory citation in the system prompt** (highest-impact, lowest-effort anti-hallucination measure):

```yaml
# config/prompts.yaml — enhanced chat_assistant prompt
chat_assistant: |
  You are an email assistant with full access to the user's inbox.
  The [Email Context] section contains REAL emails from the user's inbox.

  CRITICAL RULES:
  1. Base ALL answers ONLY on the emails provided in [Email Context].
  2. For EVERY factual claim, cite the email by [Subject] or [From].
  3. NEVER fabricate email content, senders, dates, or subjects.
  4. If the context doesn't contain enough information, say:
     "I could not find this in your emails."
  5. If emails are shown, list them with sender, subject, and date.
  6. Do NOT use knowledge outside the provided emails.
```

**Context sufficiency detection** — refuse to answer when context is weak:

```rust
fn is_context_sufficient(rag_context: &RagContext, threshold: f32) -> bool {
    if rag_context.result_count == 0 { return false; }
    // Check that at least one result has a strong relevance score
    rag_context.top_score >= threshold  // e.g., 0.01 for RRF scores
}
```

**Optional NLI verification** (P2 enhancement): Run an NLI cross-encoder post-generation to verify each claim is entailed by the context. Flag contradicted statements. Uses the same ONNX infrastructure as the re-ranker.

### 3.10 HyDE for Complex Semantic Queries (Optional)

For queries where keyword search is insufficient, generate a hypothetical email and use its embedding:

```rust
async fn hyde_expand(query: &str, llm: &dyn GenerativeModel) -> Result<String> {
    let prompt = format!(
        "Write a short email that would be a good response to this search: \"{}\"\n\nEmail:",
        query
    );
    llm.generate(&prompt, 150).await
}
```

Gated behind `QueryType::Semantic` and a complexity threshold. Adds ~300-500ms but significantly improves recall for concept queries like "discussions about restructuring the engineering team."

---

## 4. Externalized Configuration

### Extend `config/tuning.yaml`

```yaml
# ── Query Understanding ──────────────────────────────────────────────────────
query_understanding:
  rule_confidence_threshold: 0.6 # Below this, fall through to LLM parser
  llm_parser_model: null # null = use active generative model
  llm_parser_max_tokens: 150
  enable_hyde: false # HyDE for complex semantic queries
  hyde_max_tokens: 150

# ── FTS5 Scoring ─────────────────────────────────────────────────────────────
fts5:
  column_weights: # Positional: id, subject, from_name, from_addr, body_text, labels
    - 0.0 # id (never match)
    - 10.0 # subject (highest signal)
    - 5.0 # from_name
    - 3.0 # from_addr
    - 1.0 # body_text (baseline)
    - 2.0 # labels

# ── RRF Fusion ───────────────────────────────────────────────────────────────
rrf:
  default_k: 40 # Reduced from 60 for email's smaller result sets
  adaptive_weights:
    factual: { fts: 1.0, vector: 1.0 }
    needle: { fts: 1.5, vector: 0.5 }
    temporal: { fts: 0.8, vector: 1.2 }
    semantic: { fts: 0.7, vector: 1.3 }
    boolean: { fts: 1.3, vector: 0.7 }
    aggregation: { fts: 1.0, vector: 0.0 } # SQL-only, no vector

# ── Re-ranking ───────────────────────────────────────────────────────────────
reranking:
  enabled: true
  model: 'BAAI/bge-reranker-base' # ONNX cross-encoder
  candidates: 50 # Retrieve this many from RRF before re-ranking
  top_k: 10 # Return this many after re-ranking
  timeout_ms: 100 # Skip re-ranking if it takes too long

# ── Thread Awareness ─────────────────────────────────────────────────────────
threads:
  collapse_enabled: true # Deduplicate by thread in search results
  expand_on_select: true # Fetch full thread when user selects a result

# ── Context Building ─────────────────────────────────────────────────────────
context:
  extractive_passages: true # Use sentence extraction instead of full body
  max_passages_per_email: 3 # Top sentences per email
  embedding_body_budget: 1500 # Chars of body in embedding text (was 400)
  context_sufficiency_threshold: 0.01 # Min RRF score to attempt answering
```

---

## 5. Implementation Phases

### Phase A: Quick Wins (Sprint 1) — Low Effort, High Impact

- [ ] **Weighted BM25**: Set persistent FTS5 rank config with column weights (1 SQL statement)
- [ ] **Enhanced prompt**: Add mandatory citation rules to `prompts.yaml`
- [ ] **Context sufficiency**: Refuse to answer when top RRF score < threshold
- [ ] **Embedding text improvement**: Add field labels, date, category; increase body budget to 1500 chars
- [ ] **Externalize FTS5 weights**: Add `fts5.column_weights` to `tuning.yaml`

### Phase B: Query Understanding (Sprint 2) — Medium Effort

- [ ] Create `backend/src/vectors/query_parser.rs` with rule-based parser
- [ ] Add `chrono-english` dependency for temporal resolution
- [ ] Implement `ParsedQuery` struct and `QueryType` enum
- [ ] Wire query parser into `RagPipeline::retrieve_context()`
- [ ] Route aggregation queries to SQL path
- [ ] Add GBNF grammar and LLM fallback for ambiguous queries
- [ ] Add `query_understanding` section to `tuning.yaml`
- [ ] Unit tests for each query type pattern

### Phase C: Advanced Retrieval (Sprint 3) — Medium-High Effort

- [ ] **Weighted RRF**: Replace `reciprocal_rank_fusion()` with adaptive-weight variant
- [ ] **Cross-encoder re-ranking**: Add `fastembed` or ONNX BGE-reranker-base
- [ ] **Pre-filtered FTS5**: CTE pattern for date/sender-scoped queries
- [ ] Pipeline integration: RRF → re-rank → SONA → results
- [ ] Add `rrf` and `reranking` sections to `tuning.yaml`
- [ ] Performance benchmarks: latency impact of re-ranking

### Phase D: Thread Awareness (Sprint 3-4)

- [ ] Migration: add `thread_key` column and index
- [ ] Derive thread_key at ingestion from email headers
- [ ] Thread collapsing in search results
- [ ] Thread expansion API endpoint
- [ ] Add thread metadata to `FusedResult`

### Phase E: Extractive Context + HyDE (Sprint 4)

- [ ] Sentence-level extraction for context building
- [ ] Replace full-body truncation with passage extraction
- [ ] Optional HyDE expansion for semantic queries
- [ ] NLI-based post-generation verification (optional)
- [ ] RAGAS evaluation dataset and automated metrics

---

## 6. New Dependencies

| Crate            | Purpose                                            | Size Impact      |
| ---------------- | -------------------------------------------------- | ---------------- |
| `chrono-english` | Temporal expression parsing                        | Minimal (~10KB)  |
| `fastembed`      | Cross-encoder re-ranking (BGE-reranker-base ONNX)  | ~50MB model file |
| `gbnf`           | JSON Schema → GBNF grammar (for LLM query parsing) | Minimal (~5KB)   |

Note: `chrono`, `regex`, `serde_json`, and `ort` are already in the project.

---

## 7. Performance Budget

| Path                           | p50 Latency | p95 Latency | When                  |
| ------------------------------ | ----------- | ----------- | --------------------- |
| **Fast (rule-based + search)** | ~20ms       | ~70ms       | 70-80% of queries     |
| **LLM parse + search**         | ~350ms      | ~600ms      | 20-30% of queries     |
| **With re-ranking**            | +30ms       | +80ms       | Always (if enabled)   |
| **With HyDE**                  | +400ms      | +800ms      | Complex semantic only |
| **Aggregation (SQL)**          | ~5ms        | ~20ms       | "how many" queries    |

---

## 8. Expected Accuracy Improvements

| Enhancement                      | Expected Impact             | Evidence                                            |
| -------------------------------- | --------------------------- | --------------------------------------------------- |
| Weighted BM25 column scoring     | +10-20% Precision@5         | Subject matches 10x more relevant than body matches |
| Cross-encoder re-ranking         | +40-60% accuracy            | RankRAG (NeurIPS 2024), Ailog MRR study             |
| Query understanding + filters    | +30-50% on filtered queries | Currently 0% — filters are never extracted          |
| Metadata-prefixed embeddings     | +7-19% recall               | ECIR 2026, arXiv:2601.11863                         |
| Increased body budget (400→1500) | +15-25% semantic recall     | Captures 3-4x more content per email                |
| Thread collapsing                | +quality (less noise)       | Eliminates duplicate-thread domination              |
| Mandatory citation               | -80% hallucination          | Prompt engineering + context sufficiency            |
| Extractive passages              | +token efficiency           | 3x more emails in same context window               |

---

## 9. Risks and Mitigations

| Risk                                            | Mitigation                                                                    |
| ----------------------------------------------- | ----------------------------------------------------------------------------- |
| Rule parser misparses query                     | Confidence scoring + LLM fallback for low-confidence                          |
| LLM parser latency on slow hardware             | GBNF grammar limits output to ~50 tokens; timeout and fall back to raw search |
| Cross-encoder model download size (~50MB)       | Lazy download on first use; disable via config                                |
| Thread key derivation from inconsistent headers | Fallback to subject-based grouping; use provider thread IDs when available    |
| Extractive passages lose context                | Include email metadata header for every email; preserve sentence order        |
| HyDE hallucinates misleading hypothetical       | Only use for vector search leg; real FTS5 results anchor the fusion           |

---

## 10. File Impact Summary

### New Files

| Path                                     | Purpose                                           |
| ---------------------------------------- | ------------------------------------------------- |
| `backend/src/vectors/query_parser.rs`    | Rule-based + LLM query understanding              |
| `backend/src/vectors/reranker.rs`        | Cross-encoder re-ranking (BGE via fastembed/ONNX) |
| `backend/src/vectors/thread.rs`          | Thread key derivation, collapsing, expansion      |
| `backend/src/vectors/extractive.rs`      | Sentence-level passage extraction for context     |
| `backend/migrations/020_thread_key.sql`  | Add thread_key column + index                     |
| `backend/migrations/021_fts5_prefix.sql` | Recreate FTS5 with prefix indexes                 |

### Modified Files

| Path                                 | Changes                                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------------------- |
| `backend/Cargo.toml`                 | Add `chrono-english`, `fastembed` or ONNX reranker model, `gbnf`                            |
| `backend/src/vectors/search.rs`      | Weighted RRF, pre-filtered FTS5, thread collapse, re-ranker integration                     |
| `backend/src/vectors/rag.rs`         | Query understanding integration, extractive context, aggregation routing, sufficiency check |
| `backend/src/vectors/embedding.rs`   | Enhanced `prepare_email_text` with field labels, increased body budget                      |
| `backend/src/vectors/yaml_config.rs` | Load new tuning sections (query_understanding, fts5, rrf, reranking, threads, context)      |
| `backend/src/api/ai.rs`              | Pass ParsedQuery through chat endpoint                                                      |
| `config/tuning.yaml`                 | New sections per Section 4                                                                  |
| `config/prompts.yaml`                | Enhanced citation-mandatory system prompt                                                   |

---

## 11. Relationship to ADR-028

ADR-028 (MCP Tool-Calling Chat) and ADR-029 (Enhanced RAG) are **complementary, not competing**:

- **ADR-029 is the default path**: Works with any model, including Qwen3-1.7B. No tool-calling required. Focuses on making search, retrieval, and context quality as good as possible.
- **ADR-028 activates on top**: When a tool-calling-capable model is available (Qwen3-4B+, cloud providers), the MCP tools provide additional capabilities (send email, create rules, etc.) that RAG cannot.
- **Shared infrastructure**: The query understanding layer from ADR-029 benefits the tool-calling path too — it can pre-filter search results before the LLM decides whether to use tools.

Implementation recommendation: **Build ADR-029 first** (Phases A-C), then layer ADR-028 on top.

---

## 12. References

### Academic Papers

- Cormack, Clarke & Grossman. "Reciprocal Rank Fusion outperforms Condorcet and Individual Rank Learning Methods." SIGIR 2009.
- Gao et al. "Precise Zero-Shot Dense Retrieval without Relevance Labels" (HyDE). ACL 2023. arXiv:2212.10496.
- Yu et al. "RankRAG: Unifying Context Ranking with Retrieval-Augmented Generation in LLMs." NeurIPS 2024.
- Es et al. "RAGAS: Automated Evaluation of Retrieval Augmented Generation." arXiv:2309.15217.
- "Utilizing Metadata for Better Retrieval-Augmented Generation." ECIR 2026. arXiv:2601.11863.
- "Correctness is not Faithfulness in RAG Attributions." ICTIR 2025. arXiv:2412.18004.
- "JSONSchemaBench: A Rigorous Benchmark of Structured Outputs for Language Models." arXiv:2501.10868.
- "From Natural Language to SQL: Review of LLM-based Text-to-SQL Systems." arXiv:2410.01066.

### Tools & Libraries

- [fastembed-rs](https://github.com/Anush008/fastembed-rs) — Rust reranking with BGE models
- [chrono-english](https://github.com/stevedonovan/chrono-english) — Natural language date parsing
- [gbnf crate](https://crates.io/crates/gbnf) — JSON Schema to GBNF grammar
- [SQLite FTS5](https://www.sqlite.org/fts5.html) — Full-text search documentation

### Project ADRs

- ADR-001: Hybrid Search Architecture
- ADR-012: Generative AI Provider Architecture
- ADR-022: RAG Pipeline
- ADR-028: MCP-Powered Tool-Calling Chat
