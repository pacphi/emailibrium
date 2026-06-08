# Quick Start — First Value in 15 Minutes

This is the fastest way to see Emailibrium working on your own mailbox **without any Google
Cloud or Microsoft Azure setup**. It uses **IMAP + an app password** — a credential you
generate inside your own email account in a couple of clicks.

> **Already comfortable with OAuth, or need Outlook / Google Workspace?** Skip to
> [Which path should I use?](#which-path-should-i-use) — those accounts require the OAuth flow,
> documented in the [OAuth Setup Guide](docs/oauth-setup-guide.md).

---

## Which path should I use?

| Your account                         | Fastest working path                                       | Cloud setup needed?    |
| ------------------------------------ | ---------------------------------------------------------- | ---------------------- |
| **Personal Gmail** (`@gmail.com`)    | **IMAP + app password** (this guide)                       | None                   |
| **Yahoo, iCloud, Fastmail, Zoho**    | **IMAP + app password** (this guide)                       | None                   |
| **Outlook.com / Hotmail / Live**     | **OAuth** ([OAuth Setup Guide](docs/oauth-setup-guide.md)) | Azure app registration |
| **Google Workspace** (custom domain) | **OAuth** ([OAuth Setup Guide](docs/oauth-setup-guide.md)) | Google Cloud project   |

**Why the split?** As of 2025–2026, Microsoft (Outlook.com, Sept 2024) and Google Workspace
(May 2025) disabled IMAP app passwords / basic auth — those accounts now require OAuth.
**Personal Gmail still supports app passwords** (with 2-Step Verification on), which is why it
remains the no-setup fast path.

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

make setup            # interactive wizard — SKIP the OAuth credential prompts (press Enter)
make install
make dev              # → Backend: http://localhost:8080  Frontend: http://localhost:3000
```

OAuth is optional — pressing Enter at the Google/Microsoft prompts writes placeholders and the
backend still boots normally. (Docker alternative: `make setup-secrets` then `make docker-up-dev`.)

### 3. Connect your mailbox (≈2 min)

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

- **Encryption mode:** choose **SSL** (implicit TLS on port 993). STARTTLS and plaintext are not
  yet supported by the IMAP provider.
- **Credentials at rest:** your app password is encrypted with AES-256-GCM before it is stored.
- **Outlook.com / Workspace:** the IMAP form will not work for these — use the
  [OAuth Setup Guide](docs/oauth-setup-guide.md). For Gmail OAuth, _Testing mode_ lets you connect
  a handful of test users immediately without Google's multi-week app verification.

## Next steps

- [Setup Guide](docs/setup-guide.md) — full setup, prerequisites, and configuration
- [OAuth Setup Guide](docs/oauth-setup-guide.md) — Gmail/Outlook OAuth (Testing and Production)
- [Configuration Reference](docs/configuration-reference.md) — all config keys
