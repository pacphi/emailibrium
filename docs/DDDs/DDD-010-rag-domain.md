# DDD-010: RAG (Retrieval-Augmented Generation) Domain

- **Status**: Accepted
- **Date**: 2026-03-26
- **Bounded Context**: Email Intelligence / Chat
- **Implements**: ADR-022 (RAG Pipeline)

## Domain Overview

The RAG domain bridges the **Search** bounded context (DDD-002) and the **AI Providers** bounded context (DDD-006) to enable email-aware conversational AI. It is not a standalone service but a **domain service** that orchestrates retrieval and prompt augmentation.

## Ubiquitous Language

| Term                 | Definition                                                                       |
| -------------------- | -------------------------------------------------------------------------------- |
| **RAG Pipeline**     | The end-to-end flow: query → search → fetch → format → inject into prompt        |
| **Retrieval**        | Searching the email corpus for content relevant to a user query                  |
| **Context Budget**   | Maximum tokens allocated for email context within the LLM prompt                 |
| **Email Snippet**    | A formatted summary of one email (sender, subject, date, truncated body)         |
| **RAG Context**      | The aggregate output: formatted snippets + metadata, ready for prompt injection  |
| **Relevance Score**  | A 0.0–1.0 score from HybridSearch indicating how well an email matches the query |
| **Token Estimation** | Approximate conversion: 1 token ≈ 4 characters (used for budget enforcement)     |

## Aggregates

### RagPipeline (Domain Service)

The `RagPipeline` is a stateless domain service. It does not own entities or maintain state between requests — each `retrieve_context()` call is independent.

**Dependencies (injected):**

- `HybridSearch` (from DDD-002) — provides semantic + keyword email search
- `Database` — direct SQL access for fetching email content by ID
- `RagConfig` — controls retrieval behavior (top_k, min_score, budget)

**Invariants:**

1. Total context output must not exceed `max_context_tokens`
2. Only emails scoring above `min_relevance_score` are included
3. Email body text is truncated at word boundaries, never mid-character
4. The output format is deterministic: emails appear in relevance-ranked order

### RagConfig (Value Object)

Immutable configuration loaded from `config.yaml`. Controls:

- `top_k` — maximum candidate emails to retrieve
- `min_relevance_score` — threshold for inclusion
- `max_context_tokens` — budget for the formatted output
- `include_body` — whether to include body text or metadata only
- `max_body_chars` — per-email body truncation limit

### RagContext (Value Object)

Output of a single retrieval operation:

- `formatted_context: String` — ready for prompt injection
- `email_ids: Vec<String>` — IDs of included emails (for future citation UI)
- `result_count: usize` — number of emails in context

## Domain Events

None. RAG is a synchronous query-side operation with no side effects.

## Integration Points

### Upstream (consumed by RAG)

| Context          | Interface                | Purpose                            |
| ---------------- | ------------------------ | ---------------------------------- |
| Search (DDD-002) | `HybridSearch::search()` | Semantic + keyword email retrieval |
| Email Store      | `emails` table (SQLite)  | Fetch email content by ID          |

### Downstream (RAG feeds into)

| Context                | Interface                          | Purpose                                    |
| ---------------------- | ---------------------------------- | ------------------------------------------ |
| Chat (R-07)            | `ChatService::chat(email_context)` | Pre-formatted context injected into prompt |
| AI Providers (DDD-006) | `GenerativeModel::generate()`      | LLM receives augmented prompt              |

## Context Map

```text
┌─────────────┐      ┌────────────────┐      ┌──────────────┐
│   Search    │──────▶│  RAG Pipeline  │──────▶│ ChatService  │
│  (DDD-002)  │      │  (DDD-010)     │      │   (R-07)     │
│             │      │                │      │              │
│ HybridSearch│      │ retrieve_ctx() │      │ build_prompt │
│ FTS5 + Vec  │      │ format + trim  │      │ generate()   │
└─────────────┘      └────────────────┘      └──────┬───────┘
                                                     │
                                              ┌──────▼───────┐
                                              │  AI Providers │
                                              │  (DDD-006)    │
                                              │               │
                                              │ BuiltIn/Ollama│
                                              │ /Cloud        │
                                              └───────────────┘
```

## Anti-Corruption Layer

RAG translates between the Search domain's `FusedResult` (score-based ranking with metadata maps) and the Chat domain's `email_context: String` (formatted natural language). This prevents the Chat domain from depending on search-specific types.

## File Structure

```text
backend/src/vectors/
  rag.rs          — RagPipeline, RagConfig, RagContext, EmailSnippet
  chat.rs         — ChatService (consumes RagContext via email_context param)
  search.rs       — HybridSearch (upstream provider)
  config.rs       — RagConfig integrated into VectorConfig
```

## Future Extensions

1. **Citation UI** — Return `email_ids` to the frontend so chat messages can link to source emails
2. **Re-ranking** — Apply SONA preference vectors (DDD-004) to re-rank RAG results by user behavior
3. **Multi-account RAG** — Filter retrieval by account_id when multiple email accounts are onboarded
4. **Streaming RAG** — Retrieve context in parallel with prompt tokenization for lower latency
5. **Context caching** — Cache RAG results for follow-up questions in the same session
