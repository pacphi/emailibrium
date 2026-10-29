# Quick Start — First Value in 15 Minutes

This is the fastest way to see Emailibrium working on your own mailbox **without any Google
Cloud or Microsoft Azure setup**. It uses **IMAP + an app password** — a credential you
generate inside your own email account in a couple of clicks.

> **Already comfortable with OAuth, or need Outlook.com / Microsoft 365?** Skip to
> [Which path should I use?](#which-path-should-i-use) — those accounts require the OAuth flow,
> documented in the [OAuth Setup Guide](docs/oauth-setup-guide.md).

---

## Which path should I use?

| Your account                         | Fastest working path                                       | Cloud setup needed?    |
| ------------------------------------ | ---------------------------------------------------------- | ---------------------- |
| **Personal Gmail** (`@gmail.com`)    | **IMAP + app password** (this guide)                       | None                   |
| **Google Workspace** (custom domain) | **IMAP + app password** (this guide)                       | None                   |
| **Yahoo, iCloud, Fastmail, Zoho**    | **IMAP + app password** (this guide)                       | None                   |
| **Outlook.com / Hotmail / Live**     | **OAuth** ([OAuth Setup Guide](docs/oauth-setup-guide.md)) | Azure app registration |

**Why the split?** Microsoft ended IMAP basic auth for Outlook.com accounts in September 2024 —
those now require OAuth. **Google Workspace app passwords remain the exception, not the
casualty**: Google's own transition guidance keeps IMAP app passwords available for Workspace
accounts (2-Step Verification on; a Workspace admin can still block app passwords or enforce
security keys org-wide — check with your admin if the app password option doesn't appear).
**Personal Gmail works the same way**, which is why both are on the no-setup fast path.

---

## The 15-minute path (IMAP + app password)

### 1. Generate an app password (≈3 min)

**Gmail (personal):**

1. Enable **2-Step Verification** on your Google account (required for app passwords):
   <https://myaccount.google.com/security>
2. Generate an app password at <https://myaccount.google.com/apppasswords> — name it
   `emailibrium`. Copy the 16-character password (no spaces).
3. Server settings: IMAP `imap.gmail.com` port `993` (SSL); SMTP `smtp.gmail.com` port `465`.

**Other providers** (Yahoo, iCloud, Fastmail, Zoho): generate an app-specific password in your
account's security settings. The onboarding screen includes presets for their server/port values.

### 2. Set up and start the app (≈8 min)

```bash
git clone https://github.com/pacphi/emailibrium.git
cd emailibrium

just setup            # interactive wizard — SKIP the OAuth credential prompts (press Enter)
just install
just dev              # → Backend: http://localhost:8080  Frontend: http://localhost:3000
```

OAuth is optional — pressing Enter at the Google/Microsoft prompts writes placeholders and the
backend still boots normally. (Docker alternative: `just setup-secrets` then `just docker-up-dev`.)

### 3. Unlock the local engine (≈1 min)

The frontend opens to **Connect to your engine** — the mailbox stays locked until this browser
has a verified session with the backend.

1. Copy the token: `cat secrets/dev/jwt_secret` (native) — Docker Compose delivers it to the
   backend automatically, but you still paste the same file's contents into the browser.
2. Paste it into **Local access token** and click **Connect securely**.

The token is exchanged once for an HttpOnly session cookie; it's never stored in the browser.
The session lasts up to eight hours and ends when the engine restarts. See
[Local engine access](docs/security/local-engine-access.md) for details.

### 4. Connect your mailbox (≈2 min)

1. Open <http://localhost:3000/onboarding>.
2. Choose **Connect via IMAP**.
3. (Optional) pick a **provider preset** to auto-fill server/ports.
4. Enter your **email address** and the **app password** from step 1.
5. Click **Test Connection** — you should see _Connection successful!_
6. Click **Connect Account**. Your account now appears in the account list. 🎉

That's first value: your mailbox is connected and ready to sync, classify, and search — with no
cloud project to create.

---

## Notes & caveats

- **Encryption mode:** prefer **SSL** (implicit TLS, port 993) — it's the most secure and widely
  supported. **STARTTLS** (port 143) is also supported for servers that require it; it fails closed
  (never falls back to cleartext). **None** (plaintext) is intended only for trusted local relays
  such as ProtonMail Bridge.
- **Credentials at rest:** your app password is encrypted with AES-256-GCM before it is stored.
- **Outlook.com / Hotmail / Live:** the IMAP form will not work for these — use the
  [OAuth Setup Guide](docs/oauth-setup-guide.md). For Gmail OAuth, _Testing mode_ lets you connect
  a handful of test users immediately without Google's multi-week app verification.
- **Google Workspace admin-blocked app passwords:** if your organization disables app passwords
  or enforces security keys, the IMAP form will fail at step 4 — use the
  [OAuth Setup Guide](docs/oauth-setup-guide.md) instead.

## Next steps

- [Setup Guide](docs/setup-guide.md) — full setup, prerequisites, and configuration
- [OAuth Setup Guide](docs/oauth-setup-guide.md) — Gmail/Outlook OAuth (Testing and Production)
- [Configuration Reference](docs/configuration-reference.md) — all config keys
- [Local Engine Access](docs/security/local-engine-access.md) — how the engine-unlock session works
