# ADR-020: Email Attachment Management

- **Status**: Proposed
- **Date**: 2026-03-25
- **Extends**: DDD-008 (Email Operations), DDD-009 (Email Content & Attachments)
- **Research**: docs/research/email-interaction-enhancements.md

## Context

The Email TypeScript interface carries `hasAttachments: boolean` and the backend defines a `RawAttachment` struct with filename, content_type, data bytes, is_inline, and content_id. However:

1. **No attachment API endpoints** — The frontend displays a placeholder: "Attachment downloads are not yet available."
2. **No attachment storage** — Attachments from provider APIs are not persisted or served.
3. **No inline image resolution** — HTML emails with CID-referenced images (`<img src="cid:...">`) render broken images.
4. **No bulk download** — Users cannot download all attachments from an email as a ZIP archive.

Both Gmail and Outlook APIs provide attachment endpoints, and the existing `reqwest` client can fetch them without new HTTP dependencies.

## Decision

### 1. Attachment Metadata Schema

Add an `attachments` table to the SQLite database:

```sql
CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    email_id TEXT NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    is_inline BOOLEAN NOT NULL DEFAULT FALSE,
    content_id TEXT,
    storage_path TEXT NOT NULL,
    provider_attachment_id TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_attachments_email_id ON attachments(email_id);
CREATE INDEX idx_attachments_content_id ON attachments(content_id) WHERE content_id IS NOT NULL;
```

### 2. Filesystem Storage

Store attachment content on the local filesystem:

```
data/attachments/{account_id}/{email_id}/{sanitized_filename}
```

**Rationale over alternatives:**

| Strategy | Decision | Reason |
|---|---|---|
| Filesystem | **Adopted** | Simple, fast, supports streaming, works with Axum's `ReaderStream` |
| SQLite BLOB | Rejected | Bloats database, prevents streaming, slow for large files |
| S3/MinIO | Deferred | Added infrastructure complexity; revisit when cloud deployment is planned |

**Path sanitization**: Filenames must be sanitized to prevent directory traversal. Strip path separators, null bytes, and limit length to 255 characters. Generate a UUID-based fallback if the filename is empty or invalid.

### 3. API Endpoints

Three new Axum routes under the existing `/api/v1/emails` prefix:

| Endpoint | Method | Response | Description |
|---|---|---|---|
| `/api/v1/emails/{id}/attachments` | GET | JSON array | List attachment metadata (id, filename, contentType, sizeBytes, isInline) |
| `/api/v1/emails/{id}/attachments/{att_id}` | GET | Binary stream | Download single attachment with `Content-Disposition: attachment` |
| `/api/v1/emails/{id}/attachments/zip` | GET | Binary stream | Download all non-inline attachments as a ZIP archive |

**Streaming**: Both download endpoints use `tokio_util::io::ReaderStream` to stream file content directly from disk without buffering entire files in memory.

### 4. ZIP Streaming

Use `async_zip` (native tokio support) to create ZIP archives streamed directly into the HTTP response via `tokio::io::duplex`:

- A background task writes attachment files into the ZIP writer.
- The reader side is wrapped in `ReaderStream` and returned as an Axum `Body`.
- Compression: Deflate for reasonable size reduction without blocking the event loop excessively.

**New Cargo dependency:**
```toml
async_zip = { version = "0.0.17", features = ["tokio", "deflate"] }
```

### 5. Provider-Specific Attachment Fetching

#### Gmail

- Attachment content is fetched via `GET /gmail/v1/users/me/messages/{messageId}/attachments/{attachmentId}`.
- The `attachmentId` comes from the message payload's `body.attachmentId` field for parts stored separately.
- Response `data` field is base64url-encoded.

#### Outlook / Microsoft Graph

- List: `GET /me/messages/{messageId}/attachments`
- Get raw: `GET /me/messages/{messageId}/attachments/{attachmentId}/$value`
- Response includes `contentBytes` (base64-encoded) or raw bytes via `/$value`.
- Handle three attachment types: `fileAttachment`, `itemAttachment`, `referenceAttachment`.

#### Fetch Strategy

**Lazy fetch with caching:**
1. During email sync, store only attachment metadata (from the message payload) in the `attachments` table.
2. On first download request, fetch the actual content from the provider API and cache to filesystem.
3. Subsequent downloads serve from the filesystem cache.

This avoids downloading attachments the user never opens, reducing sync time and storage usage.

### 6. Inline Image (CID) Resolution

For HTML emails with `<img src="cid:...">` references:

1. During `body_html` preparation, resolve CID references to base64 data URIs.
2. Look up the inline attachment by matching `Content-ID` header against the CID value.
3. Replace `cid:{content_id}` with `data:{content_type};base64,{encoded_data}`.
4. Perform this substitution **before** ammonia sanitization (ammonia whitelists `data:` scheme for images).

**Rationale for data URI over API URL:** Eliminates extra HTTP round-trips. Inline images are typically small (logos, signatures) and the base64 overhead is acceptable.

### 7. Frontend Type Extension

Extend the existing `Email` TypeScript interface:

```typescript
interface Attachment {
  id: string;
  filename: string;
  contentType: string;
  sizeBytes: number;
  isInline: boolean;
}

interface Email {
  // ... existing fields ...
  attachments: Attachment[];  // replaces hasAttachments boolean
}
```

Retain `hasAttachments` as a computed getter (`attachments.length > 0`) for backward compatibility with existing UI logic.

## Consequences

### Positive

- **Complete email experience** — Users can view, download, and bulk-export attachments like any native email client.
- **Inline images work** — CID-referenced images render correctly in the sandboxed iframe (ADR-019).
- **Streaming architecture** — No memory bloat for large attachments; everything streams from disk to HTTP response.
- **Lazy fetching** — Sync performance unaffected; attachments are fetched on demand.
- **Minimal new dependencies** — One new Rust crate (`async_zip`). Frontend uses only existing dependencies.

### Negative

- **Filesystem management** — Need cleanup logic when emails are deleted (CASCADE on the DB side; filesystem cleanup via a background task or delete handler).
- **Storage growth** — Cached attachments accumulate on disk. Need an eviction policy or storage budget in a future iteration.
- **Provider rate limits** — Fetching many attachments in rapid succession could hit Gmail/Outlook API rate limits. Mitigated by lazy fetch and per-account rate limiting.

### Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Path traversal via malicious filename | Medium | Sanitize filenames; strip separators, null bytes; use UUID fallback |
| Disk space exhaustion | Low | Monitor storage; implement LRU eviction in a future iteration |
| Provider attachment format changes | Low | Abstracted behind provider trait; changes isolated to fetcher implementations |
| ZIP bomb (maliciously nested attachments) | Very Low | ZIP is created from known attachments, not from untrusted ZIP input |

## Alternatives Considered

### Client-Side ZIP with JSZip + file-saver

**Deferred as fallback.** Server-side ZIP streaming is more efficient (doesn't require fetching all attachments to the browser first). JSZip can be introduced later if offline/PWA support requires client-side archive creation.

### Store Attachments in SQLite BLOBs

**Rejected.** SQLite BLOB storage prevents streaming, bloats the database file, and complicates backup/restore. The database should store only metadata.

### Always Fetch Attachments During Sync

**Rejected.** Would significantly slow initial sync, consume unnecessary bandwidth and storage for attachments the user may never open. Lazy fetch is the correct strategy.
