# Mail Agent — Overall Plan (Oct 2026)

Oct 26, 2026 · @Chris Phillipson

Companion visual: the [Emailibrium explorer](https://claude.ai/artifact/61ePiwwxc8D3WGYcPZh3yA) — system map, chief-of-staff loop, stalled-work portfolio and the decisions sheet. The calls made there are stored with the artifact and mirrored in section 10 of this plan.

## 1. Executive summary and strategy

Build one local agent that acts as a chief of staff for email: it connects to any mailbox over IMAP with the credentials Apple Mail uses, learns each account's shape from headers alone, brings what needs you to your attention, proposes better organization, reduces noise week over week, and helps you close the inbox to zero at the end of each day or session. It reports and suggests first, files only with your approval, and never writes an email.

Three live accounts were profiled on 26 Oct 2026 (section 2). Together they hold \~117,000 messages, 88% of inbox mail unread, and three abandoned taxonomies each. None of the earlier builds (Oct 12 Gmail API + Microsoft Graph sorters) produced a usable result because they required cloud-console setup and vendor APIs; this plan replaces both.

**Six decisions that shape everything**

1. **IMAP, not vendor APIs.** App passwords for Google personal and Workspace, iCloud, Yahoo and Fastmail; device-code with a public client for Outlook (beta); Proton via Bridge. No Google Cloud project, no Azure registration. Header reads cost no API quota, and Gmail labels are written over IMAP with `X-GM-LABELS`, which has no quota at all.
2. **Secrets never enter the model.** The MCP server collects credentials through a one-shot localhost form and stores them in the OS keychain; the model only ever sees "connected". URL-mode elicitation is used where the client supports it (Claude Code CLI today).
3. **Three label namespaces, five Kinds.** `Kind/` (People, Subscriptions, Notifications, Receipts, Other), `Topic/` (8–12 per account, earned by volume), `State/` (user-set only). Sender key = local-part + domain, never domain alone.
4. **Role declared, archetype detected, playbook proposed, user confirms.** One tap for role, a header snapshot, one of four playbooks, 2–4 options to choose from. The agent never invents per-mailbox logic.
5. **Plan → approve → apply → undo, oldest era first, never delete.** Every write is a hashed plan the user confirms; every apply is journaled and reversible; deletion exists only as a per-rule opt-in, never as a sweep.
6. **Report first, act later, never send.** Every capability starts as a report or a suggestion; filing and archiving graduate to auto-apply per rule only after repeated approval; drafting, sending, replying and unsubscribing on the user's behalf are out of scope until explicitly revisited.

**What success looks like (90 days)**

| Measure | Target |
| --- | --- |
| Time from email address to mailbox snapshot | under 2 minutes |
| Inbox items needing a person, per day, after flow rules | 1–5 per account |
| Inbox at zero at the end of each day or session | ≥ 90% of days, with one approved close-out |
| Bulk mail reaching the inbox (noise), week over week | falling for 8 consecutive weeks, then flat near zero |
| Weekly review | under 10 minutes, every item yes/no |
| Writes without an approved plan or a graduated rule | zero |
| Emails created by the agent | zero |
| Undo success on any applied plan | 100% |
| Accounts covered | 3 (Gmail personal, Gmail professional, Workspace business), then Proton and Outlook |

**Strategy.** Build for the three accounts first, prove the detector and the ledger on real mail, then publish as an open-source MCP server plus Claude Code skills for clients (teams of 1–50, including non-developers). The business case for clients is the same one measured here: most inbox volume is machine mail that no person needs to see, and the fix is half settings, half routing.

## 2. Evidence from three live accounts

The detector would have mis-read two of the three accounts from their addresses alone; the role question and the header snapshot together got all three right. Label totals below are exact (Gmail label counts); inflow and sender shares are search estimates and samples taken 26 Oct 2026.

|  | chris@cpventuresgroup.com | christopher.a.phillipson@gmail.com | fastnsilver@gmail.com |
| --- | --- | --- | --- |
| Declared role | Business | Professional | Personal |
| Measured archetype | Machine-dominated identity | Mixed: developer sink + professional network + household admin | Primary life account + receipts hub |
| Messages (approx.) | 4,300 | 32,000 | 80,400 |
| History | > 1 year | back to 2011 | back to the 2000s (≥ 10,000 threads older than 15 yr) |
| Inbox messages / unread | 2,702 / 81% | 27,373 / 86% | 41,004 / 89% |
| Sent, lifetime | 70 | 1,236 | 2,395 |
| Inflow, last 30 days | \~17 threads/day | \~47 threads/day | \~49 threads/day |
| Dominant inflow | Circle notifications 22%, marketing drips | GitHub 62% (423 CI/push noise of 867), LinkedIn 11% | Promotions 31% of all mail; Amazon returns/orders in bursts |
| Human share | \~2% (\~10 correspondents) | moderate: recruiters + household | largest: family, household, community, travel |
| Gmail categories | Updates 59 · Promo 29 · Primary 10 | Updates 68 · Promo 12 · Primary 13 · Forums 5 | Updates 56 · Promo 31 · Primary 13 |
| Taxonomy generations | GTD (empty) · flat topics · Topic/State/Org on \~80 msgs | Newsletters 16,773 (also catching GitHub) · Developer 7,455 · Career 3,214 · triple-labeled 143 | 45 per-vendor `INBOX/Notifications/<Vendor>` folders ≈ 7,800 msgs (10%) · `INBOX/Archive` 15,390 · experiment labels ≤ 92 |
| Playbook | Archive all but humans + protected; source-settings checklist; digest | GitHub at source + reason routing; household protected; career clock; era plan | Receipts as a Kind; unsubscribe audit; family protected; deep-archive era first |

**What each account changed in the design**

- **Business:** a label covering 70% of mail (`Newsletters`) carries no information; roughly a third of it was notifications, not newsletters. Same domain, different Kind by local-part (a bank's `account@` vs `webinar@`). A paid program looked like a newsletter (`State/Enrolled`). Security alerts must be protected. Senders drift month to month, so the ledger keeps first/last seen.
- **Professional:** GitHub encodes the notification reason in the cc address (`mention@`, `review_requested@`, `ci_activity@`, `push@`…) and the repo in the To address, so routing is by reason, not sender. A classifier had auto-applied `State/ReadLater` to 143 messages; State is user-set only. Golf, entertainment, family and school mail live on the professional address — a conscious choice to surface, not a move to make.
- **Personal:** 45 hand-made vendor folders stalled at 10% coverage — the strongest evidence that per-sender folders don't scale and that the medium (8–12 topics) is right. Amazon's local-parts (`order-update@`, `shipment-tracking@`, `return@`, `auto-confirm@`, `payments-messages@`) encode the receipt event. Senders in sensitive categories exist here; the agent files them under generic topics, never names them in digests, and never proposes unsubscribing from them.

**Cross-account facts.** The same community organisation, the same housing company and the same family members appear in two accounts' sent mail; the personal account relays to the business and a former employer's address. One People graph and one identity map across accounts are therefore requirements, not extras.

## 3. Product requirements

The product is a local mail agent that any Claude surface (Code, Desktop, Cowork) can drive through a small set of commands, with no cloud setup and no message ever moved without approval.

**Users**

- Primary: Chris, three Gmail/Workspace accounts, Claude Code and Desktop on macOS.
- Secondary: clients of CP Ventures Group (startups and small businesses, teams of 1–50) who may not be developers, on Gmail, Microsoft 365, iCloud, Fastmail or Proton.

**Jobs to be done**

1. Connect a mailbox in under two minutes with the credentials I already use elsewhere.
2. Show me what my mailbox is made of before anything changes.
3. Bring to my attention only what needs me, in priority order, with the reason.
4. Suggest better organization and let me try it with one tap and undo.
5. Reduce the noise reaching my inbox over time, at the source where possible.
6. Close my inbox to zero at the end of each day or session, with every item given a disposition I approve.
7. Keep it that way with one brief a day and one ten-minute review a week.
8. Work across several accounts as one system, without merging them.

**In scope for v1**

- Providers: Gmail and Google Workspace (app password), iCloud, Fastmail, Yahoo (app password). Proton (Bridge) in v1.1. Outlook.com and Microsoft 365 (device-code, public client) as beta.
- Reads: headers only, never bodies; From, To, Cc, Date, Subject, Message-ID, List-*, Precedence, Gmail X-GM-* attributes.
- Writes: labels (Gmail, Proton, keywords where supported), moves to folders, archive (remove INBOX), mark read. Never delete, trash, send, forward or reply.
- Detection: role + snapshot + archetype; playbooks; sender rules; era plan.
- Outputs: snapshot, plan preview, doctor report, daily digest, weekly review, monthly health report, notification-settings checklist, identity map.
- Surfaces: MCP server (stdio) + Claude Code skill and slash commands; works in Desktop and Cowork through the same server.

**Non-goals for v1**

- Creating email of any kind: no sending, no drafts, no replies, no forwards, no digest emails. Briefs and reviews are command output, a Claude page or a local file.
- Reading or summarising message bodies.
- Unsubscribing on the user's behalf: **superseded 27 Oct, see §10/§11** — RFC 8058 one-click POST unsubscribe is now in scope (DKIM/DMARC-gated, per-sender Apply once/Always approval); `mailto:` and plain links are still handed to Gmail's and Outlook's built-ins, and link clicking is never automated. This bullet was left unedited when that later decision was made; reconciled 27 Oct (verification pass).
- Changing provider-side notification settings for the user (Circle, Slack, GitHub); the agent produces the ranked checklist.
- Acting without approval, except for rules the user has graduated (section 3, Operating model), and then only filing and archiving.
- A hosted service, telemetry or any cloud relay.
- Classification models beyond header rules and Claude-in-the-loop judgments (Jev and Laya are excluded by decision).

**Principles**

- Secrets never pass through the model.
- Headers decide most things; a judgment call is per unique sender, never per message.
- The user confirms; the agent proposes. Approvals are per plan hash.
- Everything is reversible except what the user explicitly deletes in the monthly review.
- Labels exist for disposition and routing, not retrieval; search does retrieval ([Whittaker et al., CHI 2011](https://dl.acm.org/doi/10.1145/1978942.1979457)).
- One vocabulary, per-account activation; one People graph across accounts.

**Operating model: chief of staff**

The agent behaves like a chief of staff: it watches everything, tells you only what needs you, proposes the plan, and acts only within the authority you have granted. Authority is granted per rule, not per agent, and it starts at zero.

| Level | Name | What the agent does | How a rule gets here |
| --- | --- | --- | --- |
| 0 | Observe | snapshot, doctor, attention brief, reports; nothing written | every new account and every new sender starts here |
| 1 | Suggest | plans, close-outs and organization proposals, each applied by one tap and reversible | the default for everything after onboarding |
| 2 | Trusted rule | filing or archiving for one sender rule applied automatically, summarised in the Monday review, always reversible | approved with Always at any suggestion, or the same Apply-once suggestion approved on 3 consecutive Mondays with zero corrections; never for People, protected senders, enrolled programs or unsubscribes |
| 3 | Act on your behalf | drafting, sending, replying, unsubscribing | not planned; requires an explicit future decision |

Three chief-of-staff duties follow from this:

- **Attention.** An ordered brief of what needs you, with the reason and the clock: open loops past their SLA, review requests and mentions, enrolled-program mail, security and account alerts, new humans. Everything else is a count.
- **Organization proposals.** Suggestions with expected effect and an undo path, never applied silently: "replace 45 vendor folders with Receipts plus search", "Golf and Entertainment belong on the personal address", "merge Learning and Topic/Learning".
- **Noise reduction over time.** A weekly noise figure (bulk threads reaching the inbox per day) with the three levers that would cut it most: a settings change at the source, a filter gap, or an unsubscribe candidate. The target is a falling line, not a one-time purge.

**Success metrics**

| Metric | v1 target | How measured |
| --- | --- | --- |
| Connect to snapshot | < 2 min | timer in `doctor` |
| Header scan throughput | ≥ 1,000 msgs/s on Gmail | scan log |
| Rules coverage after onboarding | ≥ 90% of inflow matched by a sender rule | ledger |
| Judgment calls per account | ≤ 50 at onboarding, ≤ 10/week after | ledger |
| Inbox items needing a person | 1–5/day | digest |
| Unapproved writes | 0 | journal audit |
| Undo | 100% restores prior labels | test suite + live spot check |

## 4. Architecture

[pacphi/emailibrium](https://github.com/pacphi/emailibrium) (v0.2.0, MIT; Rust Axum backend, React SPA, local ruvector index) is the engine, and the chief of staff is an authority-gated layer added to it — not a new server. Revised 27 Oct: everything the first draft proposed to build in TypeScript either already exists in emailibrium (IMAP + app-password onboarding, ingestion, rules engine, cleanup plan/apply with dry run and undo, a 15-tool read-only MCP server) or becomes a backend addition there.

**Components (what calls what)**

1. **Claude surfaces** (Code, Desktop, Cowork) → call emailibrium's MCP server (stdio and HTTP transports exist); the chief-of-staff skill and slash commands (`/mail brief`, `/mail closeout`, `/mail review`, `/mail suggest`, `/mail settings`, `/mail map`, `/mail doctor`) are thin prompts over those tools and live in an `agent/` directory of the repo.
2. **Emailibrium backend, as it stands** → accounts (IMAP + app password through its own onboarding UI; OAuth path for Workspace/Outlook), ingestion and sync state, local embeddings and clustering, rules engine (ADR-014), cleanup orchestrator with plan builder, canonical plan hash, dry run, SSE progress and undo buffer (ADR-010, ADR-030), label and folder operations per provider (ADR-018), and the read-only MCP tools in `backend/src/tools/readonly` (`list_subscriptions`, `preview_cleanup_plan`, `get_insights`, `count_emails`, `list_rules`, `list_accounts`…).
3. **New: chief-of-staff tool set** in `backend/src/tools/staff/` → `get_snapshot`, `brief`, `closeout_preview` / `closeout_apply`, `plan_apply` / `plan_undo` (over the existing orchestrator), `review`, `health`, `identity_map`, `rule_set_authority` (Apply once / Always / graduate). Write tools are enabled by an explicit authority config; send and compose stay REST-only and are never registered as MCP tools, so "never creates email" is a property of the MCP surface, not a prompt.
4. **New: deterministic Kind classifier and sender key** → header rules (section 7) ahead of the semantic classifier in `cleanup/domain/classifier`; subscription detection keyed by `local-part@registrable-domain`; `Kind/`, `Topic/`, `State/` namespaces in the ADR-018 label strategy; era-based archive strategies in the plan builder beside today's `olderThan30d/90d/1y/custom`.
5. **New: role, archetype and playbook** on the account model, driving the snapshot and the playbook proposal; `State` writable only through user actions.
6. **Provider deltas** → Google Workspace joins the app-password fast path (Google's transition page keeps app passwords as the exception; the current QUICKSTART routes Workspace to OAuth); Outlook gains a device-code flow with a configurable public client ID (beta) so the Azure registration becomes optional; Proton Bridge becomes an onboarding preset.
7. **Credential capture** → already outside the model's path: emailibrium's onboarding page at `localhost:3000` collects the app password and encrypts it at rest (AES-256-GCM, Argon2id). The MCP `account_add` tool only opens that URL (URL-mode elicitation where the client supports it).
8. **Privacy profile for the agent** → the chief-of-staff tool allowlist excludes body-returning tools (`get_email`, `get_email_thread` bodies, `search_emails` snippets); emailibrium keeps embedding bodies locally for its own search, and the model sees headers and counts only.
9. **Ledger** → emailibrium's SQLite (sqlx) is the single ledger; additions are sender first/last seen and reply flags, rule authority level and graduation history, account role, and a close-out journal. No second database.
10. **Writes** → IMAP-connected Gmail accounts write labels over IMAP (no API quota); the OAuth path uses the Gmail API with the ADR-010 rate limits. Plans run per era with resume; every chunk is journaled for undo.

**MCP tools**

| Chief-of-staff tool | Emailibrium today | Work |
| --- | --- | --- |
| `account_add` / `list_accounts` | onboarding UI + `list_accounts` | wrap: open onboarding URL; add role |
| `doctor` | `get_sync_status`, `list_accounts` | new: capabilities, label strategy, scan estimate |
| `get_snapshot` | `get_insights`, `count_emails`, `list_subscriptions` | new: archetype, role check, human share, eras |
| `brief` | — | new: attention queue with SLAs, boomerang |
| `closeout_preview` / `closeout_apply` | `preview_cleanup_plan` (ephemeral), cleanup orchestrator | new tool over existing plan/apply |
| `plan_preview` / `plan_apply` / `plan_undo` | REST plan/apply/undo (ADR-030) | expose as gated MCP tools |
| `review` / `health` / `identity_map` | `get_insights`, `list_rules`, `get_learning_metrics` | new reports |
| `rule_set_authority` | rules engine (ADR-014) | new: authority level, graduation, corrections |
| `rules_export` | — | new: Gmail filter XML / provider rules |
| send, compose, reply | REST only | never registered as MCP tools |

**Data model (SQLite)**

```sql
-- additions to emailibrium's existing schema (sqlx migrations)
connected_accounts  + role, archetype, playbook, declared_at
senders             + key (local-part@domain), first_seen, last_seen, replied_to, opened_90d, protected, sensitive
rules               + kind, topic, authority (observe|suggest|trusted), approvals_json, corrections, graduated_at, retention_days
closeout_journal    (id, account_id, at, items_json, plan_id, undone_at)
emails              + kind, topic, era   -- derived, indexed; state stays user-written
```

`from_key` is `local-part@registrable-domain` with the public suffix list applied; `msgid` is the RFC 822 Message-ID for cross-folder dedupe (Proton), and `X-GM-MSGID` on Gmail.

**Sync model.** Snapshot records `UIDVALIDITY`/`UIDNEXT` per folder (and `HIGHESTMODSEQ` where CONDSTORE exists). Incremental scans fetch `UID > last`; Gmail also tracks `X-GM-LABELS` changes via CONDSTORE. Before each apply chunk, changed UIDs since snapshot are re-planned.

**What this changes for emailibrium.** Three corrections to the repo as it stands: Workspace accounts belong on the app-password path (the QUICKSTART's "disabled May 2025" claim is contradicted by Google's own transition page); Outlook gets a public-client device-code option so the Azure registration is no longer required for personal accounts; and the MCP server gains a gated write tool set while send and compose stay off it. The chief of staff is therefore an adjunct in the literal sense — an `agent/` directory of skills and commands plus a `tools/staff` module — and the Oct 12 TypeScript sorter is retired, its planner, ledger and undo ideas already present in ADR-030.

**What the repo history says (read 27 Oct: main, develop, two unmerged branches)**

| When | What happened | What it means for the blend |
| --- | --- | --- |
| Mar 2026 (172 commits) | Inception as a vector-native email platform; Tauri desktop abandoned for a web SPA (ADR-005) | the SPA is the cockpit; nothing else should own the UI |
| Apr–May | Cleanup dry run, plan hash, SSE, undo shipped (ADR-030); v0.2.0 on 12 May | the plan/apply/undo core already exists and is tested |
| 15 Jun | IMAP + app-password "15-minute no-OAuth quick start" (#150) | the alternate path exists but is framed as a shortcut; Workspace is wrongly routed to OAuth |
| 18 Jun (unmerged) | OAuth dev-setup docs: console credentials, consent screen, unverified-app warning, restricted-scope verification for `gmail.modify` | OAuth cannot be made delightful for a self-hosted single-user tool; it belongs under "Advanced" |
| 31 Jul | Unified MCP tool registry with `--mcp-stdio` on the main binary (#203) | the agent door already opens in-process, no HTTP, no second auth |
| Aug | Autopilot pipelines: Postgres/SeaORM (158 files, +15k lines), CI, docs audit; MCP audit queued "security-baseline ahead of everything" | large orthogonal work consumed the momentum; the user promise was not the target |
| 22 Sep (develop only) | Stability and model-integration audits: not ready for private, unattended filing of 150k messages; P1s in cleanup pagination, missing-provider success, consent revocation, classifier output; same day: engine auth, encrypted credentials, local sessions, origin guard, fail-closed inference policy | LLM-in-the-loop filing is the trust problem; the hardening lives only on develop |
| 22 Sep (develop only) | `backend/src/triage`: deterministic admission and planning for approved triage, "never calls a model or provider", `Attention` → `State/…` labels, `Organization::Approved\|Individual\|Held`, revision-bound `MessageFacts`, `FilingPlan` — with no callers anywhere | the chief-of-staff decision core was built and never wired; wire it |
| Oct | Dependencies only; main unchanged since August; develop 45 ahead (verified 27 Oct, corrected from an earlier count of 51) | main is what people clone and it lacks the security fixes |

**Two front doors, one engine (recommendation)**

1. Keep the SPA onboarding flow and flip its default: `ImapConnect` becomes the primary path for Gmail, Workspace, iCloud, Fastmail, Yahoo and Zoho; `GmailConnect`/`OutlookConnect` move under "Advanced: bring your own OAuth client"; Outlook gains device code with a public client (beta). The role question is added to `ProviderSelector`.
2. The agent's `/mail connect` opens that same onboarding page with the address prefilled, and the snapshot comes back into the chat. One credential UI, two entry points; the model never sees a secret either way.
3. Wire `triage::Policy` as the single decision core for both doors. Header Kind/Topic rules feed `Organization::Approved(topic)`; `Attention` is the `State/` axis; `Held` is a pending judgment; the close-out is a `FilingPlan` over inbox `MessageFacts`. The semantic classifier stays for search, chat and judgment batches, never for filing.
4. One authority config in the backend, shown identically in Rules Studio ("Apply once / Always", graduation) and in the agent; `ArchiveStrategyPicker`'s Instant/Delayed/Manual becomes the authority level. MCP write tools are gated by it; send and compose are never registered.
5. Every agent suggestion links to the SPA's existing plan-review screen; every SPA apply is visible to the agent through the same ledger. The SPA home becomes the brief plus a close-out button.
6. Headless mode: `emailibrium --mcp-stdio` starts engine and ingestion without the SPA; the connect page is served by the backend so adding an account never requires the full app.
7. Trunk hygiene before any of this: merge develop into main (the security fixes), tag v0.3.0 with "chief of staff" as the theme, and put Postgres, multi-tenancy and the built-in LLM behind feature flags.

**Model providers and the agent (decided 27 Oct)**

The chief of staff uses a model in exactly two places, and neither is the filing loop: judgment batches for senders the header rules leave as `Held` (the output is a proposed rule, which the user approves), and search, chat and topic naming in the SPA. Everything from headers to plan to apply to undo is deterministic, so provider choice changes judgment quality and privacy posture, never safety. Emailibrium's tiered provider architecture (ADR-012) carries over unchanged.

| Tier | What it provides | Privacy | Status |
| --- | --- | --- | --- |
| Rules only (no generative model) | the full loop; `Held` senders wait for the Monday review where the user decides | nothing leaves the machine | default; the `onnx` tier in AISetup already describes it |
| Local embeddings (ONNX) | semantic search and similar-sender hints | nothing leaves the machine | shipped |
| Local generative via Ollama (3–8B instruct) | judgment batches, topic naming, chat | nothing leaves the machine; treated as local by the policy gate | shipped (`ollama.base_url`, default `http://localhost:11434`); classification-model selection fixed 22 Sep |
| Local generative via LM Studio | same as Ollama, for people who manage models in LM Studio | nothing leaves the machine; must be classified local by the policy gate, not as an OpenAI-compatible cloud | scheduled (new, milestone 5): a provider kind over the existing OpenAI-compatible client with `base_url: http://localhost:1234/v1` and no API key; model discovery from `/v1/models`; structured output via `response_format` JSON schema; endpoint details verified at implementation |
| Cloud (Anthropic, OpenAI, Gemini, OpenRouter) | higher judgment quality, chat | header-only payload for staff work; consent required; fail-closed policy gate (audit P1 follow-up) must be complete first | shipped behind consent |
| Claude as the chat surface (Code, Desktop, Cowork) | drives the conversation and calls the staff tools | sees only tool outputs — headers and counts — whatever model is configured inside the engine | by design |

Rules that make this hold across providers: one judgment payload contract (sender key, counts, three subjects, candidate topics — never bodies) used identically by every provider; schema-constrained output where the provider supports it (Ollama structured output, Anthropic/OpenAI/Gemini structured outputs) and strict validation with retry elsewhere, with an invalid answer leaving the sender `Held` rather than guessing (the audit's P2 lesson); model name and version recorded in the ledger for every judgment (the provenance the audit found missing); and a `doctor` check that runs a small golden judgment set against the configured provider and disables judgment, falling back to rules-only, below a threshold. Built-in llama.cpp is pruned; Ollama is the local generative path.

## 5. Authentication and security

Every provider is reached with credentials the user already knows how to make, and no secret is ever a tool argument, a tool result or a chat message.

**Per-provider auth paths (verified Oct 2026)**

| Provider | Path | Prerequisite | Status |
| --- | --- | --- | --- |
| Gmail, Google Workspace | 16-char app password, IMAP `LOGIN` | 2-Step Verification on; Workspace admin may block app passwords or enforce security keys | v1 ([Google: app passwords remain the exception](https://knowledge.workspace.google.com/admin/sync/transition-from-less-secure-apps-to-oauth), [how-to](https://support.google.com/accounts/answer/185833)) |
| iCloud | app-specific password | two-factor on; username is usually the name part of the address | v1 ([Apple](https://support.apple.com/en-us/102654)) |
| Fastmail | app password scoped to Mail (IMAP/POP/SMTP) | Basic plan has no IMAP | v1 ([Fastmail](https://www.fastmail.help/hc/en-us/articles/360058752854-App-passwords)) |
| Yahoo | app password | browser signed in for several days | v1 ([Yahoo](https://help.yahoo.com/kb/sln15241.html)) |
| Proton | Proton Mail Bridge on 127.0.0.1 (default 1143), Bridge-generated password | paid plan; labels appear as `Labels/`, copy = add label | v1.1 ([Proton](https://proton.me/hi/support/labels-in-bridge)) |
| Outlook.com, Microsoft 365 | OAuth device code with a public desktop-client ID (Thunderbird pattern), then XOAUTH2 | basic auth ended for all Outlook accounts on 16 Sep 2024; IMAP must be enabled in Outlook settings | beta ([Microsoft](https://support.microsoft.com/en-US/support/known-issues/modern-authentication-methods-now-needed-to-continue-syncing-outlook-email-in-non-microsoft-email-ap), [Thunderbird 2026 KB](https://support.mozilla.org/en-US/kb/microsoft-oauth-authentication-and-thunderbird-202), [public client ID practice](https://ficd.sh/blog/outlook-school-email-ouath/)) |

**Provider detection** (user types only the address): known-domain table → [Thunderbird ISPDB](https://developer.mozilla.org/en-US/docs/Mozilla/Thunderbird/Autoconfiguration) `autoconfig.thunderbird.net/v1.1/<domain>` → MX record mapped to Google (`*.google.com`), Microsoft (`*.mail.protection.outlook.com`), Fastmail (`messagingengine.com`), Proton (`mail.protonmail.ch`), iCloud → RFC 6186 SRV → guess `imap.<domain>` → ask.

**Credential capture**

1. The server starts a listener on `127.0.0.1:<random>` with a single-use 128-bit nonce in the path, 10-minute TTL, CSRF token on POST, and opens the browser (`open` / `xdg-open` / `start`).
2. The page shows the detected provider, a deep link to the provider's app-password page (`https://myaccount.google.com/apppasswords`, `https://account.apple.com`, `https://app.fastmail.com/settings/security`), and one password field.
3. On submit the server test-logs-in over IMAP, stores the secret in the OS keychain (Keychain Services, Credential Manager, Secret Service; `0600` file fallback with a warning), shuts the listener, and returns only `{status:"connected", address, authMethod}`.
4. Where the client supports MCP URL-mode elicitation, the same URL is offered through it; the spec requires URL mode for secrets and forbids form mode for them ([MCP elicitation](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation)). Claude Code CLI renders it ([docs](https://code.claude.com/docs/en/mcp)); Claude Desktop and Cowork had open issues in which elicitations hang or auto-cancel ([#88901](https://github.com/anthropics/claude-code/issues/88901), [#56243](https://github.com/anthropics/claude-code/issues/56243)), so the server-opened form is the default and elicitation is feature-detected with a short timeout.
5. Outlook device code: show the code large and copyable, open the verification URL, start polling immediately, honour `interval` and `slow_down`, stop at `expires_in`, cache the refresh token in the keychain, and explain that Microsoft's consent screen names the public client.

**Threat model and least privilege**

- An app password grants full mailbox read/write and (except Fastmail) SMTP; no provider offers a read-only IMAP app password.
- Mitigations in code: no SMTP module in v1; no `EXPUNGE` or `\Deleted`; `EXAMINE` for all scans; header-only `BODY.PEEK`; ledger and plans `0600`; no telemetry; no network except the IMAP hosts and ISPDB.
- Disclosure shown before the form (two sentences): the password lets the tool read and organise mail like Apple Mail would; it stays on this computer, never sends or deletes, and can be revoked at the provider at any time.
- Revocation links are shown at the end of onboarding and in every monthly report; `account_remove` deletes the keychain item and the ledger.
- The agent never names a sender from a sensitive category (health, legal, finance balances) in a digest, review or chat; those are filed under generic topics and excluded from unsubscribe candidates.

**Honest limits.** Workspace tenants that disable app passwords cannot connect without an OAuth client. Outlook depends on a borrowed public client that Microsoft can restrict without notice; the client ID is configurable so a business can register its own. Yahoo's app-password eligibility check is opaque.

**Emailibrium deltas (27 Oct).** Credential capture already happens in emailibrium's onboarding UI, never through chat, so the localhost-form design above describes what exists; the remaining work is the two provider corrections (Workspace on app passwords; Outlook device code with a configurable public client) and the authority-gated MCP write surface that never includes send or compose.

## 6. Onboarding and UX

Onboarding asks two questions (address, role), does the rest itself, and shows a mailbox snapshot within two minutes before proposing anything. Every later interaction is a short list of yes/no choices.

**`/mail connect` — the exact sequence**

1. "What's your email address?" (the only free-text question)
2. "What is this address for? Personal · Professional · Business · Shared · Retired" — role sets playbook defaults, protected list, privacy posture and retention.
3. Detect provider; say which and that the connection is read-only. Options: Continue / That's not my provider.
4. One prerequisite question per provider ("Is 2-Step Verification on?" with a link to turn it on).
5. Open the provider's app-password page and the private local form; wait, with a visible countdown.
6. "Connected ✓. Scanning headers only — nothing will be marked read." Progress by folder.
7. Snapshot (section 8 format), then the role check: "You said professional; 62% of your inflow is GitHub and your sent mail is half household. Treat as Mixed?" Options: Yes / Keep professional / Explain.
8. 2–4 next steps as selectable options, recommended one first, always including "Just keep it connected".

**Four playbooks and what picks them**

| Playbook | Picked when | First move | Cadence |
| --- | --- | --- | --- |
| Machine-dominated identity | humans < 5% of threads, inflow > 10/day | archive all but humans and protected; source-settings checklist | daily digest, Monday review |
| Primary life account | humans > 15%, history > 5 yr, receipts > 10% | backup, then oldest era first; Receipts as a Kind | daily, weekly, monthly retention offer |
| Professional network | GitHub/CI or recruiter share high, moderate humans | GitHub at source, reason routing, career clock | daily (review requests, recruiter SLA), weekly |
| Legacy / signup sink | inflow < 3/day, decaying curve, identity share high | service inventory, unsubscribe sweep | monthly only |

Mixed accounts get a playbook per era. When the snapshot cannot decide, the agent asks the one question that resolves it rather than guessing.

**Commands**

| Command | Does | Writes |
| --- | --- | --- |
| `/mail connect <address>` | onboarding above | keychain |
| `/mail doctor [account]` | auth, capabilities, label strategy, counts, estimates | no |
| `/mail snapshot [account]` | shape, archetype, role check, top senders | no |
| `/mail brief` | the attention brief: what needs you, in order, with reasons; counts of everything else | no |
| `/mail closeout` | every inbox item with a suggested disposition; one tap applies the close-out, leaving the inbox at zero | mailbox, on approval |
| `/mail plan <era\|playbook>` | hashed plan with counts and 3 samples per bucket | ledger |
| `/mail apply <hash>` | execute, journal, resume | mailbox |
| `/mail undo <hash>` | reverse | mailbox |
| `/mail review` | the Monday list, including rule graduations and the noise trend | ledger on approval |
| `/mail suggest` | organization proposals with expected effect and undo | no |
| `/mail settings` | notification-settings checklist ranked by volume | no |
| `/mail map` | identity map across accounts | no |

**Dialogue (the agent's own words, abridged)**

> "Hi — I'll help get your inbox under control and I'll never move, send or delete anything without showing you first. What's your email address?" "Got it: Gmail. I need a special app password from Google (not your normal one). I'm opening Google's page and a private form on your computer. Paste the password into the form, not here, so I never see it." "Connected ✓ (read-only). Taking a quick look at headers only…" "Here's your mailbox: 80,400 messages, 41,004 in the inbox, 89% unread, about 49 new threads a day. Most are receipts and promotions. What would help most?" \[Plan an inbox reset — preview only\] \[Audit subscriptions\] \[Find my important people\] \[Nothing yet\] "Before I change anything, here's the plan: 2,079 threads older than 5 years → archived with their Kind, marked read. Three examples from each bucket are below. Reply apply 7f3a to go ahead; /mail undo 7f3a reverses it."

**Delight details worth the effort**

- Address-only auto-detect; deep links that open the exact page; the secret form never touches chat.
- Snapshot before any question; three samples per bucket in every plan; one-command undo; the revocation link at the end.
- Bursts collapsed to one line ("SkilVault: 5 account emails in 5 minutes"); mentions and review requests linked directly.
- Progress and counts in every long operation; nothing silent.

**Screens added or changed for the chief of staff (27 Oct)**

| Screen | Route | Change | Depends on | Size |
| --- | --- | --- | --- | --- |
| Onboarding | `/onboarding` | role question in `ProviderSelector`; `ImapConnect` first with provider detection from the address; OAuth under Advanced; Workspace preset | `oauth.enabled` flag; account role | S |
| Home becomes the brief | `/` | attention queue ordered by SLA, boomeranged loops, counts filed, nudges; new `features/brief` | `brief` staff tool | M |
| Close-out | from the brief, or `/closeout` | every inbox item with its disposition, one approve, undo; reuses ADR-030 review components | `closeout_preview` / `closeout_apply` | M |
| Rules Studio | `/rules` | authority column (Observe / Suggest / Trusted), Apply once / Always, graduation history, corrections, retention per rule, Kind/Topic/State fields | `rule_set_authority` | M |
| Inbox Cleaner step 3 | `/inbox-cleaner` | Senders and Kinds replaces the clusters step: sender groups with suggested Kind/Topic, protected toggles, three samples each | `senders_group`; `sender_actions` in the plan builder | M |
| Archive strategy picker | onboarding and settings | becomes the authority picker | authority config | S |
| Monday review | `/review` | the yes/no list: new senders, unsubscribe candidates, open loops, strays, enrolled check, rules eligible to graduate, noise trend with three levers | `review` staff tool | M |
| Insights | `/insights` | noise-trend line, inbox-zero days, nudges accepted; identity-map table | `health`, `identity_map` | S–M |
| Settings | `/settings` | authority config, protected and sensitive sender lists, revocation links; AISetup simplified with `onnx` as the default tier and Ollama / LM Studio beside it | provider config | S |
| Plan review deep link | `/cleanup/history/$planId` | already exists; “Review in Emailibrium” from chat costs nothing | — | — |
| Headless connect page | served by the backend | the onboarding page without the full app, for `/mail connect` | build tooling | M |

The [explorer](https://claude.ai/artifact/61ePiwwxc8D3WGYcPZh3yA) shows where each of these sits in the loop.

## 7. Classification and taxonomy

Headers decide Kind deterministically for most mail; a Claude judgment is made once per unique sender for the rest; Topic labels are earned by volume; State is written only by the user.

**Three namespaces**

| Namespace | Values | Who sets it | Purpose |
| --- | --- | --- | --- |
| `Kind/` | People · Subscriptions · Notifications · Receipts · Other | header rules | disposition: inbox vs filed, retention, digest line |
| `Topic/` | 8–12 per account from a shared vocabulary (Community, Tools, OpenSource, Tech & Industry, Marketing & Creator, Learning, Finance, Career, Clients, Household, Travel, Shopping, Entertainment…) | sender rule, with judgment where needed | search and sidebar; per-account activation by gate |
| `State/` | Enrolled · Review · ReadLater · Done · New | user (Enrolled and New may be proposed, never applied silently) | workflow; never emitted by a classifier |

The `Org/` axis is retired: the sender key already is the organisation. On single-folder providers the folder is `Kind/Topic` and State lives in keywords or the ledger.

**Kind rules (in order; first match wins)**

1. Protected sender or replied-to human → People (never filed).
2. From one of the user's own addresses or agents → Other/Agent (never filed as a notification).
3. `List-Unsubscribe` + (`List-Id` or known ESP Return-Path or `Precedence: bulk/list`) → Subscriptions.
4. Receipt signatures: order/shipment/return/payment local-parts (`order-update@`, `shipment-tracking@`, `return@`, `auto-confirm@`, `invoice@`, `receipt@`, `statement@`) or retailer/payment domains with no `List-Unsubscribe` → Receipts.
5. `noreply`/`notification`/`alerts` local-parts, platform domains, or GitHub/Slack/Circle patterns → Notifications; GitHub sub-routed by the reason in the cc address (`mention@`, `review_requested@`, `assign@` → needs you; `comment@`, `author@` → filed; `ci_activity@`, `push@`, `state_change@` → source-settings candidate).
6. Human-looking sender (display name, no bulk markers, ≤ 10 recipients) → People with `State/New` until replied to or approved; > 10 recipients → People/Broadcast.
7. Else → Other, queued for judgment.

**Sender key.** `local-part@registrable-domain` (public suffix list), because one domain carries several Kinds: a bank's `account@` (Notifications, protected) vs `webinar@` (Subscriptions); Google's security alerts vs Workspace promos; Amazon's five event addresses. Plus-addressing and display-name variants normalise to the same key.

**Topic gates.** A topic appears in an account's sidebar only if it has ≥ 200 messages or ≥ 3 distinct senders, and a distinct disposition or retention from its neighbours; otherwise it collapses into its parent or Other. Expect Pareto: the top 8–10 topics cover \~85% of volume (measured: top 20 senders = 85% on the business account). A monthly promote/demote pass re-applies the gate.

**Judgment calls.** Senders unresolved by rules (≈ 10% of senders, < 50 per account at onboarding) are batched to Claude in one call with sender key, counts, three subjects and the candidate topics; the output is a rule, not a message decision. The prompt forbids disposition words as topics and never emits State. Jev and Laya are excluded by decision; no other model is required.

**Reconcile mode (existing labels).** Detect label generations by ID age and coverage; map flat topics to `Topic/` (Developer → OpenSource, $Finance → Finance, Family → Household); split catch-alls by header (`Newsletters` → Subscriptions + Notifications); fold `Org/*` into sender; delete empties immediately and migrated labels only after counts reconcile; never create a fourth scheme without mapping the other three.

**Protected lists.** Replied-to humans, starred, `State/Enrolled`, security and account alerts, bank alerts, calendar invites, and role-specific additions (clients for business; family, school, insurance, housing for personal and household). Protected mail is never archived by a plan and never an unsubscribe candidate.

**Sensitive categories.** Senders in health, legal or similar categories are filed under a generic topic, never named in a digest or review, and excluded from unsubscribe and identity-map output.

## 8. Workflows and periodic tasks

The reset clears stock once, oldest era first; the flow rules and the source-settings checklist stop the refill; the digest and the Monday review are the whole ongoing cost.

**Reset (stock), applied oldest era first**

| Era | Treatment | Why this order |
| --- | --- | --- |
| > 5 years (personal: 2,079 inbox threads; professional: part of 9,232 > 3 yr) | Kind only, archive, mark read; no topic judgment | lowest risk; proves ledger, throughput and undo |
| 2–5 years | Kind + Topic; Receipts and People kept; subscriptions from senders dead > 2 yr skipped | bulk of history, still cheap |
| Last 2 years | full treatment: Kind, Topic, protected list, screener, open loops | where judgment matters |
| Last 30 days | untouched until flow rules have run a week | avoids moving live threads |

Each era is its own plan hash; large applies run across sessions with resume. Before every chunk, UIDs changed since snapshot are re-planned. Deletion is never part of a reset.

**Flow (new mail)**

1. Export sender rules as provider filters (Gmail filters XML; Outlook rules) so routing happens server-side before the agent runs; protected and `State/Enrolled` senders stay in the inbox.
2. The agent's incremental scan handles stragglers (unmatched senders) and the screener (`State/New` for first-time humans).
3. The source-settings checklist, ranked by volume eliminated, for things no email tool should handle: measured examples — Circle 22% of business inflow; GitHub CI/push/state-change 423 threads/month on professional; USPS Informed Delivery daily; LinkedIn job alerts 152/month.

**Subscriptions, tiered by the user's own behaviour**

- Never opened in 90 days and ≥ 10 messages → unsubscribe candidate, handed to Gmail's Manage subscriptions / Outlook's Subscriptions page; the agent only auto-files.
- Sometimes opened → `Subscriptions/<topic>`, filed, read on schedule.
- Paid or enrolled → inbox until the program ends.

**Daily close-out (inbox to zero)**

The inbox is treated as a queue that is emptied at the end of each day or session, not a list that is kept. `/mail closeout` lists every item left in the inbox with a suggested disposition and one approval applies them all; undo reverses the whole close-out.

| Disposition | Applies to | What happens |
| --- | --- | --- |
| Needs you | threads awaiting your reply or decision | leaves the inbox under `State/Review`; stays at the top of tomorrow's brief with its clock; once past its SLA it re-enters the inbox at the next morning brief (boomerang) |
| Done | read, no action | archived |
| File | bulk mail with a known rule | `Kind/` + `Topic/` applied, archived |
| New sender | first-time bulk sender | filed under a proposed rule; the rule goes to Monday |
| Seen alert | security, bank, calendar after you've seen it | archived, still searchable under its Kind |
| Keep | your explicit choice | stays, counted against tomorrow's zero |

Open loops therefore live in the brief, not the inbox; the brief is the to-do list, the inbox is the intake. A close-out never changes People threads' labels beyond `State/Review`, and never touches protected mail without the explicit Seen-alert disposition.

**Attention brief**

The morning brief orders items by what they cost if missed: open loops past their topic SLA (Clients 2 days, Household 3, Career 3, Community 7), review requests and mentions with the repo named, enrolled-program mail, security and account alerts, new humans, then counts of everything filed. It is a report; nothing in it is applied.

**Periodic tasks**

| Cadence | Delivered as | Content | Action required |
| --- | --- | --- | --- |
| Morning, daily | `/mail brief` (also saved as a page) | attention brief: open loops past SLA, mentions and review requests, enrolled-program mail, protected alerts, new humans; counts filed per Kind/Topic | usually none; reply or star |
| End of day or end of session | `/mail closeout` | every inbox item with a disposition; one approval empties the inbox | one tap; undo available |
| Weekly, Monday | `/mail review`, all accounts merged and tagged | new senders (approve Kind/Topic or unsubscribe); unsubscribe candidates; open loops; strays that reached the inbox 3+ times; enrolled-program check; GitHub repos gone quiet; rules eligible to graduate to trusted; noise trend with the three biggest levers | each item yes/no, ≤ 10 min |
| Monthly | `/mail health` + `/mail suggest` | promote/demote topics against the gate; sender drift; organization proposals with expected effect; retention proposals only on trusted Notifications rules with ephemeral senders (trash after 365 d, 30-day undo), never on the personal account; Receipts, People and security never; security (app password age, 0 sends, revoke link); trusted-rule audit | one judgment call; the rest opt-in |
| Quarterly | `/mail map` | identity map: which address each bulk sender has and whether that's the one you'd choose; consolidation recommendations | none; recommendations only |

**Sample daily digest**

```
Yesterday · christopher.a.phillipson@gmail.com
Inbox now: 3 (1 review request on sindri · 1 recruiter reply · 1 household thread)
Filed: 28 GitHub (3 need you ↗) · 5 LinkedIn alerts · 2 receipts · 4 newsletters
Protected: 1 security alert (in inbox)
New senders: 1 bulk (auto-filed, review Monday) · 0 humans
Open loops: Household 1 (2 days) · Career 0
```

**Sample Monday review**

```
Week of Oct 27 · 3 accounts
1. New senders (9): approve suggested Kind/Topic, or unsubscribe   [per sender]
2. Unsubscribe candidates (4): ≥10 messages, 0 opened in 90 days
3. Open loops (2): last message theirs, > 7 days
4. Strays (3): reached an inbox unclassified 3+ times → add filter?
5. Enrolled: Creator College still active? (last session email 2 days ago)
6. GitHub: 2 repos with 0 participation in 30 days → unwatch?
```

**Cross-account.** One rules table with global and per-account scope (LinkedIn is Notifications everywhere; a vendor can be a client on one account and a newsletter on another); one People graph so a correspondent on two addresses is one open loop; one Monday review.

**Migration nudges (decided 27 Oct).** Both Gmail accounts are Mixed by era. The identity map is bidirectional and only senders active in the last 90 days whose role-home differs become nudge candidates — today, Snyk alerts on the personal address (home: professional) and the household institutions on the professional address (home: personal). A nudge is a one-line suggestion in the brief or the Monday review, per sender, with the vendor's account-settings page where known; the user changes the address at the vendor, the agent never does, and declining a nudge silences it for 90 days. Dead-era mail (the older professional folders on the personal address, under 0.5% of that mailbox) is archived in place with Kind only — no Topic, no migration. The quarterly map reports candidates remaining and nudges accepted.

## 9. Testing and verification

Everything that touches a mailbox is tested against an in-process fake IMAP server first, then proven on the three real accounts in read-only mode, then on the deep-archive era with undo, before anything recent moves.

**Layers**

| Layer | What | Tool | Gate |
| --- | --- | --- | --- |
| Unit | header → Kind rules; sender key normalisation; topic gate; planner diff; era split | cargo test, pure functions | 100% of rules, property-based on sender keys |
| Golden mailboxes | three synthetic mailboxes shaped like the measured accounts (business: 4k, 49/49 split; professional: GitHub reasons, triple labels; personal: 45 vendor folders, Amazon events) | fixtures generated from the profiles, no real mail | snapshot, archetype and plan counts match expected within ±2% |
| Fake IMAP | in-process server speaking LOGIN, EXAMINE/SELECT, UID FETCH/STORE/MOVE/COPY, CONDSTORE, `X-GM-EXT-1`, Proton `Labels/` semantics, keyword-less Outlook profile | emailibrium's MockEmailProviderFactory extended into an in-process fake IMAP server | every adapter path exercised; plan → apply → undo round-trips to identical label state |
| Invariants | no write without approved hash; undo restores exact prior labels; re-run is a no-op; protected mail never in a plan; State never emitted by classifier; no secret in any log, tool result or transcript | assertions + log scanning in CI | zero violations |
| Live, read-only | `doctor` + `scan` + `snapshot` on all three accounts | the real IMAP hosts, from the Mac | counts reconcile with Gmail label totals |
| Live, reversible | deep-archive era on fastnsilver (2,079 inbox threads > 5 yr): apply, verify, undo, verify, re-apply | real mailbox, Gmail | label state identical after undo; second apply is a no-op |
| Auth | credential form end-to-end with a throwaway app password; keychain round-trip on macOS/Windows/Linux; device-code against a test Microsoft account | manual + scripted | no secret ever appears in MCP traffic (captured and grepped) |

**Acceptance criteria for v1**

- [ ] Connect any of the three accounts in under 2 minutes with no console setup.
- [ ] Snapshot reproduces the Oct 26 profiles within sampling error and flags the professional account as Mixed.
- [ ] Deep-archive plan on fastnsilver applies and undoes cleanly; journal is complete.
- [ ] Rules export yields working Gmail filters; the next morning's inflow lands filed, with protected senders in the inbox.
- [ ] Daily digest and Monday review render from the ledger with no body access.
- [ ] A full transcript of an onboarding contains no credential string.
- [ ] Second account added without re-answering shared rules.

**Things we cannot test in a sandbox.** Hosted chat sandboxes cannot reach IMAP hosts; all live tests run on the Mac. Provider-side behaviour changes (Google app-password policy, Microsoft public-client tolerance, Gmail bandwidth limits) are monitored by `doctor` on each run, not assumed.

## 10. Implementation plan

Seven milestones inside pacphi/emailibrium (Rust backend, MIT), each ending in something you can run on a real account; the first live proof is the deep-archive era on fastnsilver, not the business inbox. Revised 27 Oct for the emailibrium pivot.

| # | Milestone | Deliverable (in emailibrium) | Exit test |
| --- | --- | --- | --- |
| 0 | Baseline | run v0.2.0 locally; connect the three accounts via the existing IMAP onboarding; fix the Workspace QUICKSTART; extend MockEmailProviderFactory into a fake IMAP server | all three accounts ingest headers; `list_subscriptions` and `get_insights` return |
| 0.5 | Security baseline — **already done, corrected 27 Oct (verification pass)** | Every deliverable this row originally asked for already shipped on `develop` (commits `e03220c`/`506ddd0`, predating this plan): `middleware::local_auth::protect_http_router` wraps the outer router after both `/api/v1` and the sibling-mounted `/api/v1/mcp` are nested (`main.rs`); bearer credential required and derived from `JWT_SECRET`, exchanged for an HttpOnly session cookie for the browser; explicit `Origin` allowlist (not wildcard) rejects DNS-rebinding attempts even with a correct bearer; `config/environments/{development,production}.yaml` and `docker-compose.yml` already bind/publish `127.0.0.1` only; a startup log line already states the bind address and `authentication = "required"` | **verified 27 Oct, not simulated:** `cargo test --lib local_auth` passes 9/9, including the load-bearing test (`unauthenticated_rest_and_sibling_mcp_are_rejected_over_http`) and the malicious-origin test, both against the real router shape `main.rs` builds |
| 0.6 | MCP identity & authorization — **phase 0 done, verified 27 Oct (ADR-037)**; phases 1–2 remain | Done: `ToolContext` carries `Principal::local_operator()`, never from a request field; `user_id` removed from `PreviewCleanupPlanRequest` (`#[serde(deny_unknown_fields)]` rejects a caller that still sends it); `cancel`/`list_operations`/`sample_operations` now enforce the owning principal (`pl-plan-id-only-authz` closed), matching REST's `cancel_plan`/`list_operations`/`sample_operations`; of the 15 read tools, only `preview_cleanup_plan` had an owner column to scope — the other 14 are enumerated in ADR-037 §3 as not user-scoped by design. **Remaining (phases 1–2, not built):** rate limiting and the audit trail keyed per-caller instead of globally; a capability/scope model in `config/tools.yaml` for future write tools | **Met:** two-principal tests prove principal A cannot see principal B's rows (`cancel_rejects_a_plan_belonging_to_another_principal`, `list_operations_and_sample_operations_reject_another_principal`); a request still sending `user_id` is rejected at the MCP layer (`preview_cleanup_plan_rejects_a_caller_supplied_user_id`, JSON-RPC -32602); full `cargo test`/`cargo clippy -D warnings` clean on `develop`. **Not yet met:** per-caller rate limiting, capability model (phases 1–2) |
| 1 | Kind + sender key | header-rule Kind classifier ahead of the semantic one; `local-part@domain` sender key; `Kind/Topic/State` label namespaces in ADR-018 ops; role on the account | snapshot reproduces the Oct 26 profiles; professional account flagged Mixed |
| 2 | Staff tools (read) | `tools/staff`: `get_snapshot`, `brief`, `doctor`, `review` (read), `identity_map`; body-free allowlist; `agent/` skill + commands | brief and review render from the ledger with no body access |
| 3 | Staff tools (write) | authority config; `plan_apply` / `plan_undo` and `closeout_preview` / `closeout_apply` over the cleanup orchestrator; era strategies in the plan builder; close-out journal | fastnsilver > 5-year era: apply, undo, re-apply no-op; a close-out empties an inbox and undoes |
| 4 | Rules + flow | authority levels, Apply once / Always, graduation and corrections; topic gates; reconcile mode for existing labels; Gmail filter export; settings checklist; screener | one week of filed inflow with protected senders in the inbox; first rule graduates |
| 5 | Cadence | health report, noise trend, retention per rule, boomerang; multi-account Monday review; pages for brief/review | two weeks of use with ≤ 10-minute Mondays and inbox zero ≥ 90% of days |
| 6 | Providers | Outlook device-code public client (beta, configurable ID); Proton Bridge preset; keyword labels for Fastmail/iCloud | adapter conformance suite passes on the fake and live |
| 7 | Publish | release notes, docs, client onboarding guide, design system applied | a non-developer connects a mailbox from the README alone |

**Repo layout**

```
emailibrium/
  backend/src/tools/staff/      # NEW: snapshot, brief, closeout, plan_apply/undo, review, health, identity_map, rule_set_authority
  backend/src/tools/readonly/   # existing 15 tools (unchanged)
  backend/src/cleanup/domain/   # + header Kind rules, era strategies, sender key
  backend/src/rules/            # + authority level, graduation, corrections, retention
  backend/migrations/           # + senders, rules, closeout_journal, account role
  backend/tests/fake_imap/      # NEW: in-process IMAP server from MockEmailProviderFactory
  agent/                        # NEW: SKILL.md, slash commands, judgment-batch prompts, authority config example
  docs/ADRs/ADR-038-chief-of-staff-authority-model.md   # NEW (was mislabeled ADR-033 in the first draft, then ADR-037; 033-036 are taken by the SeaORM/Postgres work, and ADR-037 was claimed by the real milestone-0.6 identity work, 27 Oct)
  docs/ADRs/ADR-039-header-kind-classifier-and-sender-key.md  # NEW (was mislabeled ADR-034, then ADR-038)
```

**Adopt vs build**

- Keep as is: emailibrium's IMAP ingestion and onboarding, encrypted credential store, rules engine, cleanup orchestrator and plan hash (ADR-030), SSE, read-only MCP tools, local embeddings and clustering.
- Add: header Kind rules, sender key, label namespaces, era strategies, authority model, staff tools, reports, fake IMAP server, two ADRs.
- Correct: Workspace on app passwords; Outlook device code with public client; QUICKSTART provider table.
- Retire: the Oct 12 TypeScript sorter (its planner, ledger and undo ideas are already in ADR-030); the first-draft `mail-agent` server (the npm name is taken anyway).
- Study, don't fork: better-email-mcp (device-code flow), proton-bridge-mcp (Bridge port discovery and label semantics).

**Sequencing notes**

- **0.5 and 0.6 were inserted 27 Oct, recovered from `.autopilot/`'s own queued `security-baseline` and `mcp-identity-authz` pipelines** (parked 2026-08-06, found during the retirement of that folder; full phase detail preserved in `docs/plan/postgres-autopilot-delta-2026-10.md` §4a/4b, since the pipeline files themselves are gone). **Correction, same day, later verification pass: 0.5 was already stale when inserted.** The two pipeline files were written against the tree as it stood 2026-08-06; `develop` has since shipped the real fix (commits `e03220c`/`506ddd0`) independently of the parked autopilot pipeline. Checked by running the actual test suite, not by re-reading the pipeline's claims. **0.6 (`mcp-identity-authz`) WAS independently re-verified against current `develop` before anything was built, same day** — unlike 0.5, its 2026-08-06 claims (client-supplied `user_id`, unscoped read tools, both named defects) held up exactly: `git log` confirmed the 22 Sep hardening pass that fixed 0.5 never touched `backend/src/tools/`. Phase 0 is now built and verified (ADR-037); phases 1–2 (per-caller rate limiting, capability model) remain. 0.5 and 0.6 touch different files than Milestone 1 (`backend/src/middleware/`, `backend/src/tools/context.rs`/`rate_limit.rs`/`registry.rs`, `config/tools.yaml`, `config/environments/`) so they can run as a parallel lane alongside Milestone 1, but both must land before Milestone 2 starts.
- **0.6's dependency-chain gap turned out not to block phase 0:** the retired `mcp-identity-authz` pipeline said it ideally follows a fix (from the also-retired `correctness-debt` pipeline) that lib-exports `AppState` so authorization tests exercise the real router rather than the replica handlers in `backend/tests/api_integration.rs`. Phase 0's new tests exercise `SeaOrmCleanupPlanRepo` directly — the real implementation, not a replica — so the gap didn't apply here (see ADR-037 §5). It remains relevant for phases 1–2 or any future test that goes through `api_integration.rs` instead.
- Milestones 0–5 are Gmail and Workspace only on purpose: all three of Chris's accounts are there, and the hardest behaviours (reason routing, era plans, reconcile, authority) are provider-independent.
- Milestone 3 before 4: undo must be proven on low-risk mail before any rule writes to recent mail.
- Backup (GYB or Takeout) of fastnsilver is a manual prerequisite for milestone 3 — \~80k messages is the one mailbox where a mistake would be expensive.
- The semantic classifier stays, but runs after the header rules: it fills Topic where headers can't, never Kind, and never State.
- Estimates at one experienced engineer with Claude Code, in Rust: milestones 0–2 in two weeks, 3–4 in two more, 5 in one, 6 in two, 7 in one. The schedule risk is live-testing time, not code.

**Portfolio decisions (27 Oct, from the explorer)**

| Area | Decision | Effect on code |
| --- | --- | --- |
| Triage policy | Advance | wire as the decision core |
| Unsubscribe execution | Advance, scoped | RFC 8058 one-click POST only, gated by DKIM/DMARC alignment from `Authentication-Results` and per-sender Apply once / Always; `mailto:` and plain links are handed to the user (a tappable `mailto:` opens their own mail client; Gmail and Outlook bulk pages linked where they exist). Works on every supported provider because the POST is made by the local engine from message headers, not by the provider |
| PostgreSQL via SeaORM | **Already done, corrected 27 Oct (verification pass)** — this row described work as pending that autopilot's own phase records (gate-PASSED 0–5, court verdict SHIP) and the code both confirm is finished on `develop`: `Database::Postgres` is real and URL-dispatched, SeaORM is fully wired (84 files), the three named PG-only tests run for-real against a live `postgres:16-alpine` CI service rather than returning early | nothing to build here; the one real remaining gap is full-text search (BM25 via ruvector-postgres, ADR-034) — deliberately deferred, never started, tracked separately (see follow-up plan) |
| Ollama provider | Keep and verify | already the local generative path; add the `doctor` golden-judgment check and per-judgment provenance |
| LM Studio provider | Schedule (new, milestone 5) | provider kind over the OpenAI-compatible client pointed at `localhost:1234/v1`, classified local for the privacy gate, model list from `/v1/models`, JSON-schema output; appears beside Ollama in AISetup and Settings |
| Semantic classifier, Gmail OAuth, Outlook OAuth | Park behind flags | `classification.mode = rules` and `oauth.enabled = false` by default; code stays compiled |
| Ingest-tag-archive auto modes | Schedule | `ArchiveStrategyPicker` becomes the authority picker (milestone 4) |
| GNN clustering, SONA learning, built-in llama.cpp, evaluation harness, multi-tenancy groundwork, autopilot pipelines | Prune | \~25 of 136 routes, two crates and one Cargo feature removed; Inbox Cleaner Step 3 becomes Senders and Kinds; AISetup defaults to the `onnx` tier; `user_id` collapses to one principal alongside the authority change |
| Search, RAG and chat, reader and compose | Keep | out of the filing path; compose never on MCP |

**What the prunes remove, layer by layer (measured on develop, 27 Oct)**

| Area | Backend | API | UI | Size | Replacement |
| --- | --- | --- | --- | --- | --- |
| GNN clustering | `vectors/clustering.rs`, `api/clustering.rs`, `topic_clusters` table, `cluster_actions` input to the plan builder, MCP `list_clusters`; 18 files | 6 routes | 27 files; Inbox Cleaner step 3 is the clusters step (39 references in `Step3Topics.tsx`), plus command center and insights | L | Senders and Kinds step from header rules and `senders_group`; `sender_actions` in the plan builder |
| SONA adaptive learning | `api/learning.rs`, `api/interactions.rs`, `user_learning` table, relevance re-rank in search, MCP `get_learning_metrics`; 30 files | 6 routes | 4 files: a settings panel and one insights widget | M | a `corrections` table feeding rule graduation; search stays hybrid without re-rank |
| Built-in llama.cpp | Cargo feature `builtin-llm`, `llama-cpp-4` and `hf-hub` deps, `generative_builtin.rs`, model catalog and downloader (keep `model_integrity` for embeddings); 9 files | \~5 of 22 `ai/*` routes | 19 files: `services/ai` downloader (12), settings (3), `AISetup` whose default tier is `builtin` today | M | `AISetup` defaults to `onnx` (“embeddings only, rule-based classification”); Ollama and LM Studio are the local generative paths |
| Evaluation harness | `api/evaluation.rs`, `tests/classification_evaluation.rs`, eval DDL arm; 19 files | 8 routes | 0 files | S–M | golden mailboxes in milestone 0 |
| Multi-tenancy groundwork | `user_id` threaded through 23 files, required by the cleanup API and MCP `preview_cleanup_plan`; `wipe_user_data` | cleanup route signatures change | the `api` package types | M | one local principal; done together with the authority change since both touch cleanup |
| Autopilot pipelines | `.autopilot/`, `AGENTS.md` sections, a few `justfile` targets | — | — | **not trivial — correction 27 Oct**: `.autopilot/` is tracked and active (real phase-history commits for postgres-support, ci-build-optimization, docs-accuracy-audit as of Sep–Oct), not inert history as first assumed | open question, not a prune: confirm with the user whether `.autopilot/` is still driving in-flight work before removing it |

Net: about 25 of 136 routes removed (18%), two heavy crates and one Cargo feature gone, one wizard step and one onboarding tier replaced — both of which the plan was rewriting anyway. PostgreSQL is not pruned: it stays an optional backend with SQLite as default, and its three PG-only tests already run against a disposable database in CI (verified 27 Oct — this was previously, incorrectly, described as still pending).

**What parking means in each layer.** Semantic classifier: a runtime setting `classification.mode = rules | semantic`, default `rules`; the categorizer stays compiled; `AISetup` copy changes. Gmail and Outlook OAuth: `oauth.enabled`, default off; `ProviderSelector` hides the two cards unless enabled (4 onboarding files, the router, two types); the three OAuth routes answer “disabled”; `just setup` stops prompting for client secrets.

**Sequencing.** Merge develop into main before removing anything: the September security fixes and the SeaORM port are interleaved in the same 51 commits, so prunes are applied to main after the merge, each as its own PR, in the order above (cheapest and least coupled first: autopilot, evaluation, learning, built-in, clustering, then tenancy with the authority change).

## 11. Risks, open questions and decisions log

The largest risks are provider policy changes outside our control; the design keeps each one behind an adapter and a `doctor` check so a change breaks one path, not the product.

**Risks**

| Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- |
| Microsoft restricts the public client used for Outlook device code | medium | Outlook path stops | beta label; configurable client ID; email-oauth2-proxy fallback; document own Entra registration |
| Google withdraws app passwords for personal or Workspace | low–medium | all three accounts | watch Google's transition page; OAuth-client option kept as a documented, optional path |
| Gmail IMAP daily bandwidth or connection limits hit during a large scan | medium | slow first scan | header-only fetches, ≤ 3 connections, checkpointed resume, scan estimate in `doctor` |
| Claude Desktop/Cowork elicitation still unreliable | high today | onboarding UX | server-opened form is the default; elicitation feature-detected with timeout |
| Reconcile mode mislabels an existing taxonomy | medium | user trust | migrations only after counts reconcile; old labels retained until verified; undo |
| Sensitive sender named in a digest | low | privacy harm | generic-topic filing, exclusion lists, test that scans reports for excluded senders |
| Proton Bridge resync or duplicate copies across labels | medium | Proton counts drift | dedupe by Message-ID; skip All Mail; re-scan on UIDVALIDITY change |

**Open questions**

- [x] Digest as email or command? Resolved 27 Oct: the agent never creates email; briefs and reviews are command output or a page.
- [x] Open loops at close-out. Resolved 27 Oct: leave the inbox under `State/Review`; the brief is the to-do list; a loop past its SLA boomerangs into the inbox at the next morning brief.
- [x] Trusted-rule graduation. Resolved 27 Oct: every suggestion offers Apply once / Always; Always graduates immediately; three consecutive clean Apply-once Mondays graduate automatically with a notice.
- [x] Deletion. Resolved 27 Oct: never on the personal account; elsewhere opt-in per trusted Notifications rule, 365 days, 30-day undo; Receipts, People and security never.
- [x] Name, license, home. Resolved 27 Oct: the chief of staff is built inside pacphi/emailibrium under its MIT license; design system deferred to milestone 7.
- [x] Cross-over mail between accounts. Resolved 27 Oct: both Gmail accounts are Mixed by era; nudges only for active senders whose role-home differs, per sender, user-executed; dead-era mail archived in place with Kind only. First candidates: Snyk → professional; household institutions → personal, a good portion but not all.

New, added during the verification pass (27 Oct, later same day):

- [ ] **Local-engine unlock gate vs. `/mail connect`.** `QUICKSTART.md` now documents a session step ("Unlock the local engine") inserted before mailbox connect: a JWT-style local access token exchanged once for an HttpOnly session cookie, required before the SPA's onboarding page will work. Section 6 of this plan never accounts for it. Open: does an MCP client (Claude Code/Desktop/Cowork) calling `/mail connect` need to clear this gate too, and if so, how does a stdio-connected agent present or obtain the token? Needs an answer before milestone 2 (staff tools).
- [ ] **`.autopilot/` disposition.** See the correction in §10's prune table — it is live, not inert. Confirm before any prune PR touches it.

No other open questions remain as of 27 Oct 2026 (verification pass); items above added same day.

**Decisions log (this conversation, 26–27 Oct 2026)**

| Decision | Rationale |
| --- | --- |
| IMAP for all providers; no Google Cloud or Azure setup | the earlier builds failed on exactly that requirement |
| Secrets never enter the model; localhost form + keychain | transcripts and Cowork sessions are not a place for passwords |
| Jev and Laya excluded | header rules plus one Claude judgment per sender is sufficient and local |
| Five Kinds including Receipts | the personal account's recent inbox is almost entirely receipts |
| Sender key = local-part + domain | one domain carries several Kinds on every account measured |
| Role is declared, not detected | the address-based guess was wrong for two of three accounts |
| `Org/` axis retired; State is user-set only | overlap on the same messages; a classifier had auto-applied ReadLater |
| Topics gated at ≥ 200 messages or ≥ 3 senders; 8–12 per account | the 25 observed topics collapse to that; the 45 hand-made vendor folders show per-sender does not scale |
| Oldest era first; never delete; monthly retention is opt-in | lowest-risk proof of the ledger and undo |
| One rules table with global/account scope; one People graph | the same humans and vendors appear across accounts |
| The agent never creates email (no sends, drafts, replies or digest emails) until explicitly revisited (27 Oct) | the user wants a chief of staff that reports and suggests, not a correspondent |
| Every capability starts at report-only and graduates per rule; auto-apply only for filing and archiving (27 Oct) | authority is earned on evidence, and stays reversible |
| The inbox is a queue emptied by an approved close-out each day or session; open loops live in the brief (27 Oct) | inbox zero as a daily state, not a one-time purge |
| Open loops leave the inbox at close-out and boomerang back when past SLA (27 Oct) | the inbox has never worked as a task list (Whittaker & Sidner 1996; 81–89% unread across three accounts); time-based return is the pattern every product converged on |
| Apply once / Always on every suggestion; Always graduates now, three clean Mondays graduate automatically (27 Oct) | count thresholds are met by one burst (5 messages in 5 minutes); weekly cycles test a rule against sender drift; Always is the Gmail-filter mental model |
| No deletion on the personal account; elsewhere per trusted rule, 365 d, Notifications only (27 Oct) | storage and findability gains are negligible; one misfiled receipt trashed is an asymmetric loss |
| Build inside pacphi/emailibrium (Rust, MIT) rather than a new TypeScript server (27 Oct) | emailibrium v0.2.0 already has IMAP onboarding, rules, plan/apply with undo and a read-only MCP server; the gap is the authority model, header Kinds and the reports |
| Send and compose are never registered as MCP tools; write tools are gated by an authority config (27 Oct) | makes "never creates email" a property of the surface, not a prompt |
| Name emailibrium, license MIT, design system deferred to milestone 7 (27 Oct) | matches the existing repo and brand; nothing depends on the design system before publishing |
| Cross-over mail: era-aware, active-only migration nudges, per sender, executed by the user; dead-era mail archived in place with Kind only (27 Oct) | the dead professional era on personal is under 0.5% and below the topic gate; the active list is short (Snyk one way, household institutions the other), so nudges finish it without a project |
| Unsubscribe: one-click POST (RFC 8058) only, DKIM/DMARC-gated, per-sender approval; mailto and links handed to the user (27 Oct) | honours "never creates email" and works on every provider because the engine makes the request from headers |
| PostgreSQL kept and advanced as an optional backend; SQLite default (27 Oct) | keeps a hosted or client-run option open; the PG tests are already real as of this verification pass — not a remaining cost |
| Models appear only in judgment batches and SPA chat/search; provider tiers unchanged; rules-only is the privacy default; header-only payload contract; provenance recorded per judgment (27 Oct) | provider choice changes quality and privacy posture, never safety; emailibrium's first promise was choice with privacy first |

Decided 27 Oct, later same day: milestones 0.5 (security baseline) and 0.6 (MCP identity & authorization) inserted ahead of Milestone 2, recovered from `.autopilot/`'s own queued, never-started pipelines found while retiring that folder — not because new work was invented, but because the MCP server this plan adds mail-reading tools to turned out to have no authentication and spoofable caller identity today. Full phase detail in `docs/plan/postgres-autopilot-delta-2026-10.md` §4a/4b.

Decided 27 Oct, still later the same day: 0.5 turned out already shipped (22 Sep, independently of this plan) — corrected in place rather than rebuilt. 0.6 was re-verified against current code first (unlike 0.5's first pass) and found genuinely still needed; phase 0 (identity derivation, schema break, `pl-plan-id-only-authz`) was then built and verified on `develop`, recorded as ADR-037. Phases 1–2 (per-caller rate limiting, capability model) remain open, deferred until a write tool actually needs them.

Addendum (27 Oct): local generative models are served by Ollama today and by LM Studio once scheduled; both are classified local by the privacy gate, so neither prompts for consent and neither sends anything off the machine. The judgment payload contract, schema-constrained output and provenance rules apply to both identically.

## 12. References

Pages opened or returned during this work; account figures come from the Gmail connector on 26 Oct 2026 and are not public sources.

- Google Workspace: [Transition from less secure apps to OAuth](https://knowledge.workspace.google.com/admin/sync/transition-from-less-secure-apps-to-oauth) — app passwords remain the exception for IMAP.
- Google Account Help: [Sign in with app passwords](https://support.google.com/accounts/answer/185833).
- Microsoft Support: [Modern authentication methods now needed to continue syncing Outlook email in non-Microsoft email apps](https://support.microsoft.com/en-US/support/known-issues/modern-authentication-methods-now-needed-to-continue-syncing-outlook-email-in-non-microsoft-email-ap) — basic auth ended 16 Sep 2024.
- Thunderbird Help: [Microsoft OAuth authentication and Thunderbird in 2026](https://support.mozilla.org/en-US/kb/microsoft-oauth-authentication-and-thunderbird-202).
- Public client ID practice for Outlook IMAP: [ficd.sh](https://ficd.sh/blog/outlook-school-email-ouath/); bundled public client in [better-email-mcp](https://github.com/n24q02m/better-email-mcp).
- Proton: [Labels in Bridge](https://proton.me/hi/support/labels-in-bridge).
- Apple: [App-specific passwords](https://support.apple.com/en-us/102654). Fastmail: [App passwords](https://www.fastmail.help/hc/en-us/articles/360058752854-App-passwords). Yahoo: [Generate and manage 3rd-party app passwords](https://help.yahoo.com/kb/sln15241.html).
- Thunderbird: [Autoconfiguration (ISPDB)](https://developer.mozilla.org/en-US/docs/Mozilla/Thunderbird/Autoconfiguration).
- MCP: [Elicitation specification (2025-11-25)](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation); Claude Code: [Connect to tools via MCP](https://code.claude.com/docs/en/mcp); open issues on Desktop/Cowork elicitation: [#88901](https://github.com/anthropics/claude-code/issues/88901), [#56243](https://github.com/anthropics/claude-code/issues/56243).
- Whittaker, Matthews, Cerruti, Badenes, Tang: [Am I wasting my time organizing email?](https://dl.acm.org/doi/10.1145/1978942.1979457) (CHI 2011) — filing did not improve retrieval; search and threading did.
- Gmail limits summary: [mailjerry.com](https://www.mailjerry.com/gmail-limits/) (third-party; verify against Google's pages before relying on figures).
- Earlier research in this conversation: the Oct 2026 report "Email Agent Design: Auth, Onboarding, Intent and Inbox Strategy" (Claude artifact) for product comparisons and the Unroll.me trust lesson.

## 13. Execution lanes — orchestration addendum (27 Oct, later same day)

Decided this pass: write the lane plan only; no swarm runs yet. Grounded via the ruvnet-brain capability cards, not memory: ruflo is an orchestration-and-memory layer (hierarchical/mesh/ring/star topologies, cross-session state in `.swarm/memory.db`) — it does not itself guarantee non-colliding parallel work; file ownership has to be fixed by hand, which is what this table does. Agentic-qe's test generators cover Jest/Vitest/Playwright/pytest/JUnit, not Rust, so its role here is frontend test generation and its `quality-gate`/coverage-gap tooling wherever JS/TS or Tarpaulin (Rust) coverage exists. Adversarial code review (`brutal-honesty-review`, `qe-court`) applies everywhere, Rust included — those are Claude-side review skills, not QE-tool-specific.

**Lanes and file ownership** (no two lanes touch the same path at the same time):

| Lane | Scope | Owns | Depends on | Can start |
| --- | --- | --- | --- | --- |
| A — Core spine | M0→M1→M2→M3→M4→M5 | `backend/src/tools/staff/`, `backend/src/cleanup/domain/`, `backend/src/rules/`, `backend/migrations/`, `backend/tests/fake_imap/`, `agent/` | A0 must land before M2 starts | now |
| A0 — Security prerequisite **(M0.5 done, verified already-shipped 27 Oct; M0.6 phase 0 done + verified 27 Oct, ADR-037)** | M0.5→M0.6 phase 0 done. **Remaining:** M0.6 phases 1–2 (per-caller rate limiting/audit, capability model) | `backend/src/tools/rate_limit.rs`, `backend/src/tools/registry.rs`, `config/tools.yaml` (M0.5's `backend/src/middleware/`/`config/environments/` and M0.6 phase 0's `backend/src/tools/context.rs`, `backend/src/principal.rs`, `backend/src/cleanup/repository/plan_repo.rs` are all landed) | nothing; M1 can proceed | M0.6 phases 1–2, whenever a write tool actually needs them |
| B — Provider adapters | M6 (Outlook device-code, Proton Bridge, Fastmail/iCloud keyword labels) | provider adapter modules (exact path to confirm at kickoff) | Lane A's M1 (sender-key/Kind model) merged | after A reaches M1 |
| C — Frontend | §6's screen table | `frontend/apps/web/` feature dirs named there | Lane A's API contracts per milestone; scaffolding can start on mocked contracts | scaffolding now, wiring trails each A milestone by one step |
| D — Docs/ADR | ADR-038, ADR-039 drafts (renumbered 27 Oct — ADR-037 went to the real milestone-0.6 identity work); QUICKSTART/README fixes; plan upkeep | `docs/ADRs/`, `QUICKSTART.md`, `README.md`, `docs/plan/` | nothing | now, runs continuously |
| E — QE/fixtures | golden mailboxes, fake-IMAP invariants, property-based sender-key tests, coverage wiring | `backend/tests/` (excluding `fake_imap/`, owned by A until handed off), frontend test files | trails Lane A by one milestone | after each A milestone lands |

**Review gate, every lane, every milestone-sized deliverable.** `brutal-honesty-review` (or `qe-court` for a harder adversarial pass) runs against the diff before it counts as done; the verdict and any remediation items are written to ruflo memory so later lanes or sessions inherit the finding rather than re-discovering it. A SHIP verdict is the bar for merging into `develop`; REMAND/BLOCK sends it back for remediation in the same lane before re-review.

**Ruflo's job here, precisely.** Cross-session memory of decisions and verdicts, and, later, parallel execution of lanes B/D/E once Lane A has produced the contracts they depend on. It does not choose the lane boundaries above — that is this table, fixed by hand. The self-tuning flywheel (ruflo ≥3.24) stays off: this project has fewer than the 12 stored patterns it needs to activate, and nothing here requires it.

**Not decided yet:** the exact adapter module path for Lane B (confirm against the repo before kickoff), and whether Lane A runs as one continuous session or as a fresh swarm agent per milestone with ruflo memory carrying context between them.
