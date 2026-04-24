# Emailibrium Onboarding — Foolproof Zero-Cost Strategy

**Budget: $0. Goal: foolproof.** No CASA, no annual re-verification, no "try to avoid the cap and hope." This doc supersedes earlier plans that either required ~$3k+/yr (vendor-OAuth with CASA) or required gutting the product to stay under sensitive-scope limits (metadata-only). **Both were wrong.**

---

## The Audit Verdict (decisive)

A full audit of the Emailibrium codebase (2026-04-23) confirms:

**Full raw email bodies ARE persisted server-side.**

- `body_text TEXT` and `body_html TEXT` columns on `emails` table (`backend/migrations/001_initial_schema.sql:17-18`)
- FTS5 index indexes `body_text` (`backend/migrations/005_fts5_search.sql:14`, triggers at lines 26, 40-41)
- `upsert_email()` writes both columns (`backend/src/api/ingestion.rs:728-767`, binds at 756-757)
- All three providers fetch full bodies: Gmail `format=full` (`gmail.rs:444,479`), Outlook `$select=body` (`outlook.rs:259,317`), IMAP `RFC822` full message (`imap.rs`)
- Embeddings are **derived from**, not a **replacement for**, stored bodies — both persist
- Current scopes: Gmail `gmail.modify + gmail.labels + userinfo.email` (`vectors/config.rs:942-948`) = **restricted tier**; Graph `Mail.ReadWrite + Mail.Send + offline_access + User.Read` (`vectors/config.rs:964-970`) = fine.

### What this means

Emailibrium's product value **depends on** body access — semantic search, RAG, summaries, topic clustering, intelligent triage all require the body. Removing body persistence isn't a compliance optimization; it's a product amputation. We take it off the table.

Consequence: **Google Gmail on emailibrium.app (hosted SaaS) is paywalled at ~$3k/yr CASA, full stop.** No technical $0 path exists for hosted Gmail without rewriting the product.

Everything else — local/self-hosted distribution, Microsoft on any mode, IMAP/JMAP on any mode — has a clean $0 path.

---

## Foolproof Strategy

### Primary distribution: local / self-hosted

This is the real product. A user runs Emailibrium on their own machine (native app, Docker, homelab). Their data never leaves their machine. Full bodies, full scopes, no compliance entanglement.

- **Gmail / Google Workspace**: BYOC wizard — 7-minute one-time setup per user (create GCP project, enable Gmail API, make OAuth client, paste into Emailibrium). This is the only foolproof $0 path for Google: the user publishes their own "app" under their own GCP project, their own 100-test-user cap (which they'll never hit as themselves + family). This is exactly how `rclone`, Proton Mail Bridge, Nextcloud Mail, and Cal.com self-hosted all do it. Zero cost, zero liability for Emailibrium, zero cap concern.
- **Microsoft 365 / Outlook.com**: Vendor-shipped multi-tenant public-client app (`allowPublicClient: true`), baked into the binary. Free Publisher Verification. PKCE + loopback for desktop, RFC 8628 Device flow for headless. No cap, no cost, no CASA analog.
- **iCloud / Fastmail / Yahoo / independents**: IMAP + app password with auto-discovery (Thunderbird autoconfig DB bundled in-binary). Deep-link to each provider's app-password page.
- **JMAP** (Fastmail, Stalwart, Cyrus 3.8+, Apache James 3.8+): bearer token when detected, preferred over IMAP (push, batch, typed).

### Hosted emailibrium.app: restricted provider set

Offer **Microsoft + IMAP providers only** on hosted. No Google. Users who want Gmail get directed to self-host (one-command Docker, native installer). This sidesteps the 100-user cap entirely by not having Google OAuth on the hosted surface.

- **Microsoft on hosted**: same vendor multi-tenant app, web-client flow, Publisher Verified. Works for both personal Outlook.com and M365 business.
- **IMAP/JMAP on hosted**: Fastmail, iCloud, Yahoo, Workspace Gmail (via app password — user skips OAuth), any IMAP server.
- **Workspace Gmail via IMAP+app-password actually works on hosted** — it's not OAuth, so CASA doesn't apply. The user enables 2FA + generates an app password. This is a $0 workaround for Workspace users who land on emailibrium.app. Document that personal Gmail same-path is valid too as long as 2FA is on.
- **When revenue justifies CASA** (~$3k/yr), open Gmail OAuth on hosted as a tier upgrade. Until then: it's not there.

### Why this is foolproof

| Concern                          | Mitigation                                                               |
| -------------------------------- | ------------------------------------------------------------------------ |
| Google 100-user cap              | Not reachable — hosted has no Google OAuth                               |
| CASA $3k/yr                      | Not required — no restricted scopes on any vendor-published client       |
| Token broker SPOF                | Not built — RFC 8252 + BYOC make it unnecessary                          |
| Product compromise               | None — bodies stay, full features stay                                   |
| Self-host reliability            | BYOC means each user is their own publisher; no central dependency       |
| MS basic-auth deprecation        | Handled — we use OAuth public client for all MS accounts                 |
| Provider detection failure       | Thunderbird autoconfig DB (~700 providers, MPL 2.0) + fallback to manual |
| Hosted scaling blocked on Google | Accepted tradeoff; grow on MS + IMAP until Google is affordable          |

The only feature that requires money is "Gmail on hosted SaaS." Every other path is genuinely free forever.

---

## Architecture

```text
  LOCAL MODE                           HOSTED (emailibrium.app)
  ==========                           ========================

  Emailibrium binary                   User's browser ──HTTPS──┐
  (Rust, native/Docker)                                        │
                                                   emailibrium.app backend
  ┌─ Gmail (BYOC)  ──→ user's own GCP OAuth client            │
  ├─ MS  (baked-in vendor public-client app)                   │
  ├─ IMAP (auto-discover + app password)                       ├─ NO Google
  └─ JMAP (bearer token)                                       ├─ MS (web client, verified publisher)
                                                               ├─ IMAP (any provider inc. Workspace/Gmail via app password)
  Data: SQLite on user's disk                                  └─ JMAP (bearer token)
  Full bodies, full scopes, full features
                                                   Data: Postgres, envelope-encrypted tokens
                                                   Full bodies (under non-restricted scopes)
```

Single codebase, single flag: `EMAILIBRIUM_MODE=local|hosted`. In hosted mode, the Google OAuth connect UI is hidden and a message points to self-hosting. Everything else is identical.

---

## Revised Implementation Plan

### Phase 0 — Lock the decisions (this week, $0)

- **Decision A: Product model.** Self-hosted is primary distribution; hosted is a restricted-provider SaaS. Document in README and product copy.
- **Decision B: No token broker.** Kill the vendor-hosted token-broker phase from the original plan.
- **Decision C: Keep body persistence.** No architectural retreat on scopes. Google on hosted is a future revenue decision, not a current technical one.

### Phase 1 — IMAP-first onboarding (1-2 weeks, $0)

Highest immediate UX win. Works for everyone except M365 (which needs OAuth regardless).

- **Wire `POST /api/v1/auth/imap/connect`** — endpoint currently missing from `backend/src/api/accounts.rs`. Hand off to `ImapProvider::new()` (`backend/src/email/imap.rs:74`). Store password via existing AES-256-GCM machinery in `oauth.rs`.
- **Extend `connected_accounts`** — new migration after `023_app_settings.sql`: `imap_password_encrypted BLOB`, `imap_config_json TEXT`.
- **Bundle Thunderbird autoconfig DB** (MPL 2.0, `github.com/thunderbird/autoconfig`, ~2MB) into the binary. New module `backend/src/email/autoconfig.rs` — parallel lookup: Thunderbird DB → `autoconfig.<domain>` → SRV → autodiscover → MX heuristics. First success wins, ~800ms budget.
- **Frontend**: `ImapConnect.tsx:109` already calls the test endpoint — confirm the connect endpoint wiring is aligned.
- **Provider app-password deep links** (verified 2026-04):
  - Gmail: `https://myaccount.google.com/apppasswords` (needs 2FA first)
  - iCloud: `https://appleid.apple.com/account/manage`
  - Fastmail: `https://app.fastmail.com/settings/security/tokens/new`
  - Yahoo: `https://login.yahoo.com/account/security/app-passwords`
- **UX**: show what the app password format looks like before the deep link. Live connection test with specific diagnostics, not generic "failed."

### Phase 2 — Microsoft public-client OAuth (parallel, 1 week, $0)

- Register one Entra ID multi-tenant app: `signInAudience: AzureADandPersonalMicrosoftAccount`, `allowPublicClient: true`.
- Start Microsoft Publisher Verification (free, 1-5 days). MPN ID also free. Removes "unverified publisher" warning and unlocks admin-consent UI.
- Implement PKCE + loopback redirect in `backend/src/email/oauth.rs` for local mode.
- Implement web redirect flow for hosted mode.
- Implement RFC 8628 Device Authorization Grant for headless self-hosts (endpoint `https://login.microsoftonline.com/common/oauth2/v2.0/devicecode`).
- Existing scopes (`Mail.ReadWrite + Mail.Send + offline_access + User.Read`) are correct — no change needed.
- Fix the minor inconsistency at `backend/src/email/outlook.rs:1023` where test config uses `Mail.Read`.

### Phase 3 — Google BYOC wizard for local mode (1-2 weeks, $0)

- New frontend component: "Connect Gmail" (local mode only) → guided 6-step wizard:
  1. Open `console.cloud.google.com/projectcreate` (new tab)
  2. Open `console.cloud.google.com/apis/library/gmail.googleapis.com` → Enable
  3. Open OAuth consent screen config → External, Testing status, add self as test user
  4. Add scopes: `gmail.modify`, `gmail.labels`, `userinfo.email`
  5. Open `console.cloud.google.com/apis/credentials/oauthclient` → Desktop app → create
  6. Paste-the-JSON input box in Emailibrium — ingest `client_id` + `client_secret`
- Store per-install BYOC creds in `app_settings` or a new `oauth_apps` table. Do NOT use env vars (too technical; the whole point is non-technical-user path).
- Reuse existing OAuth code paths in `backend/src/email/oauth.rs` — the flow is identical once creds are obtained; only the source changes (env vs DB).
- In-app explainer: "You'll see an 'unverified app' warning — click Advanced → Continue. This is because you published the app to yourself; Google hasn't audited it because there's nothing to audit."

### Phase 4 — Deployment-mode gating (1 week, $0)

- Add `EMAILIBRIUM_MODE=local|hosted` in `backend/src/vectors/config.rs`.
- Hosted mode: Google OAuth routes return 501 with a message pointing to self-host; Google provider UI hidden on frontend.
- Local mode: all providers visible.
- Document the modes in README.

### Phase 5 — Distribution polish for self-host (2-3 weeks, $0)

This is where the product lives. Make it trivial to run locally.

- **Docker compose** with SQLite volume, sane defaults. One-command `docker compose up`.
- **Tauri desktop binary** (Windows/macOS/Linux) wrapping the backend + frontend. Consider for Phase 6.
- **Installer scripts** (Homebrew tap, `curl | sh` with the usual caveats).
- **Loopback OAuth redirect handling** — ephemeral port, browser auto-launch.

### Phase 6 — Optional: JMAP + Forward-to-Ingest

- JMAP provider module (`backend/src/email/jmap.rs`) alongside existing providers. Detect via session endpoint for Fastmail (`https://api.fastmail.com/jmap/session`) / Stalwart. Bearer token UX similar to IMAP app password.
- Forward-to-ingest subdomain (`ingest.emailibrium.app`) with ARC header verification (RFC 8617). Compliance-locked tenants only; low priority.

### Explicitly dropped from earlier plans

- ~~Vendor-hosted token broker~~ — RFC 8252 + BYOC make it unnecessary. Would be SPOF + CASA liability.
- ~~Google CASA Tier 2 verification~~ — deferred indefinitely. Revisit only when hosted revenue justifies it.
- ~~"Hosted Gmail under metadata-only scopes"~~ — would require gutting semantic search, summaries, RAG, clustering. Not foolproof; it's a product rewrite.
- ~~"Google shipped Desktop-app client with baked-in secret"~~ — **dropped for local mode too.** The 100-user cap applies to the publisher's project regardless of client type, and unverified-app warnings would scare non-technical users. BYOC is cleaner and cap-free.

---

## End-User Experience

| User                         | Path                                         | Steps                                                                           |
| ---------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------- |
| Local, personal Gmail        | BYOC wizard                                  | Once: 7 min GCP setup, paste JSON. Then: click Connect → browser consent → done |
| Local, M365 work             | Vendor MS app, loopback                      | Click Connect → browser consent → done                                          |
| Local, iCloud/Fastmail/Yahoo | IMAP + app password                          | Enter email → deep-link to provider → paste password → done (~1 min)            |
| Local, headless server       | MS device flow / BYOC for Google             | Enter code on `microsoft.com/devicelogin` / run BYOC wizard once                |
| Hosted, MS user              | Vendor MS app, web flow                      | Sign in → done                                                                  |
| Hosted, Fastmail/iCloud/etc  | IMAP + app password                          | Enter email → deep-link → paste → done                                          |
| Hosted, Gmail user           | Redirected to self-host OR IMAP+app-password | Either install locally OR use IMAP path with Gmail app password (2FA required)  |
| Hosted, M365 business        | Vendor MS app with admin consent URL         | IT hits admin-consent URL once for tenant; users then sign in normally          |

---

## Immediate Moves (this week)

1. **Commit to the self-host-primary, MS+IMAP-on-hosted model.** Update the README.
2. **Wire the IMAP connect endpoint** and bundle the Thunderbird autoconfig DB. Single most-impactful change.
3. **Start Microsoft Publisher Verification** — free, 1-5 days, unlocks clean MS onboarding for both modes.
4. **Remove Gmail env-var requirement** from `backend/src/api/accounts.rs:106-121` and `provider_helpers.rs:13-27` — these error out today if env vars are missing; under BYOC they should fall through to a per-user credential lookup instead.

---

## Appendix A — Provider Matrix (2026-04)

| Provider                 | OAuth public client             | Device flow                | IMAP + app password      | JMAP                | On hosted? |
| ------------------------ | ------------------------------- | -------------------------- | ------------------------ | ------------------- | ---------- |
| Gmail personal           | Yes (BYOC only under this plan) | No (Gmail scopes excluded) | Yes (2FA required)       | No                  | IMAP only  |
| Google Workspace         | Yes (BYOC)                      | No                         | Yes (admin may disable)  | No                  | IMAP only  |
| Outlook.com personal     | Yes (vendor app)                | Yes                        | No — killed Sep 2024     | No                  | OAuth      |
| M365 business            | Yes (vendor app)                | Yes                        | No — killed Oct 2022     | No                  | OAuth      |
| iCloud                   | No OAuth                        | No                         | Yes                      | No                  | IMAP       |
| Fastmail                 | Yes                             | Yes                        | Yes                      | **Yes (preferred)** | JMAP/IMAP  |
| Yahoo                    | No (deprecated)                 | No                         | Yes (app password + 2FA) | No                  | IMAP       |
| ProtonMail               | Bridge only (local 1143/1025)   | No                         | Bridge                   | No                  | Local only |
| Stalwart / Cyrus / James | n/a                             | n/a                        | Yes                      | **Yes**             | JMAP       |
| GMX / Web.de             | No                              | No                         | Yes (manual toggle)      | No                  | IMAP       |

## Appendix B — Cost Summary

| Item                                                 | Cost                  |
| ---------------------------------------------------- | --------------------- |
| Microsoft app registration, any number               | $0                    |
| Microsoft Publisher Verification + MPN ID            | $0                    |
| Google OAuth via BYOC (user's own project)           | $0                    |
| Google unverified app ≤100 grantees per user project | $0                    |
| Thunderbird autoconfig DB (MPL 2.0 snapshot)         | $0                    |
| Self-host distribution (Docker / Tauri)              | $0                    |
| Hosted emailibrium.app with MS + IMAP only           | $0                    |
| Magic-link auth (Resend free tier, 3k/mo)            | $0                    |
| **Hosted Google Gmail + CASA**                       | $3k/yr **— deferred** |

## Appendix C — Current Code Anchors (audit 2026-04-23)

| Concern                           | File:Line                                                          |
| --------------------------------- | ------------------------------------------------------------------ |
| Body columns                      | `backend/migrations/001_initial_schema.sql:17-18`                  |
| FTS5 body index                   | `backend/migrations/005_fts5_search.sql:14,26,40-41`               |
| Body upsert                       | `backend/src/api/ingestion.rs:728-767` (binds at 756-757)          |
| Gmail fetch (format=full)         | `backend/src/email/gmail.rs:444,479`                               |
| Outlook fetch (body in $select)   | `backend/src/email/outlook.rs:259,317`                             |
| IMAP fetch (RFC822)               | `backend/src/email/imap.rs` (full message)                         |
| Gmail scope config                | `backend/src/vectors/config.rs:942-948`                            |
| Outlook scope config              | `backend/src/vectors/config.rs:964-970`                            |
| Outlook test-config inconsistency | `backend/src/email/outlook.rs:1023` (uses only `Mail.Read`)        |
| Env-var-gated OAuth creds         | `backend/src/api/accounts.rs:106-121`, `provider_helpers.rs:13-27` |
| Account routes nest               | `backend/src/api/mod.rs:41`                                        |
| IMAP provider (ready, unwired)    | `backend/src/email/imap.rs:74`                                     |

## Sources

- Google: `developers.google.com/identity/protocols/oauth2/native-app`, `developers.google.com/identity/protocols/oauth2/scopes#gmail`, `developers.google.com/identity/protocols/oauth2/limited-input-device#allowedscopes`, `support.google.com/cloud/answer/10311615`, `support.google.com/accounts/answer/185833`
- Microsoft: `learn.microsoft.com/entra/identity-platform/v2-oauth2-device-code`, `learn.microsoft.com/entra/identity-platform/publisher-verification-overview`, `support.microsoft.com/office/modern-authentication-for-outlook-com`
- IETF: RFC 8252, 7636, 8628, 8620/8621, 6186, 8617
- Thunderbird autoconfig: `github.com/thunderbird/autoconfig` (MPL 2.0)
- OSS precedent studied: Thunderbird, Evolution, Nextcloud Mail, Cal.com, Cypht, rclone, Proton Bridge, Mimestream
