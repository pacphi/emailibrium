# DDD-006: AI Providers Domain (Supporting)

| Field   | Value             |
| ------- | ----------------- |
| Status  | Proposed          |
| Date    | 2026-03-23        |
| Type    | Supporting Domain |
| Context | AI Providers      |

## Overview

The AI Providers bounded context manages the lifecycle of AI models, provider configuration, inference sessions, and privacy consent. It sits between the Email Intelligence context (which consumes embeddings and classifications) and external systems (ONNX model files, Ollama, cloud APIs). This context enforces the tiered provider architecture (Tier 0: ONNX, Tier 1: Ollama, Tier 2: Cloud) and ensures that no data leaves the user's machine without explicit consent.

This is a **supporting domain**: it does not contain the core intelligence logic (that belongs to Email Intelligence), but the core domain cannot function without it. The AI Providers context owns model acquisition, session management, provider routing, consent enforcement, and audit logging.

## Strategic Classification

| Aspect              | Value                                                   |
| ------------------- | ------------------------------------------------------- |
| Domain type         | Supporting                                              |
| Investment priority | High (enables core domain)                              |
| Complexity driver   | External integration diversity, privacy invariants      |
| Change frequency    | Medium (new providers, new models)                      |
| Risk                | Model integrity, consent violations, cloud data leakage |

---

## Aggregates

### 1. ProviderConfigAggregate

Manages the tiered provider configuration for embedding and generative inference. This is the central configuration aggregate that determines which provider handles each type of inference request.

**Root Entity: ProviderConfig**

| Field                    | Type             | Description                                      |
| ------------------------ | ---------------- | ------------------------------------------------ |
| id                       | ProviderConfigId | Singleton per installation (one active config)   |
| embedding_provider       | ProviderType     | Active embedding provider (Onnx, Ollama, Cloud)  |
| generative_provider      | ProviderType     | Active generative provider (None, Ollama, Cloud) |
| image_embedding_enabled  | bool             | Whether image embedding is active                |
| image_embedding_provider | ProviderType     | Active image embedding provider                  |
| consent_status           | ConsentStatus    | Cloud data-sharing consent state                 |
| updated_at               | DateTime         | Last configuration change timestamp              |

**Invariants:**

- Cloud provider (embedding or generative) requires `consent_status == Granted`. Any attempt to set a cloud provider without consent must be rejected.
- Cloud provider requires the corresponding API key environment variable to be set. The aggregate validates key presence at configuration time, not at inference time.
- Changing the embedding provider may trigger a reindex if the new model produces different-dimensioned vectors. The aggregate emits a `ProviderChanged` event with `requires_reindex: bool`.
- The generative provider can be `None` (Tier 0 default). This is the only provider type that accepts `None`.
- Revoking cloud consent must immediately downgrade any active cloud providers to their fallback (Onnx for embedding, None for generative).

**Commands:**

- `SetEmbeddingProvider { provider: ProviderType, model_id: ModelId }` -- switches the active embedding provider
- `SetGenerativeProvider { provider: ProviderType, model_id: Option<ModelId> }` -- switches the active generative provider
- `GrantCloudConsent { user_id: UserId, provider: CloudProvider, acknowledged_at: DateTime }` -- records explicit consent
- `RevokeCloudConsent { user_id: UserId, provider: CloudProvider }` -- revokes consent and forces provider downgrade
- `EnableImageEmbedding { provider: ProviderType }` -- enables image embedding with specified provider
- `DisableImageEmbedding` -- disables image embedding

### 2. ModelRegistryAggregate

Manages the inventory of downloaded, verified, and available AI models. Each model entry tracks its download status, integrity verification, and usage history.

**Root Entity: ModelEntry**

| Field           | Type             | Description                                              |
| --------------- | ---------------- | -------------------------------------------------------- |
| model_id        | ModelId          | Unique identifier (e.g., "all-MiniLM-L6-v2")             |
| provider        | ProviderType     | Which provider type this model belongs to (Onnx, Ollama) |
| manifest        | ModelManifest    | Model metadata (name, dimensions, SHA-256, URL, size)    |
| path            | Option<FilePath> | Local filesystem path to model files                     |
| download_status | DownloadStatus   | NotStarted, Downloading, Ready, Failed, Corrupted        |
| verified        | bool             | Whether SHA-256 integrity check has passed               |
| size_bytes      | u64              | Actual size on disk                                      |
| downloaded_at   | Option<DateTime> | When the model was downloaded                            |
| last_used       | Option<DateTime> | Last time the model served an inference request          |

**Invariants:**

- A model cannot be set as the active model in ProviderConfig unless `download_status == Ready` and `verified == true`.
- SHA-256 checksum must match the value in the model manifest. If verification fails, status transitions to `Corrupted` and the model files are quarantined (not deleted, to allow manual inspection).
- Only one download operation per model may be in progress at a time. Concurrent download requests for the same model are deduplicated.
- Deleting a model that is currently active in ProviderConfig is forbidden. The provider must be switched first.
- Models are downloaded exclusively from pinned sources (Hugging Face Hub repositories listed in the manifest). Downloads from unknown sources are rejected.

**Commands:**

- `RegisterModel { manifest: ModelManifest }` -- adds a model to the registry without downloading
- `DownloadModel { model_id: ModelId }` -- initiates download from the manifest URL
- `VerifyIntegrity { model_id: ModelId }` -- runs SHA-256 verification against manifest
- `DeleteModel { model_id: ModelId }` -- removes model files from disk (only if not active)
- `SetActiveModel { model_id: ModelId }` -- marks this model as the active model for its provider type
- `UpdateLastUsed { model_id: ModelId, timestamp: DateTime }` -- updates usage tracking

### 3. InferenceSessionAggregate

Manages active inference sessions -- loaded models that are ready to serve requests. A session represents a model that has been loaded into memory (ONNX Runtime session, Ollama connection, or cloud API client) and is actively serving inference.

**Root Entity: InferenceSession**

| Field           | Type             | Description                           |
| --------------- | ---------------- | ------------------------------------- |
| session_id      | SessionId        | Unique session identifier             |
| provider_type   | ProviderType     | Onnx, Ollama, or Cloud                |
| model_id        | ModelId          | The model backing this session        |
| status          | SessionStatus    | Initializing, Ready, Degraded, Closed |
| created_at      | DateTime         | When the session was created          |
| request_count   | u64              | Total inference requests served       |
| error_count     | u64              | Total errors encountered              |
| last_request_at | Option<DateTime> | Timestamp of most recent inference    |
| last_error      | Option<String>   | Most recent error message             |

**Invariants:**

- Inference can only be executed on sessions with `status == Ready`. Requests against Initializing, Degraded, or Closed sessions are rejected.
- Cloud sessions require `consent_status == Granted` in ProviderConfig at the time of each inference call (not just at session creation). If consent is revoked mid-session, the session transitions to `Closed`.
- A session that accumulates errors beyond a configurable threshold (default: 5 consecutive errors) transitions to `Degraded`. Degraded sessions must be explicitly closed and recreated.
- Only one active session per (provider_type, model_id) pair. Creating a new session for the same pair closes the existing one.
- ONNX sessions are heavyweight (model loaded in memory). Session creation is an expensive operation and should be infrequent.

**Commands:**

- `CreateSession { provider_type: ProviderType, model_id: ModelId }` -- loads model and initializes session
- `ExecuteInference { session_id: SessionId, input: InferenceInput }` -- runs inference and returns result
- `ExecuteBatchInference { session_id: SessionId, inputs: Vec<InferenceInput> }` -- runs batch inference
- `CloseSession { session_id: SessionId }` -- releases resources and marks session as closed
- `RecordError { session_id: SessionId, error: String }` -- increments error count, may trigger degradation

---

## Domain Events

| Event                  | Fields                                                                   | Published When                                         |
| ---------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------ |
| ModelRegistered        | model_id, provider, manifest                                             | New model added to registry                            |
| ModelDownloadStarted   | model_id, url, expected_sha256, expected_size_bytes                      | Download begins                                        |
| ModelDownloadProgress  | model_id, bytes_downloaded, total_bytes, percent                         | Periodic progress update                               |
| ModelDownloadCompleted | model_id, path, actual_sha256, verified                                  | Download finishes and verification runs                |
| ModelDownloadFailed    | model_id, error, retry_count                                             | Download fails                                         |
| ModelIntegrityFailed   | model_id, expected_sha256, actual_sha256                                 | SHA-256 mismatch detected                              |
| ModelDeleted           | model_id, path, freed_bytes                                              | Model files removed from disk                          |
| ProviderChanged        | old_provider, new_provider, old_model_id, new_model_id, requires_reindex | Embedding or generative provider switched              |
| CloudConsentGranted    | user_id, provider, timestamp                                             | User explicitly consents to cloud data sharing         |
| CloudConsentRevoked    | user_id, provider, timestamp, downgraded_providers                       | User revokes consent; affected providers listed        |
| SessionCreated         | session_id, provider_type, model_id                                      | Inference session initialized and ready                |
| SessionClosed          | session_id, provider_type, model_id, total_requests, total_errors        | Session shut down                                      |
| SessionDegraded        | session_id, provider_type, model_id, consecutive_errors                  | Session health dropped below threshold                 |
| InferenceCompleted     | session_id, provider_type, latency_ms, input_tokens                      | Successful inference (emitted per-request for metrics) |
| InferenceFailed        | session_id, provider_type, error, input_tokens                           | Failed inference                                       |
| ReindexTriggered       | model_id, total_emails, reason                                           | Re-embedding required due to model change              |
| ReindexProgress        | model_id, emails_processed, total_emails, percent                        | Periodic reindex progress                              |
| ReindexCompleted       | model_id, emails_reindexed, duration_ms, errors                          | Re-embedding finished                                  |
| CloudApiCalled         | provider, endpoint, request_bytes, response_bytes, latency_ms, timestamp | Audit event for every cloud API call                   |

### Event Consumers

| Event                  | Consumed By                             | Purpose                                             |
| ---------------------- | --------------------------------------- | --------------------------------------------------- |
| ProviderChanged        | Email Intelligence                      | Triggers re-embedding if dimensions changed         |
| ReindexTriggered       | Email Intelligence                      | Marks existing embeddings as Stale, queues re-embed |
| ReindexCompleted       | Email Intelligence, Search              | Updates search index availability                   |
| CloudConsentRevoked    | Email Intelligence                      | Stops any in-flight cloud inference                 |
| ModelDownloadCompleted | Email Intelligence (via ProviderConfig) | New model available for use                         |
| CloudApiCalled         | Account Management                      | Audit trail for privacy compliance reporting        |
| InferenceCompleted     | Learning                                | Tracks provider performance for optimization        |
| SessionDegraded        | Email Intelligence                      | Triggers fallback to next provider in chain         |

---

## Value Objects

### ProviderType

```
enum ProviderType {
    Onnx,     // Tier 0: In-process ONNX Runtime via fastembed
    Ollama,   // Tier 1: Local Ollama HTTP API
    Cloud,    // Tier 2: Cloud API (OpenAI, Cohere, Anthropic)
    None,     // No provider (valid only for generative in Tier 0)
    Mock,     // Development/testing only
}
```

### ProviderTier

```
enum ProviderTier {
    Tier0,  // Zero-config default: ONNX embedding, no generative, no cloud
    Tier1,  // Local enhanced: ONNX embedding + Ollama generative
    Tier2,  // Cloud opt-in: Cloud embedding and/or generative (consent required)
}
```

### ModelManifest

| Field        | Type            | Description                                            |
| ------------ | --------------- | ------------------------------------------------------ |
| model_name   | String          | Human-readable name (e.g., "all-MiniLM-L6-v2")         |
| provider     | ProviderType    | Which provider type this model serves                  |
| capability   | ModelCapability | Embedding, Generative, ImageEmbedding                  |
| dimensions   | Option<u32>     | Output vector dimensions (for embedding models)        |
| max_tokens   | u32             | Maximum input token length                             |
| sha256       | String          | Expected SHA-256 checksum of model files               |
| download_url | String          | Hugging Face Hub URL for download                      |
| size_bytes   | u64             | Expected file size                                     |
| quantized    | bool            | Whether this is a quantized (INT8) variant             |
| languages    | Vec<String>     | Supported languages (e.g., ["en"] or ["multilingual"]) |

### ConsentStatus

```
enum ConsentStatus {
    NotRequired,  // No cloud providers configured
    Pending,      // Cloud provider requested but consent not yet given
    Granted,      // User explicitly consented with timestamp
    Revoked,      // User revoked previously granted consent
}
```

### InferenceInput

```
enum InferenceInput {
    Text(String),                    // Single text for embedding
    TextBatch(Vec<String>),          // Batch of texts for embedding
    ClassificationPrompt {           // Structured prompt for generative classification
        email_subject: String,
        email_sender: String,
        email_body_snippet: String,
        candidate_categories: Vec<String>,
    },
    ChatPrompt {                     // Conversational prompt for generative chat
        messages: Vec<ChatMessage>,
        max_tokens: u32,
        temperature: f32,
    },
    Image(Vec<u8>),                  // Raw image bytes for image embedding
}
```

### InferenceResult

```
enum InferenceResult {
    Embedding {
        vector: Vec<f32>,
        dimensions: u32,
        latency_ms: u64,
        provider_used: ProviderType,
        model_used: ModelId,
    },
    EmbeddingBatch {
        vectors: Vec<Vec<f32>>,
        dimensions: u32,
        latency_ms: u64,
        provider_used: ProviderType,
        model_used: ModelId,
    },
    Classification {
        category: String,
        confidence: f32,
        latency_ms: u64,
        provider_used: ProviderType,
    },
    ChatResponse {
        content: String,
        tokens_used: u32,
        latency_ms: u64,
        provider_used: ProviderType,
    },
    ImageEmbedding {
        vector: Vec<f32>,
        dimensions: u32,
        latency_ms: u64,
        provider_used: ProviderType,
    },
}
```

### CloudAuditEntry

| Field             | Type          | Description                                                 |
| ----------------- | ------------- | ----------------------------------------------------------- |
| id                | AuditId       | Unique audit entry identifier                               |
| timestamp         | DateTime      | When the API call was made                                  |
| provider          | CloudProvider | OpenAI, Cohere, Anthropic, Google                           |
| endpoint          | String        | API endpoint called (e.g., "/v1/embeddings")                |
| model             | String        | Model used (e.g., "text-embedding-3-small")                 |
| request_bytes     | u64           | Size of request payload                                     |
| response_bytes    | u64           | Size of response payload                                    |
| input_token_count | Option<u32>   | Tokens sent (if reported by provider)                       |
| latency_ms        | u64           | Round-trip time                                             |
| input_hash        | String        | Truncated SHA-256 of input content (not the content itself) |

### DownloadStatus

```
enum DownloadStatus {
    NotStarted,   // Model registered but not yet downloaded
    Downloading,  // Download in progress
    Ready,        // Downloaded and verified
    Failed,       // Download failed (retryable)
    Corrupted,    // Downloaded but SHA-256 verification failed
}
```

### SessionStatus

```
enum SessionStatus {
    Initializing,  // Model loading into memory
    Ready,         // Accepting inference requests
    Degraded,      // Too many consecutive errors
    Closed,        // Session terminated, resources released
}
```

### ModelCapability

```
enum ModelCapability {
    TextEmbedding,     // Produces vector embeddings from text
    ImageEmbedding,    // Produces vector embeddings from images
    Generative,        // Produces text from prompts
    CrossModal,        // Shared embedding space (text + image, e.g., CLIP)
}
```

### CloudProvider

```
enum CloudProvider {
    OpenAI,
    Cohere,
    Anthropic,
    Google,
    Voyage,
}
```

---

## Domain Services

### ModelDownloader

Downloads models from Hugging Face Hub with progress reporting and SHA-256 verification.

**Responsibilities:**

- Resolves model manifest to a download URL on Hugging Face Hub
- Downloads model files to `~/.emailibrium/models/` with configurable cache directory
- Reports download progress via `ModelDownloadProgress` events
- Verifies file integrity against SHA-256 checksum in the manifest
- Deduplicates concurrent download requests for the same model
- Supports resumable downloads for large model files

**Fallback Order:** Hugging Face Hub (primary) -- no fallback. If download fails, the model remains in `Failed` status and the user is notified.

### EmbeddingRouter

Routes `embed()` calls to the active embedding provider based on ProviderConfig.

**Routing Logic:**

1. Read `embedding_provider` from ProviderConfig
2. If `Onnx`: delegate to the active ONNX inference session (fastembed)
3. If `Ollama`: delegate to the Ollama HTTP client
4. If `Cloud`: verify consent is still granted, then delegate to cloud API client
5. On failure at any tier: attempt graceful fallback to the next lower tier (Cloud fails to Ollama, Ollama fails to Onnx)

**Responsibilities:**

- Provider selection based on current configuration
- Graceful degradation when the configured provider is unavailable
- Request batching for bulk embedding workloads
- Latency tracking per provider for performance monitoring
- Normalization of provider-specific responses to `InferenceResult`

### GenerativeRouter

Routes generative inference calls to the active generative provider.

**Routing Logic:**

1. Read `generative_provider` from ProviderConfig
2. If `None`: return a "generative not available" response (Tier 0 behavior)
3. If `Ollama`: delegate to Ollama HTTP client with model from config
4. If `Cloud`: verify consent, then delegate to cloud API client
5. On failure: fall back to `None` behavior (rule-based heuristic)

**Responsibilities:**

- Classification prompt construction from email metadata
- Chat message history management
- Temperature and token limit enforcement per use case
- Rate limiting for cloud providers (configurable RPM)
- Response parsing and normalization to `InferenceResult`

### ConsentManager

Tracks and enforces user consent for cloud data sharing.

**Responsibilities:**

- Persists consent state to the database (not config file)
- Enforces consent gate before any cloud API call
- Emits `CloudConsentGranted` / `CloudConsentRevoked` events
- Forces provider downgrade on consent revocation
- Provides consent status for UI display (consent dialog, settings page)
- Timestamps all consent state transitions for audit compliance

### ReindexOrchestrator

Coordinates re-embedding of all emails when the active embedding model changes.

**Responsibilities:**

- Listens for `ProviderChanged` events where `requires_reindex == true`
- Determines reindex scope: full reindex (dimension change) or partial (same dimensions, better model)
- Marks existing embeddings as `Stale` in the Email Intelligence context (via published event)
- Queues emails for re-embedding in configurable batch sizes
- Tracks progress and emits `ReindexProgress` events
- Handles interruption gracefully (resume from last checkpoint)
- Emits `ReindexCompleted` when finished

**Reindex Trigger Conditions:**

| Condition                                                            | Reindex Required | Reason                                           |
| -------------------------------------------------------------------- | ---------------- | ------------------------------------------------ |
| Model change, same dimensions (e.g., MiniLM to BGE-small, both 384D) | Yes              | Vectors from different models are not comparable |
| Model change, different dimensions (e.g., 384D to 768D)              | Yes              | HNSW index must be rebuilt                       |
| Provider change, same model (e.g., Onnx to Cloud with same model)    | No               | Same model produces same vectors                 |
| Provider change, different model                                     | Yes              | Different model, different vectors               |

### AuditLogger

Logs all cloud API calls for privacy compliance.

**Responsibilities:**

- Intercepts every outbound cloud API call
- Records metadata (provider, endpoint, size, latency) but never the content itself
- Stores a truncated SHA-256 hash of the input for correlation without exposure
- Persists audit entries to a dedicated audit table in SQLite
- Supports audit log export for compliance reporting
- Enforces configurable retention period (default: 90 days)

---

## Anti-Corruption Layers

### OnnxEmbeddingAdapter

Wraps the `fastembed` crate's `TextEmbedding` API behind the `EmbeddingModel` trait defined in Email Intelligence.

| Domain Method        | fastembed Operation                                              |
| -------------------- | ---------------------------------------------------------------- |
| `embed(text)`        | `TextEmbedding::embed(vec![text], None)` via `spawn_blocking`    |
| `embed_batch(texts)` | `TextEmbedding::embed(texts, None)` via `spawn_blocking`         |
| `dimensions()`       | Determined by `EmbeddingModel` enum variant at construction      |
| `model_id()`         | Mapped from `fastembed::EmbeddingModel` enum to domain `ModelId` |
| `is_available()`     | Always `true` once session is initialized                        |

**Isolation guarantees:**

- The `fastembed` crate is a synchronous API. All calls are wrapped in `tokio::task::spawn_blocking()` to avoid blocking the async runtime.
- fastembed model enum variants are mapped to domain `ModelId` values. The domain never references fastembed types directly.
- Download progress callbacks are translated to domain `ModelDownloadProgress` events.

### OllamaEmbeddingAdapter

Wraps the Ollama HTTP API behind the `EmbeddingModel` trait.

| Domain Method        | Ollama Operation                                                   |
| -------------------- | ------------------------------------------------------------------ |
| `embed(text)`        | `POST /api/embeddings { model, prompt }`                           |
| `embed_batch(texts)` | Sequential or parallel HTTP calls (Ollama does not natively batch) |
| `dimensions()`       | Configured at construction based on selected Ollama model          |
| `is_available()`     | `GET /api/tags` health check                                       |

**Isolation guarantees:**

- Ollama HTTP response format is normalized to `InferenceResult::Embedding`.
- Connection errors are translated to domain error types, never leaked as HTTP errors.
- Ollama model names (e.g., "nomic-embed-text") are mapped to domain `ModelId` values.

### OllamaGenerativeAdapter

Wraps the Ollama HTTP API behind the `GenerativeModel` trait.

| Domain Method      | Ollama Operation                                |
| ------------------ | ----------------------------------------------- |
| `classify(prompt)` | `POST /api/generate { model, prompt, options }` |
| `chat(messages)`   | `POST /api/chat { model, messages, options }`   |
| `is_available()`   | `GET /api/tags` health check                    |

### CloudEmbeddingAdapter

Wraps cloud provider SDKs (OpenAI, Cohere, Voyage) behind the `EmbeddingModel` trait.

| Domain Method        | Cloud Operation                                                            |
| -------------------- | -------------------------------------------------------------------------- |
| `embed(text)`        | Provider-specific embedding API call                                       |
| `embed_batch(texts)` | Provider-specific batch embedding API call                                 |
| `dimensions()`       | Configured per provider/model (e.g., OpenAI text-embedding-3-small = 1536) |
| `is_available()`     | API key present + consent granted + rate limit not exceeded                |

**Isolation guarantees:**

- Each cloud provider has its own adapter implementation. Provider-specific response formats, error codes, and rate limit headers are normalized.
- The consent gate is checked inside the adapter before every API call (defense in depth, even though the router also checks).
- Every call passes through the AuditLogger before reaching the external API.
- API keys are read from environment variables at call time, never cached in structs.

### CloudGenerativeAdapter

Wraps cloud generative APIs (Anthropic, OpenAI, Google) behind the `GenerativeModel` trait.

| Domain Method      | Cloud Operation                                             |
| ------------------ | ----------------------------------------------------------- |
| `classify(prompt)` | Provider-specific completion API with structured output     |
| `chat(messages)`   | Provider-specific chat/messages API                         |
| `is_available()`   | API key present + consent granted + rate limit not exceeded |

---

## Context Map Integration

```
Account Management ──[Published Language]──> AI Providers
  Events: ConsentStatusChanged, PerAccountProviderOverride
  Purpose: Account-level consent and provider preferences

AI Providers ──[Published Language]──> Email Intelligence
  Events: ProviderChanged, ReindexTriggered, ReindexCompleted, SessionDegraded
  Purpose: Provides embeddings and classifications; signals model changes

Email Intelligence ──[Customer/Supplier]──> AI Providers
  Direction: Email Intelligence (customer) defines EmbeddingModel and GenerativeModel traits;
             AI Providers (supplier) implements them via adapters
  Purpose: Email Intelligence dictates the contract; AI Providers conforms

AI Providers ──[Customer/Supplier]──> Hugging Face Hub (external)
  Direction: AI Providers (customer) downloads models per manifest spec
  Purpose: Model acquisition

AI Providers ──[Customer/Supplier]──> Ollama (external, opt-in)
  Direction: AI Providers (customer) calls Ollama HTTP API
  Purpose: Tier 1 generative inference and optional embedding

AI Providers ──[Anti-Corruption Layer]──> Cloud APIs (external, consent-gated)
  Direction: AI Providers wraps each cloud provider SDK
  Purpose: Tier 2 embedding and generative inference
  Constraint: Every call gated by consent status and audited

AI Providers ──[Published Language]──> Learning
  Events: InferenceCompleted (provider performance data)
  Purpose: Learning context tracks provider latency for optimization
```

### Integration Pattern Summary

| Relationship                       | Pattern               | Direction                                                  |
| ---------------------------------- | --------------------- | ---------------------------------------------------------- |
| Account Management -> AI Providers | Published Language    | Account Mgmt publishes consent events                      |
| AI Providers -> Email Intelligence | Published Language    | AI Providers publishes model/reindex events                |
| Email Intelligence -> AI Providers | Customer / Supplier   | Email Intelligence defines traits, AI Providers implements |
| AI Providers -> Hugging Face Hub   | Customer / Supplier   | AI Providers conforms to HF Hub API                        |
| AI Providers -> Ollama             | Customer / Supplier   | AI Providers conforms to Ollama API                        |
| AI Providers -> Cloud APIs         | Anti-Corruption Layer | Each cloud SDK wrapped in an adapter                       |
| AI Providers -> Learning           | Published Language    | AI Providers publishes inference metrics                   |

---

## Ubiquitous Language

| Term                     | Definition                                                                                                                                                                      |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Provider**             | A source of AI inference capability: ONNX (in-process), Ollama (local HTTP), or Cloud (remote API)                                                                              |
| **Tier**                 | A level of capability and privacy trade-off. Tier 0 = ONNX (zero-config, full privacy). Tier 1 = Ollama (local enhanced). Tier 2 = Cloud (maximum quality, data leaves machine) |
| **Consent**              | Explicit user permission to send email data to a cloud provider. Must be granted before the first cloud API call and can be revoked at any time                                 |
| **Reindex**              | The process of re-embedding all emails after an embedding model change. Required because vectors from different models are not comparable                                       |
| **Model manifest**       | Metadata describing an available model: name, dimensions, SHA-256 checksum, download URL, and file size. Used for download verification and registry management                 |
| **Inference session**    | A loaded model that is actively serving requests. ONNX sessions hold the model in memory; Ollama sessions hold an HTTP connection; cloud sessions hold an API client            |
| **Audit trail**          | A log of all data sent to external (cloud) services. Records metadata (provider, endpoint, size, latency) but never the email content itself                                    |
| **Fallback chain**       | The ordered sequence of providers tried when the primary provider fails: Cloud -> Ollama -> ONNX. Degradation always moves toward more privacy, never less                      |
| **Graceful degradation** | When a higher-tier provider is unavailable, the system automatically falls back to a lower tier without user intervention                                                       |
| **Model integrity**      | Verification that a downloaded model file matches its expected SHA-256 checksum. Models that fail verification are quarantined                                                  |
| **Provider routing**     | The process of directing an inference request to the correct provider based on current configuration, consent status, and provider health                                       |
| **Consent gate**         | The check performed before every cloud API call to verify that the user has granted consent. Defense in depth: checked at both the router and adapter levels                    |

---

## Boundaries

- This context does NOT perform email embedding, classification, or clustering logic. That belongs to **Email Intelligence**.
- This context does NOT handle email content extraction or sync. That belongs to **Ingestion**.
- This context does NOT own search query execution or result fusion. That belongs to **Search**.
- This context does NOT manage SONA learning, centroid updates, or feedback processing. That belongs to **Learning**.
- This context does NOT manage email accounts, OAuth tokens, or sync state. That belongs to **Account Management**.
- This context DOES own:
  - The model registry (download, verification, lifecycle)
  - Provider configuration (which provider/model is active)
  - Inference session management (load, serve, close)
  - Provider routing and fallback logic
  - Cloud consent enforcement
  - Cloud API audit logging
  - Reindex orchestration (triggering and progress tracking, not the actual re-embedding)
  - All anti-corruption layers that wrap external AI systems (fastembed, Ollama, cloud SDKs)

---

## Configuration Mapping

The AI Providers context owns the following configuration sections (from `~/.emailibrium/config.yaml`):

| YAML Path                | Aggregate                       | Description                             |
| ------------------------ | ------------------------------- | --------------------------------------- |
| `ai.embedding.provider`  | ProviderConfig                  | Active embedding provider               |
| `ai.embedding.onnx.*`    | ProviderConfig, ModelRegistry   | ONNX model selection and cache          |
| `ai.embedding.ollama.*`  | ProviderConfig                  | Ollama embedding connection             |
| `ai.embedding.cloud.*`   | ProviderConfig                  | Cloud embedding provider and model      |
| `ai.generative.provider` | ProviderConfig                  | Active generative provider              |
| `ai.generative.ollama.*` | ProviderConfig                  | Ollama generative models and parameters |
| `ai.generative.cloud.*`  | ProviderConfig                  | Cloud generative provider and model     |
| `ai.image_embedding.*`   | ProviderConfig                  | Image embedding toggle and provider     |
| `ai.consent.*`           | ProviderConfig (ConsentManager) | Consent and audit settings              |
| `ai.integrity.*`         | ModelRegistry                   | Checksum verification settings          |
| `ai.cache.*`             | InferenceSession                | Embedding cache size                    |

Environment variable overrides use the `EMAILIBRIUM_AI__` prefix with double-underscore nesting (e.g., `EMAILIBRIUM_AI__EMBEDDING__PROVIDER=ollama`).

---

## Implementation Notes

### Provider Resolution Order

```
1. Read ai.embedding.provider from config (default: "onnx")
2. If "onnx":
   a. Check if model files exist in model_path
   b. If not, download from Hugging Face Hub (with progress)
   c. Verify SHA-256 checksum
   d. Initialize fastembed TextEmbedding
   e. Return OnnxEmbeddingAdapter
3. If "ollama":
   a. Check if Ollama is reachable at configured URL
   b. If not, log warning and fall back to ONNX (graceful degradation)
   c. Return OllamaEmbeddingAdapter
4. If "cloud":
   a. Check if consent has been granted
   b. Check if API key env var is set
   c. If either fails, log error and fall back to ONNX
   d. Return CloudEmbeddingAdapter
```

### Default Model Selection

| Capability                    | Default Model         | Dimensions | Size                              | Rationale                                                 |
| ----------------------------- | --------------------- | ---------- | --------------------------------- | --------------------------------------------------------- |
| Text embedding (Tier 0)       | all-MiniLM-L6-v2      | 384        | ~90 MB                            | Smallest high-quality model; matches existing HNSW config |
| Text embedding (multilingual) | multilingual-e5-small | 384        | ~113 MB                           | Same dimensions as default; drop-in swap                  |
| Generative (Tier 1)           | llama3.2:3b           | N/A        | ~2.5 GB                           | Best instruction-following at 3B scale                    |
| Image embedding               | CLIP ViT-B-32         | 512        | ~240 MB (text) + ~340 MB (vision) | Cross-modal text/image search                             |

### Performance Targets

| Metric                 | Tier 0 (ONNX) | Tier 1 (Ollama)             | Tier 2 (Cloud) |
| ---------------------- | ------------- | --------------------------- | -------------- |
| Single embed latency   | <50 ms        | <200 ms                     | <500 ms        |
| Batch embed (32 items) | <400 ms       | <2 sec                      | <3 sec         |
| Model load time        | <800 ms       | N/A (daemon)                | N/A (API)      |
| First-run download     | 5-30 sec      | N/A (user pulls)            | N/A            |
| Memory (runtime)       | ~150-250 MB   | Ollama process: ~500-800 MB | Negligible     |
