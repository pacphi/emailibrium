# Emailibrium Onboarding Friction — Feasibility Plan

## Problem

Today each user must create their own Google Cloud project or Azure app registration to get OAuth client credentials — a dealbreaker for non-technical users. Config is app-global via env vars (`EMAILIBRIUM_GOOGLE_CLIENT_ID`, etc.), so whoever runs the instance bears that burden.

## The 2026 Landscape (Why No Silver Bullet)

- **Gmail**: Less Secure Apps gone since 2022. App Passwords still work (require 2FA) for personal Gmail + Workspace-if-admin-allows. IMAP third-party clients need either app-password or OAuth XOAUTH2.
- **Microsoft 365 (work)**: Basic auth dead since Oct 2022; SMTP AUTH deprecated Sep 2025. **OAuth is mandatory** — no app-password escape hatch.
- **Outlook.com (personal)**: App passwords still work over IMAP — one remaining MS basic-auth path.
- **Device flow**: works on Graph, **blocked by Google for Gmail restricted scopes**.
- **No IETF delegated-mailbox standard** has shipped; OAuth remains the de facto.

## Recommended Strategy: Three-Tier Onboarding

### Tier A — Vendor-Published Multi-Tenant OAuth (primary path, 95% of users)

Emailibrium-the-vendor absorbs the GCP/Azure pain once. End user clicks "Sign in with Google / Microsoft" — zero cloud-console knowledge.

| Item                                    | Cost    | Time       |
| --------------------------------------- | ------- | ---------- |
| MS Publisher Verification (do first)    | $0      | days       |
| Google OAuth verification + CASA Tier 2 | ~$3k    | 6–10 weeks |
| Annual Google re-verification           | ~$3k/yr | 2–4 weeks  |

**Architectural constraint to keep CASA at Tier 2 (not $10k+ Tier 3): don't persist mail bodies server-side.** Emailibrium already stores vectors/embeddings rather than raw messages, which is aligned — need to verify this in the sync path before committing.

**Self-hosted twist**: Google policy forbids distributing the client_secret. Solution = **vendor-hosted token-broker service** (HTTPS) that performs code exchange/refresh on behalf of self-hosted instances. For air-gapped installs, keep BYO-OAuth as the escape hatch.

### Tier B — IMAP + App Password (fallback for non-OAuth users)

Works for: personal Gmail, Google Workspace (if admin hasn't disabled), personal Outlook.com, Fastmail, iCloud, Yahoo, etc. **Does not work** for M365 work accounts — UI must say so explicitly.

**Good news**: code is already ~70% there. `ImapProvider` exists at `backend/src/email/imap.rs:69`, the frontend `ImapConnect.tsx` form is built with provider presets, but the backend route `POST /api/v1/auth/imap/connect` is **not wired** (`backend/src/api/mod.rs:25-42`). Finishing this is the smallest shippable win.

### Tier C — Forward-to-Ingest (universal last resort)

One-way ingest address. No send, no folder ops, no backfill. Useful for compliance-locked tenants where nothing else works.

---

## Implementation Plan (Anchored to Current Code)

### Phase 1 — Finish IMAP path (1–2 weeks, unblocks immediately)

- Wire `POST /api/v1/auth/imap/connect` in `backend/src/api/mod.rs:25-42` → hand off to `ImapProvider`.
- Extend `connected_accounts` schema (`backend/migrations/004_accounts.sql`) with `imap_password_encrypted`, `imap_config_json` columns — reuse existing AES-256-GCM machinery in `backend/src/email/oauth.rs:600-654`.
- Validate test-connection endpoint (frontend already calls it at `ImapConnect.tsx:109`).

### Phase 2 — Microsoft Publisher Verification (parallel, ~1 week)

- Free, fast. Immediately removes the "unverified publisher" warning. Enables admin-consent URL (`/adminconsent?client_id=…`) for B2B one-click IT approval.

### Phase 3 — Google CASA Tier 2 verification (~3 months, $3k)

- Switch Gmail scope from current `gmail.modify + gmail.labels` to minimum set; avoid `https://mail.google.com/` full-access.
- Confirm no raw mail body persistence (audit the sync path) to stay in Tier 2.
- Brand assets, privacy policy, scope-justification video.

### Phase 4 — Token-broker service for self-hosted (~2–3 weeks)

- New hosted HTTPS endpoint on Emailibrium domain that holds the confidential secret, performs code exchange + refresh, returns tokens to the self-hosted instance over mTLS or bearer-auth.
- Refactor `OAuthManager` in `backend/src/email/oauth.rs` to support a "broker mode" alongside the existing direct-to-provider mode. Config flag in `vectors/config.rs`.

### Phase 5 — Optional: per-tenant OAuth apps table (later)

- For enterprise self-hosters who want their own OAuth registration but without the per-user model: new `oauth_apps` table, DB-backed credential resolution replacing the env-var lookup in `accounts.rs:102-168`.

---

## Net End-User Experience After Rollout

| User type                 | Steps today                                                                  | Steps after                                                                  |
| ------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Personal Gmail            | Create GCP project, configure consent, copy client ID/secret, paste into env | Click "Sign in with Google"                                                  |
| M365 work                 | Azure app registration, admin consent dance                                  | Click "Sign in with Microsoft" (+ IT hits admin-consent URL once for tenant) |
| Fastmail / iCloud / Yahoo | Not supported cleanly                                                        | Paste IMAP app-password in pre-filled form                                   |
| Self-hosted               | Register own GCP app                                                         | Same "Sign in" button via vendor token broker                                |

---

## Top Risks

1. **CASA Tier 3 trap** — if Emailibrium ever persists full mail bodies, cost jumps $3k → $10–18k. Architectural decision needed now.
2. **Token-broker is now critical infra** — self-hosters depend on vendor uptime. Need SLO + fallback to BYO-OAuth.
3. **Google verification timeline** blocks new-user growth past the 100-account unverified cap. Start CASA process before that becomes the bottleneck.

---

## Recommended Immediate Moves

1. Start MS Publisher Verification **this week** (free, days).
2. Kick off Phase 1 (IMAP wiring) — fastest visible UX win.
3. Audit sync path for any raw-body persistence → lock in Tier-2-eligible architecture before starting CASA.

---

## Appendix A — Auth Alternatives Comparison Matrix

| Option                              | Provider               | User Steps                                                                         | Admin Burden        | Security                                              | 2026 Status                                                      |
| ----------------------------------- | ---------------------- | ---------------------------------------------------------------------------------- | ------------------- | ----------------------------------------------------- | ---------------------------------------------------------------- |
| App Password + IMAP                 | Gmail                  | Enable 2FA → myaccount.google.com/apppasswords → generate 16-char password → paste | None                | Medium (password-equivalent, bypasses MFA, revocable) | Supported for personal Gmail + Workspace (unless admin disables) |
| App Password + IMAP/POP             | Outlook.com (personal) | Enable 2FA → account.microsoft.com/security → "Create app password" → paste        | None                | Medium                                                | Supported for consumer Outlook.com only                          |
| App Password                        | Microsoft 365 (work)   | —                                                                                  | —                   | —                                                     | **Deprecated.** Basic auth killed Oct 2022                       |
| IMAP + OAuth (XOAUTH2)              | Gmail / M365           | Click "Sign in with Google/Microsoft" → consent screen                             | None (if verified)  | High                                                  | Supported; required for Gmail IMAP third-party apps              |
| Vendor-published multi-tenant OAuth | Google                 | Click consent link → approve scopes                                                | None                | High                                                  | Best path. Needs OAuth verification + CASA Tier 2/3              |
| Vendor-published multi-tenant OAuth | Microsoft              | Click consent → approve                                                            | IT admin for tenant | High                                                  | Needs MS Publisher Verification                                  |
| Domain-wide delegation              | Google Workspace       | —                                                                                  | IT admin only       | High                                                  | Supported but admin-gated                                        |
| Application Access Policy           | M365                   | —                                                                                  | IT admin            | High                                                  | Pairs with admin-consented app                                   |
| JMAP                                | Fastmail, Stalwart     | Generate API token in settings → paste                                             | None                | High (scoped tokens)                                  | Stable (RFC 8620/8621); limited provider support                 |
| Forward-to-ingest address           | Any                    | Add forwarding rule in webmail → verify ingest address                             | None                | Low-Medium                                            | Always works; degraded feature set                               |
| .mbox / Google Takeout              | Gmail                  | Export via Takeout → upload                                                        | None                | High (offline)                                        | Batch only, no live sync                                         |
| MASA / delegated mailbox IETF       | —                      | —                                                                                  | —                   | —                                                     | **Not standardized** as of 2026                                  |

---

## Appendix B — Cost / Time Summary (Vendor-Side OAuth Publishing)

| Item                                 | Cost   | Time               |
| ------------------------------------ | ------ | ------------------ |
| Google CASA Tier 2                   | $2–4k  | 6–10 wks           |
| Google CASA Tier 3 (if storing mail) | $8–18k | 10–16 wks          |
| Google brand/scope review            | $0     | 2–6 wks after CASA |
| Microsoft Publisher Verification     | $0     | 1–5 days           |
| Annual Google re-verification        | $2–4k  | 2–4 wks            |

Unverified-app quota (Google) remains capped at **100 distinct Google accounts lifetime** with an interstitial warning — only viable for closed beta.

---

## Sources

- Google: `support.google.com/accounts/answer/185833` (App Passwords), `support.google.com/cloud/answer/9110914` (OAuth verification), `developers.google.com/identity/protocols/oauth2/production-readiness/casa-assessment`, `cloud.google.com/security/compliance/casa`
- Microsoft: `learn.microsoft.com/exchange/clients-and-mobile-in-exchange-online/deprecation-of-basic-authentication-exchange-online`, `learn.microsoft.com/entra/identity-platform/v2-admin-consent`, `learn.microsoft.com/entra/identity-platform/publisher-verification-overview`, `learn.microsoft.com/graph/permissions-reference`
- IETF: RFC 8620 (JMAP Core), RFC 8621 (JMAP Mail), RFC 8252 §7.3 (loopback redirect), RFC 7636 (PKCE)
- Fastmail: `www.fastmail.com/for-developers/integrating-with-fastmail/`
