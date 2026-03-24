# DDD-003: Ingestion Domain

| Field   | Value             |
| ------- | ----------------- |
| Status  | Accepted          |
| Date    | 2026-03-23        |
| Type    | Supporting Domain |
| Context | Ingestion         |

## Overview

The Ingestion bounded context manages the end-to-end pipeline for bringing emails into Emailibrium. It handles bulk ingestion jobs, multi-asset content extraction (HTML, images, attachments, URLs), progress tracking via SSE, and publishes events that trigger downstream embedding and classification in the Email Intelligence context.

## Aggregates

### 1. IngestionJobAggregate

Manages the lifecycle of a bulk ingestion job for an email account.

**Root Entity: IngestionJob**

| Field        | Type               | Description                                 |
| ------------ | ------------------ | ------------------------------------------- |
| id           | IngestionJobId     | Unique job identifier                       |
| account_id   | AccountId          | The email account being ingested            |
| status       | JobStatus          | Pending, Running, Paused, Completed, Failed |
| total        | u32                | Total emails to process                     |
| processed    | u32                | Emails processed so far                     |
| embedded     | u32                | Emails successfully embedded                |
| failed       | u32                | Emails that failed processing               |
| phase        | IngestionPhase     | Current pipeline phase                      |
| started_at   | DateTime           | Job start timestamp                         |
| completed_at | Option\<DateTime\> | Job completion timestamp                    |

**Invariants:**

- Only one active ingestion job per account at a time.
- Phase transitions must follow the defined order: Syncing --> Embedding --> Categorizing --> Clustering --> Analyzing --> Complete.
- `processed + failed` must not exceed `total`.
- A failed job can be retried; retry creates a new IngestionJob that resumes from the last checkpoint.

**Commands:**

- `StartIngestion { account_id, sync_depth }` -- begins a new ingestion job
- `PauseIngestion { job_id }` -- pauses an active job
- `ResumeIngestion { job_id }` -- resumes a paused job
- `RetryIngestion { job_id }` -- retries a failed job from last checkpoint
- `CancelIngestion { job_id }` -- cancels a running job

### 2. ContentExtractionAggregate

Manages multi-asset extraction for a single email.

**Root Entity: ContentExtraction**

| Field                 | Type                              | Description                      |
| --------------------- | --------------------------------- | -------------------------------- |
| email_id              | EmailId                           | The email being extracted        |
| html_extracted        | ExtractionResult                  | HTML body extraction result      |
| images_ocrd           | Vec\<ImageExtractionResult\>      | OCR results for inline images    |
| attachments_extracted | Vec\<AttachmentExtractionResult\> | Text extraction from attachments |
| urls_resolved         | Vec\<UrlResolutionResult\>        | URL analysis results             |
| quality_scores        | QualityScores                     | Per-asset quality metrics        |
| extracted_at          | DateTime                          | Extraction timestamp             |

**Invariants:**

- HTML extraction is mandatory; image/attachment/URL extraction is best-effort.
- Each extraction result includes a quality score; assets below a configurable quality threshold are flagged but not discarded.
- Failed extractions are recorded with error details (not silently dropped).

**Commands:**

- `ExtractContent { email_id, raw_email }` -- runs the full extraction pipeline
- `RetryExtraction { email_id, asset_type }` -- retries a failed extraction for a specific asset

## Domain Events

| Event                 | Fields                                                 | Published When                                   |
| --------------------- | ------------------------------------------------------ | ------------------------------------------------ |
| IngestionStarted      | job_id, account_id, total_emails                       | A new ingestion job begins                       |
| IngestionPhaseChanged | job_id, phase                                          | The job transitions to a new pipeline phase      |
| IngestionProgress     | job_id, processed, embedded, failed, emails_per_second | Periodic progress update (streamed via SSE)      |
| IngestionCompleted    | job_id, report (summary statistics)                    | The job finishes successfully                    |
| IngestionFailed       | job_id, error, last_checkpoint                         | The job fails with an error                      |
| ContentExtracted      | email_id, asset_type, quality_score                    | An asset is successfully extracted from an email |
| ExtractionFailed      | email_id, asset_type, error                            | An asset extraction fails                        |

### Event Consumers

| Event              | Consumed By        | Purpose                                                 |
| ------------------ | ------------------ | ------------------------------------------------------- |
| ContentExtracted   | Email Intelligence | Triggers embedding generation for the extracted content |
| IngestionCompleted | Email Intelligence | Triggers batch clustering and analysis                  |
| IngestionStarted   | Frontend (via SSE) | Initiates progress UI                                   |
| IngestionProgress  | Frontend (via SSE) | Updates progress bar and statistics                     |

## Value Objects

### IngestionPhase

```
enum IngestionPhase {
    Syncing,       -- Fetching emails from provider
    Embedding,     -- Generating vector embeddings
    Categorizing,  -- Running classification
    Clustering,    -- Running topic clustering
    Analyzing,     -- Running analytics and quality checks
    Complete,      -- All phases done
}
```

### ExtractionQuality

| Field      | Type          | Description                                                        |
| ---------- | ------------- | ------------------------------------------------------------------ |
| score      | f32           | Quality score [0.0, 1.0]                                           |
| confidence | f32           | Confidence in the quality assessment                               |
| warnings   | Vec\<String\> | Quality warnings (e.g., "low OCR confidence", "truncated content") |

### AssetType

```
enum AssetType {
    HtmlBody,       -- The email's HTML/text body
    InlineImage,    -- Images embedded in the email
    Attachment,     -- File attachments (PDF, DOCX, XLSX, etc.)
    Url,            -- URLs found in the email body
}
```

### ExtractionResult

| Field   | Type              | Description              |
| ------- | ----------------- | ------------------------ |
| status  | ExtractionStatus  | Success, Failed, Skipped |
| content | Option\<String\>  | Extracted text content   |
| quality | ExtractionQuality | Quality metrics          |
| error   | Option\<String\>  | Error message if failed  |

## Domain Services

### IngestionPipeline

Orchestrates the 6-stage per-email pipeline.

**Pipeline Stages:**

1. **Sync** -- Fetch raw email from provider via Account Management context
2. **Extract** -- Run content extraction (HTML, images, attachments, URLs)
3. **Embed** -- Generate vector embeddings via Email Intelligence context
4. **Categorize** -- Classify via Email Intelligence context
5. **Cluster** -- Assign to topic clusters via Email Intelligence context
6. **Analyze** -- Run quality checks and generate analytics

**Responsibilities:**

- Manages backpressure: limits concurrent email processing based on system resources.
- Implements checkpoint/resume: records progress so failed jobs can restart.
- Handles rate limiting from email providers (Gmail API quotas, Graph API throttling).
- Emits IngestionProgress events at configurable intervals for SSE streaming.

### HtmlExtractor

Extracts clean text from email HTML bodies.

**Pipeline:** `raw HTML --> ammonia (sanitize) --> scraper (parse) --> html2text (convert)`

**Responsibilities:**

- Strips tracking pixels and invisible elements.
- Preserves semantic structure (headers, lists, links).
- Handles malformed HTML gracefully.
- Outputs clean text suitable for embedding.

### ImageAnalyzer

Processes inline images for OCR text and visual embeddings.

**Tools:**

- OCR: `ocrs` crate for text extraction from images
- Visual embedding: `fastembed` with CLIP model

**Responsibilities:**

- Filters out tracking pixels and decorative images (< 50x50 px or known tracker domains).
- Runs OCR on images containing text.
- Generates CLIP embeddings for visual content.
- Reports quality scores based on OCR confidence and image resolution.

### AttachmentExtractor

Extracts text content from file attachments.

**Supported Formats:**
| Format | Tool |
|--------|------|
| PDF | `pdf-extract` crate |
| DOCX | `dotext` crate |
| XLSX/CSV | `calamine` crate |
| Plain text | Direct read |
| Other | File type detection via `infer` crate; unsupported types are skipped |

**Responsibilities:**

- Detects file type via magic bytes (`infer` crate), not file extension.
- Extracts text content with structure preservation where possible.
- Handles encrypted/password-protected files gracefully (marks as Skipped).
- Enforces size limits to prevent memory exhaustion on large attachments.

### LinkAnalyzer

Extracts and analyzes URLs found in email bodies.

**Responsibilities:**

- Extracts URLs from HTML content and plain text.
- Resolves redirects (follows up to 5 hops).
- Detects and flags tracking URLs (UTM parameters, known tracker domains).
- Extracts destination domain and title for metadata enrichment.

### ProgressStreamer

Streams ingestion progress to the frontend via Server-Sent Events.

**Responsibilities:**

- Converts IngestionProgress domain events to SSE format.
- Manages SSE connections (heartbeat, reconnection).
- Throttles updates to avoid overwhelming the frontend (max 2 updates/second).
- Provides final summary when IngestionCompleted fires.

## Context Map

### Upstream Dependencies

| Context            | Dependency                    | What Ingestion Consumes                                   |
| ------------------ | ----------------------------- | --------------------------------------------------------- |
| Account Management | Provider credentials and sync | OAuth tokens, provider-specific sync APIs, email metadata |

### Downstream Consumers

| Context            | Relationship        | What Ingestion Publishes                                     |
| ------------------ | ------------------- | ------------------------------------------------------------ |
| Email Intelligence | Customer / Supplier | ContentExtracted events trigger embedding and classification |

### Integration Pattern: Customer / Supplier

Email Intelligence (the customer) defines the content format it needs. Ingestion (the supplier) conforms to that format when publishing ContentExtracted events. The event payload includes:

- Extracted text content
- Asset type (so Intelligence knows which collection to embed into)
- Quality score (so Intelligence can decide whether to embed low-quality content)

## Ubiquitous Language

| Term              | Definition                                                               |
| ----------------- | ------------------------------------------------------------------------ |
| **Ingestion**     | The process of bringing emails from a provider into Emailibrium          |
| **Extraction**    | Pulling text and metadata from email content (HTML, images, attachments) |
| **Phase**         | A discrete stage in the ingestion pipeline                               |
| **Checkpoint**    | A saved progress point allowing job resumption after failure             |
| **Asset**         | A piece of content within an email (body, image, attachment, URL)        |
| **Quality score** | A metric indicating how well content was extracted                       |
| **Backpressure**  | Rate limiting within the pipeline to prevent resource exhaustion         |

## Boundaries

- This context does NOT generate embeddings or classify emails (that is Email Intelligence). It extracts content and publishes it for downstream processing.
- This context does NOT manage email account credentials or OAuth flows (that is Account Management). It consumes credentials provided by Account Management.
- This context does NOT handle user-facing search (that is Search).
- This context DOES own the ingestion pipeline, content extraction, progress tracking, and job lifecycle.
