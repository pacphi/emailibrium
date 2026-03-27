# ADR-022: Retrieval-Augmented Generation (RAG) Pipeline for Email-Aware Chat

- **Status**: Accepted
- **Date**: 2026-03-26
- **Extends**: ADR-012 (Tiered AI Provider Architecture), ADR-021 (Built-in Local LLM), DDD-010 (RAG Domain)

## Context

The chat assistant (R-07) generates responses using the configured generative model (built-in llama.cpp, Ollama, or cloud LLM). However, prior to this decision, the LLM had **no access to the user's email data**. When a user asked "Did I receive an email from Josh Bob?", the model could only respond with generic advice because:

1. The `ChatService.build_prompt()` method injected email IDs as plain strings ("email-001, email-002"), not actual content
2. No automatic retrieval occurred — the system waited for the frontend to manually select emails
3. The `HybridSearch` engine (semantic + keyword, ADR-001) existed but was not connected to the chat flow

The infrastructure was 90% complete:

| Component               | Status Before | Purpose                                         |
| ----------------------- | ------------- | ----------------------------------------------- |
| `HybridSearch`          | Ready         | Semantic + keyword email search with RRF fusion |
| `EmbeddingPipeline`     | Ready         | Embed user queries for similarity search        |
| `emails` table (FTS5)   | Ready         | Full-text search on subject, body, sender       |
| `ChatService`           | Missing RAG   | Session management + prompt building            |
| `GenerativeModel` trait | Ready         | Provider-agnostic generation                    |

## Decision

### 1. Add a `RagPipeline` Struct

A new `RagPipeline` in `src/vectors/rag.rs` bridges search and chat. It is:

- **Provider-agnostic** — the same retrieval feeds all LLM providers
- **Composable** — sits between the API handler and ChatService, not embedded inside either
- **Token-budget-aware** — truncates email content to fit the active model's context window

### 2. Automatic Retrieval on Every Chat Turn

Every chat message triggers a hybrid search. Results below `min_relevance_score` (default 0.25) are discarded. If no emails match, the prompt proceeds without email context. This avoids fragile intent-classification heuristics while keeping search cheap.

### 3. Context Budget by Provider

| Provider        | Context Window | Email Budget  | Approx. Emails |
| --------------- | -------------- | ------------- | -------------- |
| Built-in (0.5B) | 2,048 tokens   | ~1,024 tokens | 2-3 snippets   |
| Ollama (3B)     | 8,192 tokens   | ~4,096 tokens | 8-10 snippets  |
| Cloud (GPT-4o)  | 128,000 tokens | ~8,192 tokens | 15-20 snippets |

The caller controls the budget via `max_context_tokens`. The pipeline greedily fills it with ranked results.

### 4. Pre-formatted Context Injection

`ChatService.chat()` accepts `email_context: Option<String>` — a pre-formatted text block ready for prompt injection. The `RagPipeline` formats each email as:

```text
--- Email ---
From: Jane Doe <jane@example.com>
Subject: Q1 Budget Review
Date: 2026-03-15
Category: Finance
Body: Please review the attached Q1 budget spreadsheet...
```

This is injected into the `[Email Context]` block of the prompt, which the `to_chatml()` converter (ADR-021 addendum) wraps into a `<|im_start|>system` block for Qwen-compatible models.

### 5. Architecture

```text
POST /api/v1/ai/chat/stream
  │
  ├─ RagPipeline.retrieve_context(user_message)
  │    ├─ HybridSearch (semantic + FTS5 keyword)
  │    ├─ Filter by min_relevance_score
  │    ├─ Fetch email content from SQLite
  │    └─ Format + truncate to token budget
  │
  └─ ChatService.chat(session_id, message, email_context)
       ├─ build_prompt() with [System] + [Email Context] + [History]
       └─ GenerativeRouter.generate() → any provider
```

## Configuration

```yaml
rag:
  top_k: 5 # Max emails to retrieve per query
  min_relevance_score: 0.005 # RRF scores are ~0.016 for rank-1; this filters noise
  max_context_tokens: 1024 # Token budget for email context
  include_body: true # Include body text (vs. metadata only)
  max_body_chars: 500 # Max body characters per email
```

## Consequences

### Positive

- **Email-aware chat** — The LLM can now answer "Did I get email from Josh Bob?" with real data
- **Zero frontend changes** — RAG runs server-side; the frontend remains a REST/SSE client
- **Provider-agnostic** — Same retrieval pipeline for all generative providers
- **Graceful degradation** — If no emails match, the LLM still responds (without context)
- **Cheap** — HybridSearch is fast (~50ms); no extra API calls needed

### Negative

- **Token pressure on small models** — The 0.5B model has only 2,048 tokens; email context competes with conversation history. Mitigated by configurable budget.
- **No citation linking** — The frontend doesn't yet show which emails were used. The `RagContext` returns `email_ids` for future UI integration.
- **Always-on search** — Every chat turn searches emails, even for greetings. Mitigated by the relevance threshold; non-email queries return zero results cheaply.

## References

- ADR-001: Hybrid Search with Reciprocal Rank Fusion
- ADR-012: Tiered AI Provider Architecture
- ADR-021: Built-in Local LLM / Addendum (Rust backend)
- DDD-002: Search Bounded Context
- DDD-010: RAG Domain
