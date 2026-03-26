# Email Interaction Enhancements — Implementation Plan

| Field     | Value                          |
| --------- | ------------------------------ |
| Status    | In Progress                    |
| Date      | 2026-03-25                     |
| Updated   | 2026-03-25                     |
| ADRs      | ADR-019, ADR-020               |
| DDD       | DDD-009                        |
| Research  | docs/research/email-interaction-enhancements.md |

---

## Priority Matrix

Features are prioritized using four levels based on user impact, security criticality, and dependency ordering. Each item within a priority level is ordered by implementation sequence.

---

## Critical Priority

> Must be completed first. These items address security vulnerabilities and the fundamental inability to read email content properly.

### C1. Backend: Extract `body_html` from Gmail and Outlook APIs — COMPLETED

**Why critical:** Without HTML body extraction, all downstream rendering work has no data to display. This is the foundation that unblocks everything else.

| Attribute | Value |
|---|---|
| ADR | ADR-019 §3 |
| DDD | DDD-009 — ContentExtractionService |
| Effort | Small (2–3 days) |
| Files affected | `backend/src/email/gmail.rs`, `backend/src/email/outlook.rs`, `backend/src/email/types.rs` |
| Dependencies | None |
| Acceptance criteria | `body_html` populated for newly synced emails from both providers; existing `body_text` extraction unaffected |
| **Status** | **COMPLETED — 2026-03-25** |

**Scope:**
- Gmail: Traverse `payload.parts[]` recursively to find `text/html` MIME part; base64url-decode `body.data`.
- Outlook: Use `body.content` when `body.contentType === "html"`.
- Store `body_html` in the `emails` table (add column if not present).
- Backfill: Add a migration or re-sync mechanism for existing emails missing `body_html`.

**Completion notes:**
- Added `body_html: Option<String>` field to `EmailMessage` struct in `types.rs`.
- Gmail: Added `extract_body_html()` with recursive MIME traversal via `find_html_in_parts()` helper; handles direct payload, top-level parts, and nested multipart/alternative. 5 new tests.
- Outlook: Updated `parse_message()` to distinguish `body.contentType == "html"` vs `"text"`; HTML goes to `body_html`, text goes to `body`. 4 new tests.
- Updated all other `EmailMessage` construction sites (`imap.rs`, `sync.rs`, `rule_processor.rs`, `api/rules.rs`, `tests/email_providers.rs`) with `body_html: None`.
- DB schema already had `body_html TEXT DEFAULT ''` — no migration needed.
- All 662 backend tests pass.

---

### C2. Backend: Server-side HTML sanitization with ammonia — COMPLETED

**Why critical:** Storing unsanitized HTML in the database is a persistent XSS vector. This must be in place before any HTML is served to the frontend.

| Attribute | Value |
|---|---|
| ADR | ADR-019 §3 |
| DDD | DDD-009 — HtmlSanitizationService |
| Effort | Small (1–2 days) |
| Files affected | `backend/src/content/` (new service), `backend/src/email/gmail.rs`, `backend/src/email/outlook.rs` |
| Dependencies | C1 |
| Acceptance criteria | All `body_html` passes through ammonia with email-specific whitelist before DB insertion; existing ammonia dependency used; tests cover known XSS vectors |
| **Status** | **COMPLETED — 2026-03-25** |

**Scope:**
- Implement `sanitize_email_html()` using ammonia `Builder` with the configuration from ADR-019.
- Call sanitization in the email sync pipeline after extraction, before DB write.
- Unit tests: verify `<script>`, `onerror`, `data:text/html`, `javascript:` URIs are stripped; verify legitimate email tags (table, img, style) are preserved.

**Completion notes:**
- Created `backend/src/content/email_sanitizer.rs` with `sanitize_email_html()` using ammonia 4 Builder.
- Email-specific whitelist: 28 allowed tags (table, td, th, img, a, div, span, font, etc.), per-tag attribute allowlists, `data:` URI scheme for CID images, `rel="noopener noreferrer"` on all links.
- Uses `clean_content_tags` for `style` (CSS sanitization) and `script` (complete removal including content).
- Wired into both Gmail (`parse_message` → `extract_body_html` → `sanitize_email_html`) and Outlook (`parse_message` → `sanitize_email_html`).
- Registered module in `content/mod.rs`; added `pub mod content` to `main.rs`.
- 10 comprehensive tests: XSS vectors (script, event handlers, javascript: URI), formatting preservation (tables, images, links, inline styles), link rel enforcement, data URI allowance, iframe/form stripping, empty input.
- Zero new dependencies — uses existing `ammonia = "4"` from Cargo.toml.

---

### C3. Frontend: Replace SanitizedHtml with sandboxed iframe viewer — COMPLETED

**Why critical:** The current regex-based sanitizer is a known XSS vulnerability. Replacing it eliminates the attack vector and enables proper HTML email rendering.

| Attribute | Value |
|---|---|
| ADR | ADR-019 §1, §2 |
| DDD | DDD-009 — EmailContentAggregate |
| Effort | Medium (3–4 days) |
| Files affected | `frontend/apps/web/src/features/email/MessageBubble.tsx`, new `EmailHtmlViewer.tsx` component |
| Dependencies | C1, C2 |
| Acceptance criteria | HTML emails render with full fidelity in sandboxed iframe; plain-text emails render in `<pre>`; XSS payloads blocked; CSS does not leak between iframe and host |
| **Status** | **COMPLETED — 2026-03-25** |

**Scope:**
- Create `EmailHtmlViewer` component with sandboxed iframe + srcdoc + CSP meta tag.
- Auto-resize iframe to content height via load-event handler.
- Content type switching: HTML primary with plain-text toggle when both available.
- Replace existing `SanitizedHtml` component usage in `MessageBubble.tsx`.
- Remove the regex-based sanitizer code entirely.

**Completion notes:**
- Created `frontend/apps/web/src/features/email/EmailHtmlViewer.tsx` — sandboxed iframe with triple-layer security: `sandbox="allow-popups allow-popups-to-escape-sandbox allow-same-origin"`, CSP meta tag (`script-src 'none'; object-src 'none'`), `referrerPolicy="no-referrer"`.
- Auto-resizes to content height via `contentDocument.scrollHeight` on load event.
- Includes base styles for body, images (max-width: 100%), links, tables, blockquotes, and pre elements.
- `<base target="_blank">` ensures all links open in new tabs.
- Removed the regex-based `SanitizedHtml` component entirely from `MessageBubble.tsx` — no longer in any source file.
- Plain text emails render in `<pre>` with `whitespace-pre-wrap` and `font-sans` for readability.
- Zero new npm dependencies — uses only native browser iframe APIs.
- Frontend build succeeds cleanly.

---

### C4. Frontend: Fix body truncation — COMPLETED

**Why critical:** Users cannot read their full emails. This is the most visually obvious defect.

| Attribute | Value |
|---|---|
| ADR | ADR-019 §5 |
| Effort | Small (0.5–1 day) |
| Files affected | `frontend/apps/web/src/features/email/MessageBubble.tsx` |
| Dependencies | C3 |
| Acceptance criteria | Collapsed emails show a meaningful preview (200 chars); expanded emails show full content; no `slice(0, 100)` truncation in expanded state |
| **Status** | **COMPLETED — 2026-03-25** |

**Scope:**
- Remove `bodyText?.slice(0, 100)` from expanded message rendering.
- Collapsed state: Use DOMPurify-sanitized snippet (first 200 chars of `bodyText`, or text extracted from `bodyHtml`).
- Add `isomorphic-dompurify` as a frontend dependency for snippet sanitization.

**Completion notes:**
- Changed collapsed preview from `email.bodyText?.slice(0, 100)` to `(email.bodyText || email.subject)?.slice(0, 200)` — doubled preview length and added subject fallback when bodyText is unavailable.
- Expanded view now shows full untruncated content via `EmailHtmlViewer` (HTML) or `<pre>` (plain text).
- Did NOT add `isomorphic-dompurify` — the collapsed preview uses plain text slicing which is safe (no HTML interpretation). DOMPurify can be added later if HTML-derived snippets are needed (deferred to L5 toggle work).
- Frontend build succeeds cleanly.

---

## High Priority

> Enables core attachment functionality. These items deliver the ability to see and download email attachments — a table-stakes feature for any email client.

### H1. Backend: Attachment metadata extraction and storage schema

**Why high:** Foundation for all attachment features. Without metadata, the frontend cannot display attachment lists.

| Attribute | Value |
|---|---|
| ADR | ADR-020 §1, §2 |
| DDD | DDD-009 — AttachmentAggregate |
| Effort | Medium (2–3 days) |
| Files affected | `backend/src/db/` (migration), `backend/src/email/gmail.rs`, `backend/src/email/outlook.rs`, `backend/src/email/types.rs` |
| Dependencies | None (can parallel with C1–C4) |
| Acceptance criteria | `attachments` table created; metadata populated during sync for Gmail and Outlook; filename sanitization enforced |

**Scope:**
- SQLite migration: Create `attachments` table per ADR-020 §1 schema.
- Gmail: Extract attachment metadata from `payload.parts[]` where `body.attachmentId` is present.
- Outlook: Extract from `message.attachments` array in Graph API response.
- Implement `SanitizedFilename` value object (strip path separators, null bytes, limit length).
- Store `provider_attachment_id` for lazy fetching. Set `fetch_status = Pending`.

---

### H2. Backend: Single attachment download endpoint with lazy fetch

**Why high:** Enables users to download individual attachments — the most common attachment interaction.

| Attribute | Value |
|---|---|
| ADR | ADR-020 §3, §5 |
| DDD | DDD-009 — AttachmentFetchService, AttachmentServingService |
| Effort | Medium (3–4 days) |
| Files affected | `backend/src/api/emails.rs` (new route), `backend/src/email/attachments.rs` (new module) |
| Dependencies | H1 |
| Acceptance criteria | `GET /api/v1/emails/{id}/attachments/{att_id}` returns streamed binary with correct Content-Type and Content-Disposition headers; first request triggers lazy fetch from provider API; subsequent requests serve from filesystem cache |

**Scope:**
- Implement `fetch_gmail_attachment()` and `fetch_outlook_attachment()` using existing `reqwest` client.
- Implement `cache_to_filesystem()` with directory creation and path validation.
- Implement streaming response via `tokio_util::io::ReaderStream`.
- Update `fetch_status` to `Fetched` after successful cache.
- Error handling: Return 404 if attachment not found; 502 if provider fetch fails; set `fetch_status = Failed` with retry eligibility.

---

### H3. Backend: Attachment list endpoint

**Why high:** The frontend needs to know what attachments exist before it can render download UI.

| Attribute | Value |
|---|---|
| ADR | ADR-020 §3 |
| Effort | Small (0.5–1 day) |
| Files affected | `backend/src/api/emails.rs` |
| Dependencies | H1 |
| Acceptance criteria | `GET /api/v1/emails/{id}/attachments` returns JSON array of attachment metadata; excludes inline attachments by default; optional `include_inline=true` query parameter |

---

### H4. Frontend: Attachment chips UI

**Why high:** Displays attachment information and enables download — the primary user-facing attachment feature.

| Attribute | Value |
|---|---|
| ADR | ADR-020 §7 |
| DDD | DDD-009 — AttachmentAggregate |
| Effort | Small (2–3 days) |
| Files affected | `frontend/apps/web/src/features/email/MessageBubble.tsx`, new `AttachmentList.tsx`, `AttachmentChip.tsx` components |
| Dependencies | H2, H3 |
| Acceptance criteria | Emails with attachments show chip/pill components below the body; each chip shows file icon (lucide-react), filename, size; clicking downloads the file; "Attachment downloads are not yet available" placeholder removed |

**Scope:**
- Extend `Email` TypeScript interface: add `attachments: Attachment[]` array.
- Create `AttachmentChip` component with icon mapping by MIME type (lucide-react).
- Create `AttachmentList` container that fetches from the attachments endpoint.
- Wire into `MessageBubble.tsx` — render below email body when `attachments.length > 0`.
- `formatSize()` utility for human-readable byte sizes.
- No new npm dependencies — uses existing `lucide-react` and Tailwind.

---

### H5. Frontend: Update Email type and API layer

**Why high:** The TypeScript types and API client must be updated to carry `bodyHtml` and `attachments` data.

| Attribute | Value |
|---|---|
| ADR | ADR-019, ADR-020 §7 |
| Effort | Small (1 day) |
| Files affected | `frontend/packages/types/src/email.ts`, `frontend/apps/web/src/api/` or equivalent fetch layer |
| Dependencies | C1, H1 |
| Acceptance criteria | `Email` interface includes `bodyHtml?: string` and `attachments: Attachment[]`; API responses correctly deserialized; backward compatible with emails that lack these fields |

---

## Moderate Priority

> Enhances the core experience with inline images, bulk download, and image previews. Valuable but not blocking core usability.

### M1. Backend: CID inline image resolution

**Why moderate:** Inline images (logos, signatures) are common in HTML emails. Without CID resolution, they show as broken images. Important for rendering fidelity but not a security blocker.

| Attribute | Value |
|---|---|
| ADR | ADR-019 §3, ADR-020 §6 |
| DDD | DDD-009 — HtmlSanitizationService (resolve_cid_references) |
| Effort | Small (1–2 days) |
| Files affected | `backend/src/content/` (sanitization service) |
| Dependencies | C2, H1 |
| Acceptance criteria | HTML emails with `<img src="cid:...">` render inline images correctly; CID references replaced with base64 data URIs before ammonia sanitization |

**Scope:**
- Build `HashMap<ContentId, (content_type, bytes)>` from inline attachments.
- Replace `cid:{id}` patterns in HTML body with `data:{type};base64,{encoded}`.
- Run CID resolution **before** ammonia sanitization.
- Test with real-world HTML emails containing signature images.

---

### M2. Backend: ZIP streaming endpoint for bulk attachment download

**Why moderate:** Convenience feature for emails with many attachments (e.g., 10+ photos, document bundles). Lower frequency interaction than single download.

| Attribute | Value |
|---|---|
| ADR | ADR-020 §3, §4 |
| DDD | DDD-009 — AttachmentServingService (stream_zip) |
| Effort | Small (2 days) |
| Files affected | `backend/src/api/emails.rs`, `backend/Cargo.toml` |
| Dependencies | H2 |
| Acceptance criteria | `GET /api/v1/emails/{id}/attachments/zip` streams a ZIP containing all non-inline attachments; correct `Content-Type: application/zip` and `Content-Disposition` headers; handles lazy-fetch for uncached attachments before zipping |

**New dependency:**
```toml
async_zip = { version = "0.0.17", features = ["tokio", "deflate"] }
```

---

### M3. Frontend: Bulk download button

**Why moderate:** Companion to M2. Simple UI addition once the backend endpoint exists.

| Attribute | Value |
|---|---|
| ADR | ADR-020 |
| Effort | Small (0.5 day) |
| Files affected | `AttachmentList.tsx` |
| Dependencies | M2, H4 |
| Acceptance criteria | "Download all (N)" link/button appears when email has 2+ non-inline attachments; triggers ZIP download from backend |

---

### M4. Frontend: Image attachment preview thumbnails

**Why moderate:** Better UX for image attachments — users can see thumbnails instead of just a filename chip. Common in modern email clients.

| Attribute | Value |
|---|---|
| ADR | — (research §5.3) |
| Effort | Small (1 day) |
| Files affected | `AttachmentChip.tsx` or new `ImageAttachmentPreview.tsx` |
| Dependencies | H2, H4 |
| Acceptance criteria | Image-type attachments render as thumbnails (24px height) with filename overlay on hover; clicking opens full image in new tab; `loading="lazy"` on thumbnail img |

---

### M5. Backend: Attachment cleanup on email delete

**Why moderate:** Without cleanup, deleted emails leave orphaned files on disk. Important for storage hygiene but not user-facing.

| Attribute | Value |
|---|---|
| ADR | ADR-020 (consequences: filesystem management) |
| DDD | DDD-009 — AttachmentAggregate invariant #4 |
| Effort | Small (1 day) |
| Files affected | `backend/src/api/emails.rs` (delete handler), `backend/src/email/attachments.rs` |
| Dependencies | H1, H2 |
| Acceptance criteria | When an email is deleted, its attachment files are removed from the filesystem; `ON DELETE CASCADE` handles DB rows; background task or synchronous delete for files |

---

## Low Priority

> Nice-to-have improvements and future-proofing. Can be deferred without impacting core email reading and attachment experience.

### L1. Backend: IMAP MIME parsing with mail-parser

**Why low:** Only relevant when IMAP provider support is added. Gmail and Outlook use structured JSON APIs, not raw MIME.

| Attribute | Value |
|---|---|
| ADR | ADR-019 (research §3.2) |
| DDD | DDD-009 — ContentExtractionService (extract_imap_body) |
| Effort | Medium (3–4 days) |
| Dependencies | C1, C2, H1 |
| Acceptance criteria | Raw IMAP FETCH responses parsed via `mail-parser`; HTML body, text body, and attachment metadata extracted; CID inline images handled |

**New dependency:**
```toml
mail-parser = "0.11"
```

---

### L2. Frontend: Remote image proxy for tracking pixel prevention

**Why low:** Security/privacy enhancement. The backend already detects tracking pixels in `content/types.rs`. A full proxy adds complexity and latency.

| Attribute | Value |
|---|---|
| ADR | ADR-019 (research §2.7) |
| Effort | Medium (3–4 days) |
| Dependencies | C3 |
| Acceptance criteria | External images in HTML emails routed through a backend proxy endpoint; original IP and headers not exposed to remote servers; optional per-account toggle |

---

### L3. Backend: Attachment storage eviction policy

**Why low:** Storage growth is gradual and manageable for personal use. Enterprise-scale would need this sooner.

| Attribute | Value |
|---|---|
| ADR | ADR-020 (consequences: storage growth) |
| Effort | Small (1–2 days) |
| Dependencies | H2 |
| Acceptance criteria | LRU eviction removes cached attachment files older than a configurable threshold; re-fetch from provider on next download; storage budget configurable |

---

### L4. Frontend: Compose attachment upload with drag-and-drop

**Why low:** Compose functionality is a separate feature track. This item is forward-looking.

| Attribute | Value |
|---|---|
| ADR | — (research §5.4) |
| Effort | Medium (3–4 days) |
| Dependencies | Compose feature (not yet planned) |
| Acceptance criteria | Drag-and-drop file upload in compose view; `react-dropzone` integration; attachment previews before send |

**New dependency:**
```json
{ "react-dropzone": "^14.0.0" }
```

---

### L5. Frontend: Plain text / HTML toggle button

**Why low:** For multipart/alternative emails, defaulting to HTML is correct 95%+ of the time. The toggle is a power-user feature.

| Attribute | Value |
|---|---|
| ADR | ADR-019 §4 |
| Effort | Small (0.5 day) |
| Dependencies | C3 |
| Acceptance criteria | When both `bodyHtml` and `bodyText` are present, a toggle button switches between HTML iframe view and plain text `<pre>` view |

---

## Dependency Graph

```
C1 (extract body_html) ✅
 └─► C2 (ammonia sanitization) ✅
      └─► C3 (sandboxed iframe) ✅
           └─► C4 (fix truncation) ✅

H1 (attachment schema + metadata) ──────────────┐
 ├─► H2 (single download + lazy fetch)          │
 │    ├─► H4 (attachment chips UI)              │
 │    ├─► M2 (ZIP streaming)                    │
 │    │    └─► M3 (bulk download button)        │
 │    ├─► M4 (image previews)                   │
 │    └─► M5 (cleanup on delete)                │
 └─► H3 (list endpoint)                         │
      └─► H4 (attachment chips UI)              │
                                                 │
H5 (update types + API layer) ◄──────── C1 + H1 ┘

M1 (CID resolution) ◄── C2 + H1

L1 (IMAP mail-parser) ◄── C1 + C2 + H1
L2 (image proxy) ◄── C3
L3 (eviction) ◄── H2
L4 (compose upload) — independent
L5 (toggle button) ◄── C3
```

---

## Effort Summary

| Priority | Items | Estimated Total Effort | Status |
|---|---|---|---|
| **Critical** | C1–C4 | 7–10 days | **4/4 COMPLETED** |
| **High** | H1–H5 | 9–12 days | 0/5 Pending |
| **Moderate** | M1–M5 | 5.5–7 days | 0/5 Pending |
| **Low** | L1–L5 | 10.5–14.5 days | 0/5 Pending |
| **Total** | 19 items | 32–43.5 days | **4/19 completed** |

---

## New Dependencies Summary

| Dependency | Type | Priority Level | Purpose |
|---|---|---|---|
| ~~`isomorphic-dompurify` ^3.0.0~~ | npm | ~~Critical (C4)~~ | Deferred — plain text slicing is safe for collapsed previews; revisit with L5 |
| `async_zip` 0.0.17 | Cargo | Moderate (M2) | Streaming ZIP archive creation |
| `mail-parser` 0.11 | Cargo | Low (L1) | IMAP MIME parsing |
| `react-dropzone` ^14.0.0 | npm | Low (L4) | Compose file upload (future) |

---

## Cross-References

| Document | Relevance |
|---|---|
| [ADR-019](../ADRs/ADR-019-email-body-rendering.md) | Email body rendering strategy and security model |
| [ADR-020](../ADRs/ADR-020-email-attachment-management.md) | Attachment storage, API, and download architecture |
| [DDD-009](../DDDs/DDD-009-email-content-rendering.md) | Domain model for email content and attachments |
| [DDD-008](../DDDs/DDD-008-email-operations.md) | Email Operations domain (upstream dependency) |
| [DDD-005](../DDDs/DDD-005-account-management.md) | Account Management (OAuth tokens for lazy fetch) |
| [DDD-003](../DDDs/DDD-003-ingestion.md) | Ingestion context (supplies raw provider responses) |
| [Research](../research/email-interaction-enhancements.md) | Library evaluations, code examples, security analysis |
