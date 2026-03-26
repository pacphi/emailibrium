# ADR-019: Email Body Rendering Strategy

- **Status**: Proposed
- **Date**: 2026-03-25
- **Extends**: DDD-008 (Email Operations), DDD-009 (Email Content & Attachments)
- **Research**: docs/research/email-interaction-enhancements.md

## Context

The current frontend renders email bodies using a regex-based `SanitizedHtml` component that strips `<script>` tags and `on*` event handlers, then injects via `dangerouslySetInnerHTML`. This approach has three critical gaps:

1. **Security vulnerability** — The regex sanitizer is trivially bypassed (e.g., `<img src=x onerror=alert(1)>`, data URIs, CSS expressions, SVG payloads). This is a known XSS vector.
2. **No HTML rendering fidelity** — HTML emails with tables, inline CSS, images, and complex layouts render as broken markup or stripped plain text. Users cannot read newsletters, receipts, or formatted correspondence.
3. **Body truncation** — Collapsed messages show only `email.bodyText?.slice(0, 100)` with no expand-to-full or HTML/plain-text toggle.

On the backend, Gmail and Outlook API fetchers extract `body_text` but not `body_html`, even though the `Email` TypeScript interface already carries a `bodyHtml` field and the provider APIs return HTML content.

## Decision

### 1. Sandboxed Iframe Rendering (Primary)

Adopt the **sandboxed iframe with `srcdoc`** pattern for rendering untrusted HTML email content. This is the industry standard used by Close.com, Roundcube, Mailspring, and ProtonMail.

**Triple-layer security:**

| Layer | Mechanism | Protection |
|---|---|---|
| 1 | `sandbox` attribute | Blocks JS execution, form submission, navigation. Whitelists only `allow-popups allow-popups-to-escape-sandbox allow-same-origin` |
| 2 | CSP meta tag | `<meta http-equiv="Content-Security-Policy" content="script-src 'none'; object-src 'none'">` injected into iframe document head |
| 3 | Referrer policy | `referrerPolicy="no-referrer"` prevents origin leakage to remote resources |

**Additional properties:**
- `<base target="_blank">` ensures all links open in a new tab
- Auto-resize via `contentDocument.scrollHeight` polling on iframe `load` event
- Full CSS isolation — email styles cannot leak into or from the host page

### 2. DOMPurify Sanitization (Snippets & Previews)

Use `isomorphic-dompurify` for rendering email previews in the email list (collapsed state) and notification toasts. The iframe is only instantiated when the user expands a message.

**Why isomorphic-dompurify:**
- ~1.9M weekly npm downloads, wraps DOMPurify (~14K GitHub stars)
- Works identically on server (jsdom) and client (native DOM)
- Apache-2.0 / MPL-2.0 license
- Single new frontend dependency

### 3. Backend HTML Extraction & Sanitization

- **Gmail API**: Extract `body_html` from the `text/html` MIME part in `payload.parts[].body.data` (base64url-encoded).
- **Outlook/Graph API**: Use the `body.content` field when `body.contentType === "html"`.
- **Server-side sanitization**: Apply `ammonia` (already in Cargo.toml) with an email-specific whitelist before storing `body_html` in the database. This provides defense-in-depth — even if the frontend iframe sandbox has a browser bug, stored HTML is pre-sanitized.

### 4. Content Type Rendering Strategy

| Content Available | Rendering Strategy |
|---|---|
| `bodyHtml` present | Sandboxed iframe (primary view) |
| `bodyText` only | `<pre>` with `whitespace-pre-wrap` and monospace font |
| Both (multipart/alternative) | Default to HTML with a toggle button for plain text |

### 5. Body Truncation Fix

- Remove the `slice(0, 100)` truncation from collapsed messages.
- Collapsed state shows a DOMPurify-sanitized snippet (first 200 characters of `bodyText`, or stripped `bodyHtml`).
- Expanded state renders the full email body via sandboxed iframe or pre-formatted text.

## Consequences

### Positive

- **Security**: Defense-in-depth eliminates the XSS vector. No single-layer bypass can succeed.
- **Rendering fidelity**: Full HTML email rendering with tables, images, inline CSS, fonts — matching native email client quality.
- **CSS isolation**: Email styles cannot bleed into the application UI or vice versa.
- **Minimal dependency footprint**: One new npm package (`isomorphic-dompurify`). Zero new Rust crates (ammonia already present).
- **Progressive enhancement**: Plain text fallback ensures all emails remain readable.

### Negative

- **iframe overhead**: Each expanded email instantiates an iframe. Mitigated by lazy rendering (only when expanded) and existing virtualization.
- **Auto-resize complexity**: iframe height calculation requires load-event polling and cross-origin safety catches. Edge cases with dynamically loaded images may require a ResizeObserver inside the iframe.
- **jsdom pinning**: `isomorphic-dompurify` may require pinning `jsdom` to 25.0.1 via npm overrides if Next.js SSR encounters ESM import failures.

### Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Browser sandbox bypass (0-day) | Low | Triple-layer defense; server-side ammonia sanitization as backstop |
| iframe auto-resize flicker | Medium | Set `minHeight: 200px`; debounce resize; use skeleton loader during iframe load |
| Performance with many expanded emails | Low | Only one email expanded at a time in current UX; virtualization handles the list |

## Alternatives Considered

### DOMPurify + dangerouslySetInnerHTML (no iframe)

**Rejected as primary.** No CSS isolation — email styles leak into the host page. Tailwind's `prose` class provides partial containment but is insufficient for complex newsletter layouts. Retained as the snippet/preview renderer.

### Shadow DOM

**Rejected.** Partial CSS isolation only. Cannot block script execution or navigation. No referrer policy control. Less battle-tested for email rendering than iframe sandboxing.

### html-react-parser

**Deferred.** Useful for element-level transformation (link rewriting, image src proxying) but adds complexity without solving the core rendering/security problem. Can be introduced later if CID rewriting or link decoration is needed on the frontend.

## Security Checklist

- [ ] `sandbox="allow-popups allow-popups-to-escape-sandbox allow-same-origin"` on iframe
- [ ] CSP meta tag: `script-src 'none'; object-src 'none'` in iframe document
- [ ] `referrerPolicy="no-referrer"` on iframe element
- [ ] `<base target="_blank">` in iframe document head
- [ ] Server-side ammonia sanitization before storing `body_html`
- [ ] `target="_blank" rel="noopener noreferrer"` on all links (ammonia `link_rel` config)
- [ ] DOMPurify `FORBID_TAGS` includes `script`, `iframe`, `object`, `embed`, `form`, `input`
