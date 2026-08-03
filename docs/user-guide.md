# User Guide

## Getting Started

### Connecting Your Email

1. Open Emailibrium at `http://localhost:3000`
2. Navigate to **Settings** (gear icon in the sidebar)
3. Click **Add Email Account**
4. Select your provider:
   - **Gmail**: Authenticate via Google OAuth. Emailibrium requests read-only access to your mailbox.
   - **Outlook**: Authenticate via Microsoft OAuth.
   - **IMAP**: Enter your IMAP server, port, username, and password.
5. Emailibrium verifies the connection and begins initial sync

All email data is processed and stored locally on your machine. No email content is transmitted to external servers unless you explicitly enable cloud LLM features.

### First-Time Analysis

After connecting your account, Emailibrium begins the ingestion pipeline:

1. **Syncing**: Fetches email metadata and content from your provider
2. **Embedding**: Generates vector embeddings for each email (~500 emails/sec for text)
3. **Categorizing**: Classifies emails into categories (Work, Personal, Finance, etc.)
4. **Clustering**: Groups related emails by topic
5. **Analyzing**: Detects subscriptions, recurring senders, and patterns

Progress is displayed in real time via the Command Center. You can start using search and browse features while ingestion is still running -- results appear as emails are processed.

### Inbox Cleaner

The Inbox Cleaner is the fastest path to inbox zero:

1. Open **Inbox Cleaner** from the sidebar
2. Click **Start Analysis** to scan your inbox
3. Emailibrium groups your emails by category and topic
4. For each group, choose an action:
   - **Archive**: Move to archive (keeps the email, removes from inbox)
   - **Delete**: Permanently delete
   - **Label**: Apply a label/folder
   - **Keep**: Leave in inbox
   - **Skip**: Decide later
5. Use **batch actions** to process entire groups at once
6. Review the summary before confirming

## Features

### Command Center

The Command Center is the home screen and primary dashboard.

- **Quick stats**: Total emails, unread count, categories breakdown
- **Ingestion progress**: Real-time status of email processing
- **Recent activity**: Latest actions taken
- **Quick actions**: Clean Inbox, View Insights, Chat with AI, Manage Rules, Add Account, Sync Now

Press `Cmd+K` to open the command palette from anywhere in the application.

### Semantic Search

Emailibrium uses vector similarity to understand the meaning of your search, not just keywords.

**How to search:**

1. Click the search bar or press `/` to focus it
2. Type a natural language query (e.g., "budget reports from last quarter")
3. Results are ranked by semantic similarity, not keyword frequency
4. Filter by collection, category, date range, or sender

**Search tips:**

- Describe what you are looking for in plain language
- Longer, more specific queries produce better results
- Use filters to narrow results when you have many matches
- The system learns from your clicks to improve future results

**Collections:**

| Collection      | Content                                                     |
| --------------- | ----------------------------------------------------------- |
| Email Text      | Subject + sender + body text                                |
| Image Text      | OCR text extracted from inline images and image attachments |
| Image Visual    | Visual similarity (CLIP embeddings) for image content       |
| Attachment Text | Text extracted from PDF, DOCX, and XLSX attachments         |

### Insights Explorer

The Insights Explorer surfaces patterns in your email that are hard to spot manually.

**Subscriptions:**

- Lists all detected newsletter and marketing subscriptions
- Shows send frequency, read rate, and unsubscribe availability
- One-click unsubscribe for emails with `List-Unsubscribe` headers
- Sort by frequency, last received, or estimated read rate

**Recurring Senders:**

- Identifies senders who email at regular intervals
- Classifies patterns: daily, weekly, bi-weekly, monthly, quarterly
- Helps identify automated notifications that could be filtered

**Inbox Report:**

- Aggregated view of your inbox health
- Category distribution chart
- Top senders by volume
- Estimated time savings from using Emailibrium
- Actionable recommendations

### Email Client

Emailibrium includes a built-in email client for viewing, replying, and composing emails.

**Viewing emails:**

- Click any email in a list to open the reading pane
- Inline images and attachments are displayed within the email
- Thread view groups related emails in a conversation

**Composing and replying:**

- Click **Compose** to start a new email
- Click **Reply** to reply to the current email
- Click **Forward** to forward
- Markdown-aware compose box with a Write/Preview toggle (no rich-text formatting toolbar)

### Rules Studio

Create automation rules that run on incoming emails.

**Creating a rule:**

1. Open **Rules Studio** from the sidebar
2. Click **New Rule**
3. Define conditions:
   - Sender (From) matches
   - Recipient (To) matches
   - Subject contains
   - Body contains
   - Label equals
   - Date compares
   - Semantic similarity to a reference email
4. Define actions:
   - Apply label
   - Archive
   - Delete
   - Forward to another address
   - Mark as read
5. Set priority (rules are evaluated in priority order)
6. Save and enable the rule

Rules are evaluated during ingestion and can also be applied retroactively to existing emails.

> **Known issue:** Saving a rule can currently fail silently, for two independent reasons: (1) a
> freshly-added condition row doesn't send the `type` discriminant the backend requires, and (2)
> four of the six operator choices — Starts with, Ends with, Matches regex, Not contains — use a
> naming format the backend doesn't recognize (Not contains has no backend equivalent at all).
> Only **Contains** and **Equals** are safe to select on any condition, new or existing, until
> this is fixed.

### Settings

**Account settings:**

- Add/remove email accounts
- Configure sync frequency
- Enable/disable full-body sync (for bandwidth-constrained connections)

**Privacy settings:**

- Encryption-at-rest toggle and master-password field (UI only today — see the note below)
- Audit log of vector store access (currently a static example, not live data)

> **Note:** Real encryption at rest (AES-256-GCM, Argon2id) is configured through
> `backend/config.yaml`'s `encryption.enabled` / `master_password` keys, not through this screen —
> the Settings toggle and master-password field don't yet drive the backend encryption path. See
> the [Configuration Reference](configuration-reference.md) for the real settings.

**Appearance:**

- Light/dark theme
- Sidebar layout preferences
- Notification preferences

**AI Settings:**

- Embedding provider selection: Built-in ONNX (default, runs locally with no external service), Ollama, cloud (OpenAI, Cohere)
- Generative AI provider: Built-in local LLM (default, no setup needed), Local (Ollama), or cloud (OpenAI, Anthropic) — set to **None (Rule-based)** for keyword/heuristic classification with no AI model
- Provider-aware model selection with per-provider API key configuration

**Advanced:**

- Vector store configuration
- Quantization settings
- Export/import configuration

## Keyboard Shortcuts

| Shortcut                       | Action                         |
| ------------------------------ | ------------------------------ |
| `Cmd+K` / `Ctrl+K`             | Open command palette           |
| `Escape`                       | Close modal / palette / panel  |
| `↓` / `↑`                      | Navigate the email list        |
| `C`                            | Compose new email              |
| `R`                            | Reply to email                 |
| `F`                            | Forward email                  |
| `E`                            | Archive email                  |
| `Shift+#`                      | Delete email                   |
| `Cmd+Shift+A` / `Ctrl+Shift+A` | Select all emails              |
| `Cmd+,` / `Ctrl+,`             | Open settings                  |
| `Shift+?`                      | Show keyboard shortcuts        |
| `Cmd+Enter`                    | Send reply (in the reply box)  |
| `Enter`                        | Send message (in the Chat box) |
| `Shift+Enter`                  | New line (in the Chat box)     |

`C`/`R`/`F`/`E`/`Shift+#`/`Shift+?` are suppressed while typing in any text field, so typing those
characters in an email body, reply, or search box behaves normally. `Cmd+Shift+A`, `Cmd+,`,
`Ctrl+Shift+A`, and `Ctrl+,` work even while a text field is focused, the same as `Cmd+K`. The
command palette (`Cmd+K`) and shortcut help panel (`Shift+?`) are available on the Command Center
page; search-box focus (in the email list's filter bar) is mouse/tap-only.

This table is verified against the app's actual registered shortcuts by
`bash scripts/audit/extract-shortcuts.sh`, which prints every shortcut key the frontend actually
dispatches (grounded in `frontend/apps/web/src/shared/hooks/useKeyboard.ts`), so it can be
checked instead of trusted.

## Tips and Best Practices

### Onboarding

When you launch Emailibrium for the first time, a guided onboarding flow walks you through six steps:

1. **Welcome** -- introduces the application, shows server health status, and lets you choose an email provider (Gmail, Outlook, IMAP). You can also skip email setup to explore the app first.
2. **Connect** -- authenticates with your chosen email provider via OAuth (Gmail/Outlook) or direct credentials (IMAP).
3. **Connected Accounts** -- shows your connected accounts with the option to add more providers.
4. **AI Setup** -- shows available AI tiers. Local AI (ONNX) is ready by default with no setup needed. Optionally configure Ollama for enhanced local LLM features, or add cloud API keys (OpenAI, Anthropic, Gemini) for advanced capabilities.
5. **Archive Strategy** -- choose how Emailibrium handles archiving: immediate, delayed, or manual.
6. **Setup Complete** -- summary of your configuration with green/amber status indicators for each component. Launch the app or go to Settings to adjust.

You can return to onboarding settings at any time via Settings.

> **Developer setup:** Run `just setup` on the command line for a guided wizard that configures prerequisites, secrets, OAuth apps, and AI providers before launching the app. See the [Setup Guide](setup-guide.md).

### Chat

The Chat feature provides a conversational interface for interacting with your email:

- Ask questions about your email in natural language (e.g., "What meetings do I have this week?")
- Results are powered by semantic search and, when configured, generative AI
- The chat panel is accessible from the sidebar

Chat requires a generative AI provider (Ollama or cloud). Without one, the chat interface is available but responses are limited to search results without synthesis.

### Achieving Inbox Zero Quickly

1. Start with the **Inbox Cleaner** -- it groups emails for batch processing
2. Process the largest category groups first (newsletters, marketing)
3. Use **Archive** liberally -- archived emails are still searchable
4. Set up **Rules** for recurring patterns to prevent inbox buildup
5. Check the **Insights** report weekly to catch new subscriptions

### Improving Search Quality

- The system learns from your interactions. Click on relevant results to help train it.
- Use the feedback buttons (thumbs up/down) on search results when they appear
- More specific queries yield better results than single-word searches
- If initial results are poor, try rephrasing -- semantic search understands intent, not just keywords

### Privacy Considerations

- All processing happens locally by default
- Enable AES-256-GCM encryption at rest via `backend/config.yaml`'s `encryption.enabled` /
  `master_password` keys if your device may be accessed by others -- this isn't yet configurable
  from the Settings UI (see the note under Settings > Privacy above)
- The Settings > Privacy audit log is currently a static example, not live activity data
