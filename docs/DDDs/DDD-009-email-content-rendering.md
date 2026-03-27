# DDD-009: Email Content & Attachments Domain

| Field   | Value                       |
| ------- | --------------------------- |
| Status  | Proposed                    |
| Date    | 2026-03-25                  |
| Type    | Core Domain                 |
| Context | Email Content & Attachments |

## Overview

The Email Content & Attachments bounded context handles the extraction, sanitization, storage, and serving of email body content (HTML and plain text) and file attachments. It sits between the Ingestion context (which fetches raw messages from provider APIs) and the Email Operations context (which manages email lifecycle and state). This is a **core domain** because rendering email content faithfully and securely is a primary user-facing capability that directly determines whether Emailibrium is usable as a daily email client.

## Strategic Classification

| Aspect              | Value                                                                           |
| ------------------- | ------------------------------------------------------------------------------- |
| Domain type         | Core                                                                            |
| Investment priority | Critical (email body rendering and attachments are table-stakes UX)             |
| Complexity driver   | HTML sanitization security, MIME format diversity, provider API differences     |
| Change frequency    | Medium (new providers, new MIME types, security patches)                        |
| Risk                | XSS via unsanitized HTML, data loss on attachment fetch failure, path traversal |

---

## Ubiquitous Language

| Term                      | Definition                                                                                                                  |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| **Body HTML**             | The HTML representation of an email body, extracted from the provider and sanitized server-side with ammonia before storage |
| **Body Text**             | The plain-text representation of an email body, used for search indexing, previews, and fallback rendering                  |
| **Content Type Strategy** | The rendering decision based on available content: HTML-primary with plain-text toggle, or plain-text-only                  |
| **Inline Attachment**     | An attachment embedded within the email body via CID reference (e.g., logos, signature images)                              |
| **File Attachment**       | A non-inline attachment that the user can download (documents, archives, media)                                             |
| **CID Resolution**        | The process of replacing `cid:` URI references in HTML with base64 data URIs or API URLs                                    |
| **Sanitized HTML**        | HTML that has passed through ammonia's whitelist-based sanitizer, safe for storage and rendering                            |
| **Attachment Metadata**   | The database record describing an attachment (filename, type, size, storage path) without the file content                  |
| **Lazy Fetch**            | The strategy of storing only attachment metadata during sync and fetching actual file content on first download request     |
| **Storage Path**          | The filesystem location where attachment content is cached: `data/attachments/{account_id}/{email_id}/{filename}`           |
| **ZIP Stream**            | A server-side streamed ZIP archive containing all file attachments for a single email                                       |

---

## Aggregates

### 1. EmailContentAggregate

Manages the body content of an email message, including HTML sanitization and content type selection.

**Root Entity: EmailContent**

| Field             | Type             | Description                                          |
| ----------------- | ---------------- | ---------------------------------------------------- |
| email_id          | EmailId          | Foreign key to the email message                     |
| body_html         | Option\<String\> | Sanitized HTML body (ammonia-processed)              |
| body_text         | Option\<String\> | Plain text body                                      |
| content_type      | ContentType      | `HtmlOnly`, `TextOnly`, `Multipart` (both available) |
| has_inline_images | bool             | Whether CID references exist and have been resolved  |
| sanitized_at      | Timestamp        | When ammonia sanitization was last applied           |

**Value Objects:**

- `ContentType` — Enum: `HtmlOnly`, `TextOnly`, `Multipart`
- `SanitizationConfig` — Ammonia builder configuration (allowed tags, attributes, URL schemes)

**Domain Events:**

| Event                   | Trigger                                          | Payload                                             |
| ----------------------- | ------------------------------------------------ | --------------------------------------------------- |
| `HtmlBodyExtracted`     | Provider fetcher extracts HTML from API response | email_id, raw_html_length                           |
| `HtmlBodySanitized`     | Ammonia sanitization completes                   | email_id, sanitized_html_length, tags_removed_count |
| `CidReferencesResolved` | Inline images replaced with data URIs            | email_id, cid_count                                 |

**Invariants:**

1. `body_html` must always be ammonia-sanitized before storage — never store raw provider HTML.
2. At least one of `body_html` or `body_text` must be present.
3. CID resolution must occur before ammonia sanitization (ammonia whitelists `data:` scheme).

---

### 2. AttachmentAggregate

Manages the lifecycle of email attachments from metadata extraction through content caching and download serving.

**Root Entity: Attachment**

| Field                  | Type             | Description                                                  |
| ---------------------- | ---------------- | ------------------------------------------------------------ |
| id                     | AttachmentId     | UUID, primary key                                            |
| email_id               | EmailId          | Foreign key to the email message                             |
| account_id             | AccountId        | The account this attachment belongs to                       |
| filename               | String           | Sanitized filename (path separators and null bytes stripped) |
| content_type           | String           | MIME type (e.g., `application/pdf`, `image/png`)             |
| size_bytes             | u64              | Size in bytes                                                |
| is_inline              | bool             | Whether this is a CID-referenced inline image                |
| content_id             | Option\<String\> | Content-ID header value for CID resolution                   |
| storage_path           | Option\<String\> | Filesystem path (None if not yet fetched from provider)      |
| provider_attachment_id | Option\<String\> | Provider-specific ID for lazy fetching                       |
| fetch_status           | FetchStatus      | `Pending`, `Fetched`, `Failed`                               |

**Value Objects:**

- `FetchStatus` — Enum: `Pending` (metadata only), `Fetched` (content cached on disk), `Failed` (fetch error, retryable)
- `StoragePath` — Validated filesystem path under `data/attachments/`
- `SanitizedFilename` — Filename with path separators, null bytes, and control characters removed; max 255 chars

**Domain Events:**

| Event                      | Trigger                                                        | Payload                                        |
| -------------------------- | -------------------------------------------------------------- | ---------------------------------------------- |
| `AttachmentMetadataStored` | Email sync extracts attachment metadata from provider response | email_id, attachment_id, filename, size_bytes  |
| `AttachmentContentFetched` | Lazy fetch retrieves content from provider API                 | attachment_id, storage_path, fetch_duration_ms |
| `AttachmentContentFailed`  | Provider API fetch fails                                       | attachment_id, error_message, retry_eligible   |
| `AttachmentDownloaded`     | User downloads an attachment                                   | attachment_id, download_type (single \| zip)   |
| `AttachmentsCleaned`       | Cascade delete removes attachment files from disk              | email_id, files_removed_count, bytes_freed     |

**Invariants:**

1. `filename` must be sanitized — no path separators (`/`, `\`), no null bytes, max 255 characters.
2. `storage_path` must be under `data/attachments/` — never allow arbitrary filesystem paths.
3. An attachment with `fetch_status = Fetched` must have a valid `storage_path` pointing to an existing file.
4. When an email is deleted, all associated attachments and their filesystem content must be cleaned up.
5. Inline attachments (`is_inline = true`) must have a non-null `content_id`.

---

## Domain Services

### ContentExtractionService

Extracts HTML and plain-text bodies from provider API responses.

| Operation              | Input                  | Output                           | Notes                                                                                            |
| ---------------------- | ---------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------ |
| `extract_gmail_body`   | Gmail message payload  | (Option\<html\>, Option\<text\>) | Recursively traverses MIME parts for `text/html` and `text/plain`; base64url-decodes `body.data` |
| `extract_outlook_body` | Graph API message body | (Option\<html\>, Option\<text\>) | Uses `body.content` when `body.contentType === "html"`                                           |
| `extract_imap_body`    | Raw MIME bytes         | (Option\<html\>, Option\<text\>) | Uses `mail-parser` crate for zero-copy MIME parsing                                              |

### HtmlSanitizationService

Applies ammonia-based sanitization with an email-specific configuration.

| Operation                | Input                               | Output                | Notes                                                                                                                                                                                                                     |
| ------------------------ | ----------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sanitize_email_html`    | Raw HTML string                     | Sanitized HTML string | Whitelist: standard email tags (table, td, th, div, span, img, a, etc.). Allows `style`, `class`, `href`, `src` attributes. Sets `link_rel("noopener noreferrer")`. Allows `http`, `https`, `mailto`, `data` URL schemes. |
| `resolve_cid_references` | HTML string + inline attachment map | HTML with data URIs   | Replaces `cid:{id}` with `data:{type};base64,{content}`. Must run before `sanitize_email_html`.                                                                                                                           |

### AttachmentFetchService

Fetches attachment content from provider APIs on demand.

| Operation                  | Input                                          | Output      | Notes                                                                                         |
| -------------------------- | ---------------------------------------------- | ----------- | --------------------------------------------------------------------------------------------- |
| `fetch_gmail_attachment`   | account credentials, message_id, attachment_id | bytes       | `GET /gmail/v1/users/me/messages/{id}/attachments/{att_id}`; base64url-decode response `data` |
| `fetch_outlook_attachment` | account credentials, message_id, attachment_id | bytes       | `GET /me/messages/{id}/attachments/{att_id}/$value`; raw binary response                      |
| `cache_to_filesystem`      | bytes, target storage_path                     | StoragePath | Writes bytes to disk; creates parent directories; returns validated path                      |

### AttachmentServingService

Serves attachment content to the frontend via streaming HTTP responses.

| Operation       | Input                       | Output                  | Notes                                                                            |
| --------------- | --------------------------- | ----------------------- | -------------------------------------------------------------------------------- |
| `stream_single` | attachment metadata         | Axum streaming response | Sets `Content-Type`, `Content-Disposition: attachment`, `Content-Length` headers |
| `stream_zip`    | list of attachment metadata | Axum streaming response | Uses `async_zip` to create ZIP archive streamed via `tokio::io::duplex`          |

---

## Integration Points

### Upstream: Ingestion Context (DDD-003)

- **Published Language**: `EmailFetched` event carries raw provider API response.
- **Integration pattern**: Customer/Supplier — Ingestion supplies raw content; this context extracts and sanitizes.
- **Data flow**: Raw provider JSON → ContentExtractionService → sanitized body + attachment metadata.

### Upstream: Account Management Context (DDD-005)

- **Published Language**: OAuth tokens and provider credentials for lazy attachment fetching.
- **Integration pattern**: Conformist — This context uses Account Management's token refresh without modification.

### Downstream: Email Operations Context (DDD-008)

- **Published Language**: `EmailContent` and `Attachment` entities are owned by this context but referenced by Email Operations for display.
- **Integration pattern**: Shared Kernel — The `email_id` foreign key links content to the Email Operations aggregate.

### Downstream: Email Intelligence Context (DDD-001)

- **Published Language**: `body_text` is consumed by the embedding pipeline for vector indexing.
- **Integration pattern**: Customer/Supplier — Intelligence consumes plain text; this context is the authoritative source.

---

## Context Map Update

This context should be added to DDD-000 (Context Map):

```text
                  ┌─────────────────────────┐
                  │   Email Content &       │
                  │   Attachments (Core)    │
                  │                         │
                  │  EmailContent,          │
                  │  Attachment,            │
                  │  Sanitization           │
                  └──────┬──────────┬───────┘
                         │          │
              Shared Kernel    Customer/Supplier
              (email_id)       (body_text for
                │              embeddings)
                ▼                   ▼
    ┌───────────────────┐  ┌──────────────────┐
    │  Email Operations │  │ Email Intelligence│
    │     (DDD-008)     │  │    (DDD-001)     │
    └───────────────────┘  └──────────────────┘
```

---

## Anti-Corruption Layer

### Provider Response Translation

Each provider returns attachment metadata in a different format:

| Provider | Attachment ID Field | Content Field                        | Encoding                   |
| -------- | ------------------- | ------------------------------------ | -------------------------- |
| Gmail    | `body.attachmentId` | `data`                               | base64url                  |
| Outlook  | `id`                | `contentBytes` or `/$value` endpoint | base64 or raw              |
| IMAP     | MIME part index     | MIME part content                    | quoted-printable or base64 |

The `ContentExtractionService` and `AttachmentFetchService` act as the ACL, translating provider-specific structures into the domain's `EmailContent` and `Attachment` entities.

### Filename Sanitization ACL

Provider filenames may contain:

- Path traversal sequences (`../`, `..\\`)
- Null bytes
- OS-reserved characters
- Excessively long names
- Unicode normalization issues

The `SanitizedFilename` value object enforces safety at the domain boundary.
