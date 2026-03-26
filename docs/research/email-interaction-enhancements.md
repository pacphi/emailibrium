# Email Body Rendering & Attachment Handling Research

> Research date: 2026-03-25
> Scope: Best-in-class solutions for a Next.js + TypeScript frontend with a Rust (Axum) backend

---

## Table of Contents

1. [Current State Analysis](#1-current-state-analysis)
2. [Area 1: Email Body Rendering (Frontend)](#2-area-1-email-body-rendering-frontend)
3. [Area 2: Email Body Rendering (Backend)](#3-area-2-email-body-rendering-backend)
4. [Area 3: Email Attachments (Backend)](#4-area-3-email-attachments-backend)
5. [Area 4: Email Attachments (Frontend)](#5-area-4-email-attachments-frontend)
6. [Area 5: Full-Featured Email Rendering Libraries](#6-area-5-full-featured-email-rendering-libraries)
7. [Recommended Stack](#7-recommended-stack)

---

## 1. Current State Analysis

### Frontend (`MessageBubble.tsx`)

The current implementation has three critical gaps:

1. **Naive HTML sanitization** -- a regex-based `SanitizedHtml` component strips `<script>` tags and `on*` event handlers, then injects via `dangerouslySetInnerHTML`. This is trivially bypassed by a motivated attacker (e.g., `<img src=x onerror=alert(1)>` patterns, data URIs, CSS expressions, SVG payloads).

2. **No attachment rendering** -- `hasAttachments` is a boolean flag with a placeholder message: "Attachment downloads are not yet available."

3. **Body truncation** -- collapsed messages show only `email.bodyText?.slice(0, 100)`, and there is no toggle between HTML and plain-text views.

### Backend (Rust / Axum)

- `RawAttachment` struct already models filename, content_type, data bytes, is_inline, and content_id.
- `ammonia` crate (v4) is already in `Cargo.toml` for HTML sanitization.
- The Gmail/Outlook fetchers extract `body_text` but not `body_html` from API responses.
- No API endpoint for serving attachment content to the frontend.

### Frontend Types (`Email` interface)

```typescript
bodyText?: string;
bodyHtml?: string;
hasAttachments: boolean;
```

The type system already carries `bodyHtml` but the API layer and rendering layer do not fully exploit it.

---

## 2. Area 1: Email Body Rendering (Frontend)

### 2.1 Rendering Approaches Compared

| Approach | Security | CSS Isolation | Performance | Complexity |
|---|---|---|---|---|
| **Sandboxed iframe + srcdoc** | Excellent | Full | Good | Medium |
| **DOMPurify + dangerouslySetInnerHTML** | Very Good | None | Excellent | Low |
| **html-react-parser + DOMPurify** | Very Good | None | Good | Medium |
| **Shadow DOM** | Good | Partial | Good | Medium |

### 2.2 Recommended: Sandboxed Iframe (Close.com Approach)

Close.com's engineering team published a detailed analysis of safely rendering untrusted HTML email. Their approach uses **triple-layer protection**:

1. **`sandbox` attribute** -- blocks JavaScript, form submission, and navigation by default. Whitelists only `allow-popups allow-popups-to-escape-sandbox allow-same-origin`.
2. **`srcdoc` attribute** -- injects HTML directly without needing a separate origin.
3. **Content Security Policy** -- `<meta http-equiv="Content-Security-Policy" content="script-src 'none'">` injected into the document head as redundant protection.

This is the industry standard for webmail clients. Roundcube, Mailspring (Electron + Chromium), and ProtonMail all use browser-engine rendering with heavy sandboxing rather than HTML sanitization alone.

**Key insight from Close.com:** "We have learned of particular browser bugs where there were temporary limitations in any one of these individual approaches. A reminder that security in layers is often best."

#### Example Implementation

```tsx
interface EmailHtmlViewerProps {
  html: string;
  onLoad?: () => void;
}

export function EmailHtmlViewer({ html, onLoad }: EmailHtmlViewerProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const wrappedHtml = useMemo(() => {
    // Inject CSP meta tag and base target for link safety
    const csp = `<meta http-equiv="Content-Security-Policy" content="script-src 'none'; object-src 'none';">`;
    const base = `<base target="_blank">`;
    const style = `<style>body { margin: 0; font-family: system-ui, sans-serif; }</style>`;
    return `<!DOCTYPE html><html><head>${csp}${base}${style}</head><body>${html}</body></html>`;
  }, [html]);

  // Auto-resize iframe to content height
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;
    const resize = () => {
      try {
        const height = iframe.contentDocument?.documentElement?.scrollHeight;
        if (height) iframe.style.height = `${height}px`;
      } catch { /* cross-origin safety */ }
      onLoad?.();
    };
    iframe.addEventListener('load', resize);
    return () => iframe.removeEventListener('load', resize);
  }, [onLoad]);

  return (
    <iframe
      ref={iframeRef}
      srcDoc={wrappedHtml}
      sandbox="allow-popups allow-popups-to-escape-sandbox allow-same-origin"
      referrerPolicy="no-referrer"
      title="Email content"
      className="w-full border-0"
      style={{ minHeight: 200 }}
    />
  );
}
```

### 2.3 Alternative: DOMPurify + dangerouslySetInnerHTML

For simpler emails or performance-critical scenarios (e.g., rendering many emails simultaneously in a list), DOMPurify-based sanitization is viable.

#### Library: isomorphic-dompurify

| Attribute | Value |
|---|---|
| **npm** | [isomorphic-dompurify](https://www.npmjs.com/package/isomorphic-dompurify) |
| **Weekly downloads** | ~1,937,000 |
| **GitHub stars** | ~500 (wraps DOMPurify at ~14,000 stars) |
| **License** | Apache-2.0 / MPL-2.0 |
| **Last updated** | Active, 2025 |
| **Key feature** | Works identically on server (jsdom) and client (native DOM) |

**Caveat for Next.js:** CommonJS + jsdom@28 can cause ESM-only import failures. Pin jsdom to 25.0.1 via overrides if SSR issues arise.

```tsx
import DOMPurify from 'isomorphic-dompurify';

function SanitizedEmailHtml({ html }: { html: string }) {
  const clean = DOMPurify.sanitize(html, {
    ALLOW_TAGS: ['a', 'b', 'br', 'div', 'em', 'h1', 'h2', 'h3', 'h4',
      'hr', 'i', 'img', 'li', 'ol', 'p', 'span', 'strong', 'table',
      'tbody', 'td', 'th', 'thead', 'tr', 'u', 'ul', 'blockquote', 'pre', 'code',
      'center', 'font', 'style'],
    ALLOW_ATTR: ['href', 'src', 'alt', 'title', 'style', 'class', 'width',
      'height', 'align', 'valign', 'bgcolor', 'color', 'border',
      'cellpadding', 'cellspacing', 'colspan', 'rowspan', 'target', 'rel'],
    FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input'],
    FORBID_ATTR: ['onerror', 'onload', 'onclick', 'onmouseover'],
    ADD_ATTR: ['target'],
  });

  return (
    <div
      className="prose prose-sm max-w-none dark:prose-invert"
      dangerouslySetInnerHTML={{ __html: clean }}
    />
  );
}
```

**Limitation:** No CSS isolation. Email styles leak into and from the host page. This is acceptable if Tailwind's `prose` class provides sufficient containment.

### 2.4 Library: html-react-parser

| Attribute | Value |
|---|---|
| **npm** | [html-react-parser](https://www.npmjs.com/package/html-react-parser) |
| **Weekly downloads** | ~2,673,000 |
| **GitHub stars** | ~2,384 |
| **License** | MIT |
| **Last updated** | Active, v5.x (2025) |
| **Key feature** | Converts HTML to React elements; allows element replacement via `replace` callback |

**Pros:**
- Most popular HTML-to-React parser
- Allows transforming elements during parsing (e.g., rewriting image src attributes, adding `target="_blank"` to links)
- TypeScript support

**Cons:**
- Does NOT sanitize by default. Must pair with DOMPurify.
- Slightly more overhead than raw `dangerouslySetInnerHTML`.

### 2.5 Library: react-safe-src-doc-iframe (GoDaddy)

| Attribute | Value |
|---|---|
| **npm** | [react-safe-src-doc-iframe](https://www.npmjs.com/package/react-safe-src-doc-iframe) |
| **GitHub stars** | 23 |
| **License** | MIT |
| **Last updated** | Archived August 2022 |
| **Key feature** | Disables pointer events on links/buttons/images, restricts sandbox to `allow-same-origin` |

**Verdict:** Archived and low adoption. Better to implement a custom sandboxed iframe component (see section 2.2) which gives full control over sandbox flags, CSP, and auto-resizing.

### 2.6 Plain Text vs HTML vs Multipart

The frontend should handle three content types:

| Content Type | Rendering Strategy |
|---|---|
| `bodyHtml` present | Render in sandboxed iframe (primary) |
| `bodyText` only | Render in `<pre>` with `whitespace-pre-wrap` |
| Both present (multipart/alternative) | Default to HTML with toggle button for plain text |

### 2.7 Performance Considerations

- **Lazy loading:** Only render iframe when `MessageBubble` is expanded (`isExpanded === true`). Current code already does this.
- **Virtualization:** The app already uses `@tanstack/react-virtual` for the email list. Email body rendering happens only for the active/expanded message.
- **Image lazy loading:** Add `loading="lazy"` to images via CSP or html-react-parser's `replace` callback.
- **Remote image proxy:** Consider routing external images through the backend to prevent tracking pixels and IP leakage. The backend already detects tracking pixels in `content/types.rs`.

---

## 3. Area 2: Email Body Rendering (Backend)

### 3.1 HTML Sanitization with Ammonia (Already Available)

The backend already depends on `ammonia = "4"`. Ammonia is the Rust equivalent of DOMPurify.

| Attribute | Value |
|---|---|
| **Crate** | [ammonia](https://crates.io/crates/ammonia) |
| **GitHub stars** | ~700 |
| **License** | Apache-2.0 / MIT |
| **Last updated** | Active |
| **Key feature** | Whitelist-based, uses html5ever parser, fuzz-tested |

#### Recommended Configuration for Email

```rust
use ammonia::Builder;

pub fn sanitize_email_html(raw_html: &str) -> String {
    Builder::new()
        .tags(hashset![
            "a", "b", "blockquote", "br", "center", "code", "div", "em",
            "font", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img",
            "li", "ol", "p", "pre", "span", "strong", "style", "table",
            "tbody", "td", "th", "thead", "tr", "u", "ul",
        ])
        .tag_attributes(hashmap![
            "a" => hashset!["href", "target", "rel"],
            "img" => hashset!["src", "alt", "width", "height", "style"],
            "td" => hashset!["style", "width", "height", "align", "valign",
                             "bgcolor", "colspan", "rowspan"],
            "th" => hashset!["style", "width", "height", "align", "valign",
                             "bgcolor", "colspan", "rowspan"],
            "table" => hashset!["style", "width", "border", "cellpadding",
                                "cellspacing", "bgcolor", "align"],
            "div" => hashset!["style", "class", "align"],
            "span" => hashset!["style", "class"],
            "font" => hashset!["color", "size", "face"],
            "p" => hashset!["style", "align"],
        ])
        .link_rel(Some("noopener noreferrer"))
        .url_schemes(hashset!["http", "https", "mailto", "cid"])
        .clean(raw_html)
        .to_string()
}
```

### 3.2 MIME Parsing with mail-parser

| Attribute | Value |
|---|---|
| **Crate** | [mail-parser](https://crates.io/crates/mail-parser) |
| **GitHub stars** | 429 |
| **License** | Apache-2.0 / MIT |
| **Version** | 0.11.1 (Aug 2025) |
| **Key features** | Zero-copy, 100% safe Rust, no dependencies, 41 character sets, RFC 8621 compliant |

**Why mail-parser over mailparse:** mail-parser provides a human-friendly representation with `body_html()`, `body_text()`, and `attachment()` accessors rather than nested MIME tree traversal. It also handles automatic HTML-to-text conversion when the alternative is missing.

#### Usage for Email Body Extraction

```rust
use mail_parser::MessageParser;

let message = MessageParser::default().parse(raw_bytes).unwrap();

// Get HTML body (index 0 = first HTML part)
let html_body = message.body_html(0);

// Get plain text body
let text_body = message.body_text(0);

// Iterate attachments
for (i, attachment) in message.attachments().enumerate() {
    let filename = attachment.attachment_name().unwrap_or("unnamed");
    let content_type = attachment.content_type().map(|ct| ct.ctype());
    let is_inline = attachment.is_inline();
    let content_id = attachment.content_id();
    let data = attachment.contents();
}
```

**Note:** This is relevant for IMAP-based providers where raw MIME data is fetched. For Gmail and Outlook APIs, body and attachments are already structured in API responses, but mail-parser is still useful for processing `.eml` files or IMAP FETCH responses.

### 3.3 Inline Image (CID) Resolution

HTML emails reference inline images via `cid:` URIs (e.g., `<img src="cid:image001@example.com">`). The backend must:

1. Parse all MIME parts and collect inline attachments with their `Content-ID` headers.
2. Either:
   - **Option A (recommended):** Replace `cid:` references in HTML with base64 data URIs before serving to the frontend.
   - **Option B:** Serve inline images from a backend endpoint and rewrite `cid:` to API URLs.

Option A avoids extra round-trips. The ammonia configuration above whitelists `cid` as a URL scheme, but the rewriting should happen before sanitization.

```rust
fn resolve_cid_references(html: &str, inline_images: &HashMap<String, &[u8]>) -> String {
    let mut result = html.to_string();
    for (content_id, data) in inline_images {
        let cid_ref = format!("cid:{}", content_id.trim_matches('<').trim_matches('>'));
        let data_uri = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        );
        result = result.replace(&cid_ref, &data_uri);
    }
    result
}
```

### 3.4 Character Encoding

mail-parser handles 41 character sets natively. For Gmail/Outlook API responses (always UTF-8 JSON), encoding is not an issue. For IMAP sources, mail-parser transparently decodes quoted-printable and base64 content transfer encodings.

---

## 4. Area 3: Email Attachments (Backend)

### 4.1 Provider API Attachment Endpoints

#### Gmail API

| Attribute | Value |
|---|---|
| **Endpoint** | `GET /gmail/v1/users/{userId}/messages/{messageId}/attachments/{attachmentId}` |
| **Auth** | OAuth 2.0 with `gmail.readonly` scope |
| **Response** | `{ attachmentId, size, data }` where `data` is base64url-encoded |
| **Max size** | 25 MB per message (Gmail limit) |

The `attachmentId` is available in the message payload's `body.attachmentId` field for parts that are stored separately (large attachments).

#### Microsoft Graph API

| Attribute | Value |
|---|---|
| **List endpoint** | `GET /me/messages/{messageId}/attachments` |
| **Get endpoint** | `GET /me/messages/{messageId}/attachments/{attachmentId}` |
| **Raw content** | Append `/$value` to get raw bytes |
| **Auth** | OAuth 2.0 with `Mail.Read` permission |
| **Response** | `{ id, name, contentType, size, isInline, contentId, contentBytes }` |
| **Attachment types** | `fileAttachment`, `itemAttachment`, `referenceAttachment` |

#### Rust SDK Options

| Crate | Purpose | Notes |
|---|---|---|
| `google-gmail1` | Gmail API client | Official Google-generated SDK, uses hyper/reqwest |
| `graph-rs-sdk` | Microsoft Graph client | Community SDK, returns `reqwest::Response` |
| Direct `reqwest` | Both APIs | Simpler; the project already uses reqwest |

**Recommendation:** Use direct `reqwest` calls (already in the project) with structured types rather than adding heavyweight SDK crates. Both APIs return JSON with base64-encoded content.

### 4.2 Storage Strategy

Given the project uses SQLite + local storage, attachment storage options:

| Strategy | Pros | Cons |
|---|---|---|
| **Filesystem (recommended)** | Simple, fast, works with streaming | Needs cleanup on delete, path management |
| **SQLite BLOB** | Atomic with email record | Bloats DB, slow for large files, no streaming |
| **S3/MinIO** | Scalable, CDN-friendly | Added infrastructure complexity |

**Recommended:** Store attachment metadata in SQLite, store file content on the local filesystem under `data/attachments/{account_id}/{message_id}/{filename}`.

#### Schema Addition

```sql
CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    email_id TEXT NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    is_inline BOOLEAN NOT NULL DEFAULT FALSE,
    content_id TEXT,           -- for CID references
    storage_path TEXT NOT NULL, -- relative path on filesystem
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_attachments_email_id ON attachments(email_id);
```

### 4.3 New API Endpoints

```
GET /api/v1/emails/{id}/attachments          -- list attachments for an email
GET /api/v1/emails/{id}/attachments/{att_id}  -- download single attachment
GET /api/v1/emails/{id}/attachments/zip       -- download all as ZIP
```

### 4.4 Streaming Large Attachments

Use Axum's streaming body response to avoid loading entire files into memory:

```rust
use axum::body::Body;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

async fn download_attachment(/* params */) -> impl IntoResponse {
    let file = File::open(&storage_path).await?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .header("Content-Type", &attachment.content_type)
        .header("Content-Disposition", format!("attachment; filename=\"{}\"", attachment.filename))
        .header("Content-Length", attachment.size_bytes.to_string())
        .body(body)
        .unwrap()
}
```

### 4.5 ZIP Archive for Bulk Download

| Crate | Purpose | Notes |
|---|---|---|
| **async_zip** | Async ZIP creation with tokio | Best fit for Axum streaming |
| **zipit** | Streaming ZIP generation | Simpler API, fewer features |
| **zip** (standard) | Sync ZIP creation | Requires blocking task spawn |

**Recommended: `async_zip`** -- native tokio support, can stream ZIP creation directly into an Axum response body.

```rust
use async_zip::tokio::write::ZipFileWriter;
use tokio::io::duplex;
use tokio_util::io::ReaderStream;

async fn download_all_attachments_zip(/* params */) -> impl IntoResponse {
    let (writer, reader) = duplex(65536);
    let attachments = load_attachments(email_id).await?;

    tokio::spawn(async move {
        let mut zip = ZipFileWriter::with_tokio(writer);
        for att in attachments {
            let entry = ZipEntryBuilder::new(att.filename.into(), Compression::Deflate);
            let data = tokio::fs::read(&att.storage_path).await.unwrap();
            zip.write_entry_whole(entry, &data).await.unwrap();
        }
        zip.close().await.unwrap();
    });

    let body = Body::from_stream(ReaderStream::new(reader));
    Response::builder()
        .header("Content-Type", "application/zip")
        .header("Content-Disposition", "attachment; filename=\"attachments.zip\"")
        .body(body)
        .unwrap()
}
```

---

## 5. Area 4: Email Attachments (Frontend)

### 5.1 UI Pattern: Attachment Chips

The standard email client pattern uses horizontal chip/pill elements below the email body. Each chip shows:

- File type icon (from lucide-react, already in the project)
- Filename (truncated)
- File size
- Click to download

#### Implementation with Existing Dependencies

```tsx
import { Paperclip, FileText, Image, File, FileSpreadsheet, Download } from 'lucide-react';

const FILE_ICONS: Record<string, React.ComponentType> = {
  'application/pdf': FileText,
  'image/': Image,
  'text/': FileText,
  'application/vnd.ms-excel': FileSpreadsheet,
  'application/vnd.openxmlformats': FileSpreadsheet,
};

function getIcon(contentType: string) {
  for (const [prefix, Icon] of Object.entries(FILE_ICONS)) {
    if (contentType.startsWith(prefix)) return Icon;
  }
  return File;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface AttachmentChipProps {
  attachment: { id: string; filename: string; contentType: string; sizeBytes: number };
  emailId: string;
}

function AttachmentChip({ attachment, emailId }: AttachmentChipProps) {
  const Icon = getIcon(attachment.contentType);
  const downloadUrl = `/api/v1/emails/${emailId}/attachments/${attachment.id}`;

  return (
    <a
      href={downloadUrl}
      download={attachment.filename}
      className="inline-flex items-center gap-2 rounded-lg border border-gray-200
        px-3 py-2 text-sm hover:bg-gray-50 dark:border-gray-700 dark:hover:bg-gray-700/50
        transition-colors"
    >
      <Icon className="h-4 w-4 text-gray-500" />
      <span className="max-w-[150px] truncate">{attachment.filename}</span>
      <span className="text-xs text-gray-400">{formatSize(attachment.sizeBytes)}</span>
      <Download className="h-3 w-3 text-gray-400" />
    </a>
  );
}
```

### 5.2 Bulk Download (ZIP)

For bulk download, use a simple link to the ZIP endpoint:

```tsx
function DownloadAllButton({ emailId, count }: { emailId: string; count: number }) {
  return (
    <a
      href={`/api/v1/emails/${emailId}/attachments/zip`}
      download="attachments.zip"
      className="inline-flex items-center gap-1 text-sm text-indigo-600 hover:text-indigo-700"
    >
      <Download className="h-4 w-4" />
      Download all ({count})
    </a>
  );
}
```

For client-side ZIP creation (if backend streaming is not available), use `jszip` + `file-saver`:

| Library | Weekly Downloads | Stars | License | Notes |
|---|---|---|---|---|
| **jszip** | ~6M | ~9,800 | MIT / GPLv3 | Browser ZIP creation, well-maintained |
| **file-saver** | ~3M | ~21,000 | MIT | Cross-browser download trigger |

### 5.3 Inline Image Preview

For inline images (`isInline: true`), the CID references should be resolved server-side (see section 3.3). No extra frontend work needed -- the sandboxed iframe renders them as data URIs.

For non-inline image attachments, show a thumbnail preview:

```tsx
function ImageAttachmentPreview({ attachment, emailId }: AttachmentChipProps) {
  const isImage = attachment.contentType.startsWith('image/');
  if (!isImage) return <AttachmentChip attachment={attachment} emailId={emailId} />;

  return (
    <button
      onClick={() => window.open(`/api/v1/emails/${emailId}/attachments/${attachment.id}`)}
      className="group relative overflow-hidden rounded-lg border border-gray-200
        dark:border-gray-700"
    >
      <img
        src={`/api/v1/emails/${emailId}/attachments/${attachment.id}`}
        alt={attachment.filename}
        loading="lazy"
        className="h-24 w-auto object-cover"
      />
      <div className="absolute inset-x-0 bottom-0 bg-black/60 px-2 py-1 text-xs text-white
        opacity-0 group-hover:opacity-100 transition-opacity">
        {attachment.filename}
      </div>
    </button>
  );
}
```

### 5.4 Frontend Libraries (Not Needed)

Given that the project already has `lucide-react` for icons and uses Tailwind CSS, no additional UI libraries are required for attachment rendering. The patterns above use only existing dependencies.

For future compose/upload functionality:

| Library | Weekly Downloads | Stars | License | Use Case |
|---|---|---|---|---|
| **react-dropzone** | ~2.5M | ~10,600 | MIT | Drag-and-drop file upload for compose |

---

## 6. Area 5: Full-Featured Email Rendering Libraries

### 6.1 Open-Source Email Clients (Reference Implementations)

| Project | Tech Stack | HTML Rendering | Stars | Status |
|---|---|---|---|---|
| **Roundcube** | PHP, JS | HTML5-PHP sanitizer + iframe | ~6,000 | Active, GPLv3 |
| **Mailspring** | Electron, React | Chromium renderer (full browser) | ~15,000 | Active, GPLv3 |
| **ProtonMail** | React | Sanitization + iframe sandbox | Private | Active |
| **next-email-client** (leerob) | Next.js, Postgres | Basic; demo project | ~600 | Demo only |
| **Cozy Emails** | Node.js, React | Legacy, not maintained | ~300 | Archived |

### 6.2 Email Template Libraries (Not Directly Applicable)

These libraries are for **creating** emails, not **viewing** them, but are worth noting:

| Library | Stars | Purpose |
|---|---|---|
| **React Email** (Resend) | ~14,000 | Build email templates with React components |
| **MJML** | ~16,000 | Responsive email markup language |
| **Mailing** | ~3,800 | Next.js email sending framework |

### 6.3 Key Takeaway

There is no off-the-shelf "React email viewer component" that handles arbitrary untrusted HTML email rendering. Every production email client builds its own viewer using the sandboxed-iframe pattern described in section 2.2. This is because:

1. Email HTML is a unique dialect with table-based layouts, inline CSS, and vendor-specific quirks.
2. Security requirements demand defense-in-depth (sandbox + CSP + sanitization).
3. CSS isolation is essential -- email styles must not leak into the application.

---

## 7. Recommended Stack

### Frontend

| Layer | Solution | Rationale |
|---|---|---|
| **HTML email rendering** | Custom sandboxed iframe with `srcdoc` + CSP | Industry standard; full CSS isolation; defense-in-depth |
| **Fallback sanitization** | `isomorphic-dompurify` | For previews, snippets, or when iframe is overkill |
| **HTML parsing (optional)** | `html-react-parser` | Only if element-level transformation is needed (e.g., link rewriting) |
| **Attachment chips** | Custom component with `lucide-react` icons | No new dependency; leverages existing icon library |
| **Bulk ZIP download** | Backend-streamed ZIP (preferred) or `jszip` + `file-saver` (fallback) | Server-side is more efficient |
| **File upload (compose, future)** | `react-dropzone` | De facto standard |

#### New npm Dependencies

```json
{
  "isomorphic-dompurify": "^3.0.0"
}
```

That is **one new dependency**. The sandboxed iframe, attachment chips, and image previews use only existing project dependencies (React, lucide-react, Tailwind).

### Backend (Rust)

| Layer | Solution | Rationale |
|---|---|---|
| **HTML sanitization** | `ammonia` (already in Cargo.toml) | Whitelist-based, fuzz-tested, html5ever-backed |
| **MIME parsing (IMAP)** | `mail-parser` | Zero-copy, RFC-compliant, handles CID and attachments |
| **CID resolution** | Custom (replace `cid:` with base64 data URIs) | Avoids extra HTTP round-trips |
| **Attachment storage** | Filesystem under `data/attachments/` | Simple, works with streaming |
| **Attachment streaming** | Axum + `tokio_util::io::ReaderStream` | No full file buffering |
| **ZIP streaming** | `async_zip` | Native tokio; streams directly to HTTP response |
| **Gmail attachments** | Direct `reqwest` to `messages.attachments.get` | Already using reqwest |
| **Outlook attachments** | Direct `reqwest` to Graph API `attachments/{id}/$value` | Already using reqwest |

#### New Cargo Dependencies

```toml
[dependencies]
mail-parser = "0.11"    # MIME parsing for IMAP sources
async_zip = { version = "0.0.17", features = ["tokio", "deflate"] }  # ZIP streaming
```

### Implementation Priority

| Phase | Work | Effort |
|---|---|---|
| **Phase 1** | Backend: extract `body_html` from Gmail/Outlook APIs and store in DB | Small |
| **Phase 2** | Frontend: replace `SanitizedHtml` with sandboxed iframe viewer | Medium |
| **Phase 3** | Backend: attachment download endpoints + filesystem storage | Medium |
| **Phase 4** | Frontend: attachment chips with download | Small |
| **Phase 5** | Backend: CID inline image resolution | Small |
| **Phase 6** | Backend: ZIP streaming endpoint | Small |
| **Phase 7** | Frontend: bulk download button, image previews | Small |
| **Phase 8** | Backend: `mail-parser` integration for IMAP provider | Medium |

### Security Checklist

- [ ] Sandboxed iframe with `sandbox="allow-popups allow-popups-to-escape-sandbox allow-same-origin"`
- [ ] CSP meta tag: `script-src 'none'; object-src 'none'`
- [ ] Server-side ammonia sanitization before storing `body_html`
- [ ] `target="_blank" rel="noopener noreferrer"` on all links
- [ ] `referrerPolicy="no-referrer"` on iframe
- [ ] Remote image proxy to prevent tracking (future enhancement)
- [ ] Content-Disposition: attachment header on all download endpoints
- [ ] Path traversal prevention on filesystem storage paths
- [ ] Rate limiting on attachment download endpoints

---

## Sources

### Email Body Rendering
- [Rendering untrusted HTML email, safely (Close.com)](https://making.close.com/posts/rendering-untrusted-html-email-safely/)
- [DOMPurify - XSS Sanitizer](https://github.com/cure53/DOMPurify)
- [isomorphic-dompurify (npm)](https://www.npmjs.com/package/isomorphic-dompurify)
- [html-react-parser (npm)](https://www.npmjs.com/package/html-react-parser)
- [react-safe-src-doc-iframe (GoDaddy)](https://github.com/godaddy/react-safe-src-doc-iframe)
- [Safely Handling HTML in React (DEV Community)](https://dev.to/joshydev/safely-handling-html-in-react-ba)
- [Using the Shadow DOM as a Better Iframe (Mailcoach)](https://www.mailcoach.app/resources/blog/using-the-shadow-dom-as-a-better-iframe/)
- [Sanitising HTML: the DOM clobbering issue (Fastmail)](https://www.fastmail.com/blog/sanitising-html-the-dom-clobbering-issue/)

### Backend Parsing & Sanitization
- [mail-parser (crates.io)](https://crates.io/crates/mail-parser)
- [mail-parser GitHub (Stalwart Labs)](https://github.com/stalwartlabs/mail-parser)
- [Ammonia HTML Sanitizer (crates.io)](https://crates.io/crates/ammonia)
- [Ammonia GitHub](https://github.com/rust-ammonia/ammonia)
- [mailparse (crates.io)](https://crates.io/crates/mailparse)

### Email Provider APIs
- [Gmail API - users.messages.attachments](https://developers.google.com/gmail/api/reference/rest/v1/users.messages.attachments)
- [Microsoft Graph API - Get attachment](https://learn.microsoft.com/en-us/graph/api/attachment-get?view=graph-rest-1.0)
- [graph-rs-sdk (Rust Microsoft Graph)](https://github.com/sreeise/graph-rs-sdk)
- [google-gmail1 (Rust Gmail SDK)](https://crates.io/crates/google-gmail1)

### ZIP & File Handling
- [async_zip (crates.io)](https://lib.rs/crates/async_zip)
- [JSZip](https://stuk.github.io/jszip/)
- [file-saver (npm)](https://www.npmjs.com/package/file-saver)

### Reference Implementations
- [Roundcube Webmail](https://roundcube.net/)
- [Mailspring (GitHub)](https://github.com/Foundry376/Mailspring)
- [next-email-client (leerob)](https://github.com/leerob/next-email-client)
- [React Email](https://react.email)
