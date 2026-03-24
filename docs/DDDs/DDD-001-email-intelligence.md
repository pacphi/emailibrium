# DDD-001: Email Intelligence Domain (Core)

| Field   | Value              |
| ------- | ------------------ |
| Status  | Accepted           |
| Date    | 2026-03-23         |
| Type    | Core Domain        |
| Context | Email Intelligence |

## Overview

The Email Intelligence bounded context is the strategic core of Emailibrium. It encompasses the vector embedding, classification, clustering, and hybrid search capabilities that differentiate the platform. All intelligence derived from email content originates here.

## Aggregates

### 1. EmbeddingAggregate

Manages the lifecycle of email embeddings: generation, storage, update, and deletion.

**Root Entity: EmailEmbedding**

| Field       | Type            | Description                                                               |
| ----------- | --------------- | ------------------------------------------------------------------------- |
| email_id    | EmailId         | Correlation ID from ingestion                                             |
| vector_id   | VectorId        | ID within the vector store                                                |
| collection  | CollectionName  | Target collection (email_text, image_text, image_visual, attachment_text) |
| dimensions  | u32             | Embedding dimensionality (e.g., 384, 768)                                 |
| model       | ModelIdentifier | Embedding model used                                                      |
| status      | EmbeddingStatus | Pending, Embedded, Failed, Stale                                          |
| embedded_at | DateTime        | Timestamp of embedding creation                                           |

**Invariants:**

- An email may have multiple embeddings (one per collection/asset), but only one active embedding per (email_id, collection) pair.
- Embedding status must transition in order: Pending --> Embedded or Pending --> Failed.
- Re-embedding (model upgrade) marks the old embedding as Stale before creating a new one.

**Commands:**

- `EmbedEmail { email_id, content, collection }` -- triggers embedding generation
- `ReembedEmail { email_id, new_model }` -- triggers re-embedding with a new model
- `DeleteEmbedding { email_id, collection }` -- removes an embedding

### 2. ClassificationAggregate

Manages email categorization via vector centroid similarity and LLM fallback.

**Root Entity: EmailClassification**

| Field         | Type                 | Description               |
| ------------- | -------------------- | ------------------------- |
| email_id      | EmailId              | Correlation ID            |
| category      | Category             | Assigned category         |
| confidence    | SimilarityScore      | Classification confidence |
| method        | ClassificationMethod | Centroid or LLM           |
| classified_at | DateTime             | Timestamp                 |

**Invariants:**

- Every classified email must have an associated embedding (embedding must exist before classification).
- If centroid confidence < ConfidenceThreshold, LLM fallback is triggered.
- Reclassification preserves history (old classification is not deleted, new one is appended with a correction event).

**Commands:**

- `ClassifyEmail { email_id }` -- runs centroid-based classification
- `ReclassifyEmail { email_id, new_category, source }` -- manual or feedback-driven correction

### 3. ClusterAggregate

Manages topic cluster lifecycle via GraphSAGE and HDBSCAN.

**Root Entity: TopicCluster**

| Field              | Type      | Description                               |
| ------------------ | --------- | ----------------------------------------- |
| id                 | ClusterId | Unique cluster identifier                 |
| name               | String    | Human-readable cluster name               |
| description        | String    | Auto-generated or user-edited description |
| centroid_vector_id | VectorId  | Centroid vector in the vector store       |
| email_count        | u32       | Number of emails in this cluster          |
| stability_score    | f32       | Cluster stability metric [0.0, 1.0]       |
| created_at         | DateTime  | When the cluster was first discovered     |

**Invariants:**

- Clusters with stability_score < 0.3 are candidates for merging or dissolution.
- Cluster merges must produce a new centroid (weighted average of source centroids).
- Minimum cluster size is configurable (default: 5 emails).

**Commands:**

- `DiscoverClusters { collection }` -- runs clustering algorithm
- `MergeClusters { source_ids, target_name }` -- merges multiple clusters
- `DissolveClusters { cluster_ids }` -- removes unstable clusters, reassigning emails

## Domain Events

| Event                   | Fields                                                | Published When                           |
| ----------------------- | ----------------------------------------------------- | ---------------------------------------- |
| EmailEmbedded           | email_id, vector_id, collection, dimensions           | Embedding successfully stored            |
| EmailClassified         | email_id, category, confidence, method                | Classification assigned                  |
| ClusterDiscovered       | cluster_id, name, email_count                         | New cluster identified by clustering run |
| ClusterMerged           | source_ids, target_id                                 | Two or more clusters consolidated        |
| ClassificationCorrected | email_id, old_category, new_category, feedback_source | User or system corrects a classification |

### Event Consumers

| Event                   | Consumed By      | Purpose                                      |
| ----------------------- | ---------------- | -------------------------------------------- |
| EmailEmbedded           | Search           | Makes email searchable via vector similarity |
| EmailClassified         | Search, Learning | Updates search index; feeds SONA learning    |
| ClassificationCorrected | Learning         | Triggers centroid adjustment                 |
| ClusterDiscovered       | Search           | Updates cluster-based search facets          |

## Value Objects

### EmbeddingVector

```
EmbeddingVector(Vec<f32>)
```

An immutable, dense floating-point vector representing the semantic content of an email or asset. Dimensionality is determined by the embedding model.

### CategoryCentroid

| Field        | Type            | Description                                    |
| ------------ | --------------- | ---------------------------------------------- |
| category     | Category        | The category this centroid represents          |
| vector       | EmbeddingVector | Average vector of all emails in this category  |
| email_count  | u32             | Number of emails contributing to this centroid |
| last_updated | DateTime        | Last time the centroid was recalculated        |

### SimilarityScore

```
SimilarityScore(f32) -- bounded [0.0, 1.0]
```

Represents the cosine similarity between two vectors. Used for classification confidence and search relevance.

### ConfidenceThreshold

```
ConfidenceThreshold(f32) -- configurable, default 0.7
```

The minimum SimilarityScore required for centroid-based classification to be accepted without LLM fallback.

## Domain Services

### EmbeddingPipeline

Converts text content into vector embeddings using a tiered fallback chain.

**Fallback Order:**

1. RuvLLM (local, fastest, no network)
2. Ollama (local, supports more models)
3. Cloud provider (OpenAI, Cohere -- last resort)

**Responsibilities:**

- Text preprocessing (truncation, chunking for long emails)
- Model selection based on content type and collection
- Batch embedding for ingestion workloads
- Embedding quality validation (zero-vector detection, dimensionality check)

### VectorCategorizer

Centroid-based classification with LLM fallback.

**Algorithm:**

1. Compute cosine similarity between email embedding and all category centroids.
2. If max similarity >= ConfidenceThreshold, assign that category.
3. If max similarity < ConfidenceThreshold, invoke LLM classification.
4. Emit EmailClassified event with method = Centroid or LLM.

### ClusterEngine

Topic discovery via GraphSAGE neighborhood aggregation and HDBSCAN density clustering.

**Responsibilities:**

- Periodic cluster discovery (configurable schedule)
- Incremental cluster updates as new emails arrive
- Stability scoring based on inter-cluster distance and intra-cluster cohesion
- Cluster naming via LLM summarization of representative emails

### HybridSearch

Combined FTS5 full-text search and HNSW vector search with Reciprocal Rank Fusion.

**Algorithm:**

1. Execute FTS5 keyword search against SQLite.
2. Execute HNSW vector search against RuVector.
3. Fuse results using RRF: `score = sum(1 / (k + rank))` where k=60.
4. Apply SONA re-ranking weights (from Learning context).
5. Return scored, deduplicated result set.

## Anti-Corruption Layers

### VectorStore Facade (ADR-003)

The RuVector SDK is wrapped in a `VectorStore` facade that exposes domain-oriented operations:

| Facade Method                                     | RuVector Operation                     |
| ------------------------------------------------- | -------------------------------------- |
| `store_embedding(email_id, vector, collection)`   | `collection.add(vector, metadata)`     |
| `search_similar(query_vector, collection, limit)` | `collection.search(query, k)`          |
| `get_centroid(category)`                          | `collection.get_by_metadata(category)` |
| `delete_embedding(vector_id)`                     | `collection.delete(id)`                |

This layer ensures that if the vector database implementation changes, only the facade needs modification.

### EmbeddingModel Trait (ADR-002)

All embedding providers implement a unified trait:

```
trait EmbeddingModel {
    fn embed_text(text: &str) -> Result<EmbeddingVector>;
    fn embed_batch(texts: &[&str]) -> Result<Vec<EmbeddingVector>>;
    fn dimensions() -> u32;
    fn model_id() -> ModelIdentifier;
}
```

Implementations: `RuvLlmEmbedder`, `OllamaEmbedder`, `CloudEmbedder`.

## Ubiquitous Language

| Term                | Definition                                                                             |
| ------------------- | -------------------------------------------------------------------------------------- |
| **Embed**           | Generate a vector representation of email content using a language model               |
| **Classify**        | Assign a category to an email via centroid similarity or LLM fallback                  |
| **Cluster**         | Group related emails by topic using graph neural network techniques                    |
| **Centroid**        | The average vector representing all emails in a given category                         |
| **Confidence**      | The cosine similarity score indicating how certain a classification is                 |
| **Hybrid search**   | Combined FTS5 full-text and HNSW vector search with RRF fusion                         |
| **Collection**      | A named vector store partition (email_text, image_text, image_visual, attachment_text) |
| **Stale embedding** | An embedding generated by an older model version, pending re-embedding                 |
| **Fallback chain**  | The ordered sequence of embedding providers tried when generating vectors              |

## Boundaries

- This context does NOT handle email ingestion or provider sync (that is Ingestion / Account Management).
- This context does NOT manage user search queries or result presentation (that is Search).
- This context does NOT own the SONA learning loop (that is Learning), but it consumes updated centroids from Learning.
- This context DOES own the vector store, embedding pipeline, classification engine, and clustering engine.
