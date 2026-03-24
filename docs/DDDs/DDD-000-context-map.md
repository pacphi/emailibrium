# DDD-000: Emailibrium Context Map

| Field | Value |
|-------|-------|
| Status | Accepted |
| Date | 2026-03-23 |
| Scope | System-wide |

## Overview

This document defines the bounded context map for Emailibrium, a vector-native email intelligence platform. It establishes the relationships, integration patterns, and event flows between all bounded contexts.

## Bounded Contexts

| Context | Type | Document | Responsibility |
|---------|------|----------|----------------|
| **Email Intelligence** | Core | DDD-001 | Embedding, classification, clustering, hybrid search internals |
| **Search** | Core | DDD-002 | Query execution, result fusion, SONA re-ranking |
| **Ingestion** | Supporting | DDD-003 | Email sync, multi-asset extraction, pipeline orchestration |
| **Learning** | Supporting | DDD-004 | SONA adaptive learning, centroid updates, feedback processing |
| **Account Management** | Supporting | DDD-005 | Provider connections, OAuth, sync state, archive strategy |

## Context Map Diagram

```
                  ┌─────────────────────────┐
                  │   Account Management    │
                  │      (Supporting)       │
                  │                         │
                  │  EmailAccount, SyncState│
                  └────────────┬────────────┘
                               │
                     Published Language
                    (AccountConnected,
                     SyncCompleted)
                               │
                               ▼
                  ┌─────────────────────────┐
                  │       Ingestion         │
                  │      (Supporting)       │
                  │                         │
                  │  IngestionJob,          │
                  │  ContentExtraction      │
                  └────────────┬────────────┘
                               │
                     Customer / Supplier
                    (ContentExtracted,
                     IngestionCompleted)
                               │
                               ▼
                  ┌─────────────────────────┐
                  │   Email Intelligence    │◄──────────────────┐
                  │        (Core)           │                   │
                  │                         │        Published Language
                  │  EmailEmbedding,        │       (CentroidUpdated)
                  │  EmailClassification,   │                   │
                  │  TopicCluster           │                   │
                  └──────┬──────────┬───────┘                   │
                         │          │                           │
              Published  │          │  Published                │
              Language   │          │  Language                 │
           (EmailEmbedded│          │(EmailClassified,          │
            EmailClassified)        │ ClassificationCorrected)  │
                         │          │                           │
                         ▼          ▼                           │
          ┌──────────────────┐  ┌──────────────────┐           │
          │      Search      │  │     Learning     │           │
          │      (Core)      │  │   (Supporting)   │───────────┘
          │                  │  │                   │
          │  SearchQuery,    │  │  LearningModel,   │
          │  SearchInteraction│ │  UserFeedback     │
          └────────┬─────────┘  └──────────▲───────┘
                   │                       │
                   │    Published Language  │
                   │   (SearchResultClicked,│
                   │    SearchFeedbackProvided)
                   └───────────────────────┘
```

## Integration Patterns

### 1. Account Management --> Ingestion: Published Language

- **Pattern**: Published Language
- **Direction**: Account Management publishes; Ingestion consumes
- **Events**: `AccountConnected`, `SyncCompleted`, `TokenRefreshed`, `TokenExpired`
- **Rationale**: Ingestion needs to know when new accounts are ready and when sync completes to begin processing. The event schema is the published contract.

### 2. Ingestion --> Email Intelligence: Customer / Supplier

- **Pattern**: Customer / Supplier
- **Direction**: Ingestion (supplier) provides extracted content; Email Intelligence (customer) defines what it needs
- **Events**: `ContentExtracted`, `IngestionCompleted`
- **Rationale**: Email Intelligence is the core domain and dictates the contract. Ingestion conforms to the extraction format that Intelligence requires for embedding and classification.

### 3. Email Intelligence --> Search: Published Language

- **Pattern**: Published Language
- **Direction**: Email Intelligence publishes embedding and classification events; Search consumes them
- **Events**: `EmailEmbedded`, `EmailClassified`, `ClusterDiscovered`
- **Rationale**: Search depends on the vector store and classification data produced by Intelligence. The event schema acts as the integration contract.

### 4. Email Intelligence --> Learning: Published Language

- **Pattern**: Published Language
- **Direction**: Email Intelligence publishes classification events; Learning consumes them
- **Events**: `EmailClassified`, `ClassificationCorrected`
- **Rationale**: Learning needs to observe classification outcomes and corrections to update its models.

### 5. Search --> Learning: Published Language

- **Pattern**: Published Language
- **Direction**: Search publishes interaction events; Learning consumes them
- **Events**: `SearchResultClicked`, `SearchFeedbackProvided`
- **Rationale**: Learning uses search interaction signals as implicit feedback for SONA re-ranking weight updates.

### 6. Learning --> Email Intelligence: Published Language

- **Pattern**: Published Language
- **Direction**: Learning publishes centroid updates; Email Intelligence consumes them
- **Events**: `CentroidUpdated`, `LongTermConsolidated`
- **Rationale**: Updated centroids from SONA learning flow back into the classification engine, creating a closed feedback loop. This is the key adaptive behavior of the system.

## Anti-Corruption Layers

| ACL | Location | Purpose |
|-----|----------|---------|
| **VectorStore facade** | Email Intelligence | Wraps RuVector SDK, isolating the core domain from vector DB implementation details (ADR-003) |
| **EmbeddingModel trait** | Email Intelligence | Abstracts over RuvLLM, Ollama, and cloud embedding providers (ADR-002) |
| **EmailProvider trait** | Account Management | Wraps Gmail API, Microsoft Graph API, and IMAP/POP3 behind a unified interface |

## Shared Kernel

The **Email** entity is referenced across all bounded contexts. However, each context maintains its own projection of it rather than sharing a single model.

| Context | Email Projection |
|---------|-----------------|
| Account Management | `SyncedEmail { provider_id, account_id, raw_headers, sync_timestamp }` |
| Ingestion | `RawEmail { email_id, subject, from, to, date, html_body, attachments, headers }` |
| Email Intelligence | `EmbeddedEmail { email_id, embedding_ids, category, cluster_id }` |
| Search | `SearchableEmail { email_id, subject, from, to, date, snippet, score }` |
| Learning | `FeedbackEmail { email_id, category, user_actions, interaction_history }` |

The `email_id` is the correlation identifier across all contexts. It is generated during ingestion and propagated via domain events.

## Event Bus Architecture

All cross-context communication uses asynchronous domain events via an in-process event bus (for single-node deployment) with the option to externalize to a message broker for distributed deployment.

```
┌─────────────────────────────────────────────────────┐
│                    Event Bus                        │
│                                                     │
│  Publishers:                                        │
│    Account Management, Ingestion,                   │
│    Email Intelligence, Search, Learning             │
│                                                     │
│  Delivery: At-least-once with idempotent consumers  │
│  Ordering: Per-aggregate ordering guaranteed         │
│  Persistence: Events stored for replay/audit        │
└─────────────────────────────────────────────────────┘
```

## Dependency Rules

1. **No circular runtime dependencies** between contexts. The Learning --> Email Intelligence feedback loop is event-driven and asynchronous.
2. **Core domains never depend on supporting/generic domains** at the code level. All integration is via events or trait abstractions.
3. **Each context owns its data store**. No shared databases. Cross-context queries go through published APIs or read models built from events.
4. **The Email Intelligence context is the strategic core**. It receives the most investment and has the most rigorous modeling.
