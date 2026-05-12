# ADR-010: Ingest-Tag-Archive Pipeline Strategy

- **Status**: Proposed
- **Date**: 2026-03-23
- **Context**: The plan's core strategy is "Gmail becomes a dumb store, Emailibrium is the smart interface." New emails are classified, labeled in Gmail, and archived. The research paper does not specifically critique this, but it has significant operational implications around safety, undo capability, and provider abstraction.
- **Decision**: Implement ingest-tag-archive with configurable timing, safety mechanisms, and undo capability.
- **Consequences**: Auto-archive is powerful but risky if classification is wrong. Confidence threshold + sender whitelist + undo buffer provide safety layers. Rate limiting prevents Gmail API quota exhaustion. First-run safety prevents accidental mass-archive.
- **Alternatives Considered**: No archiving (just label, user archives manually -- safer but defeats inbox zero promise), server-side rules (Gmail filters -- less intelligent, cannot use vector classification), email forwarding (route all email through Emailibrium -- too invasive).
- **Research References**: Gmail API quota limits and batch operation documentation; push notification architecture via Gmail watch(); incremental sync via history.list().

## Detailed Design

### Pipeline Flow

Gmail `watch()` push --> fetch --> parse --> embed --> classify --> apply labels --> archive

### Archive Timing (Per-Account Configurable)

| Mode              | Behavior                                                       | Target User        |
| ----------------- | -------------------------------------------------------------- | ------------------ |
| Instant           | Archive within 2s of classification. True zero inbox.          | Power users        |
| Delayed (default) | Archive after 60s. Allows mobile Gmail notification to appear. | Most users         |
| Manual            | No auto-archive. User marks "Done" in Emailibrium.             | Conservative users |

### Gmail Label Strategy

- **Prefix**: "EM/" (configurable per account).
- **Categories**: `EM/{category}` (e.g., EM/Work, EM/Finance).
- **Clusters**: `EM/{cluster_name}` (e.g., EM/Project Alpha).
- Labels created automatically via Gmail API `labels.create`.
- Maximum 50 auto-created labels to prevent label explosion.

### Safety Mechanisms

1. **Classification confidence threshold**: Only auto-archive if confidence >= 0.7. Below threshold, leave in inbox and flag for review.
2. **Sender whitelist**: Emails from starred/VIP senders are never auto-archived.
3. **Undo buffer**: 5-minute window where the user can "unarchive" via Emailibrium UI (calls `messages.modify` to re-add INBOX label).
4. **First-run safety**: During initial sync, only tag -- do not archive. Archive only after the user reviews suggestions and clicks "Execute Cleanup."
5. **Rate limiting**: Max 1000 archive operations per 10 minutes (Gmail API quota protection).

### Offline Resilience

- If Emailibrium is offline, emails accumulate in Gmail INBOX normally.
- On reconnect: `history.list(startHistoryId=...)` catches all missed emails.
- Incremental sync, not full re-sync.

### Batch Operations for Initial Sync

- Gmail `batchModify`: 1000 emails per batch call.
- 10,000 emails archived in ~5 seconds.

### Provider Abstraction

| Provider | Archive Mechanism                    |
| -------- | ------------------------------------ |
| Gmail    | `removeLabelIds: ["INBOX"]`          |
| Outlook  | Move to Archive folder via Graph API |
| IMAP     | `MOVE` to Archive mailbox            |

All providers abstracted behind a `ProviderArchive` trait for uniform handling.

### Rollback Capability

- Settings --> Danger Zone --> "Unarchive all emails" (restore INBOX label to all EM/-labeled emails).
- Settings --> Danger Zone --> "Remove all Emailibrium labels" (delete all EM/ labels from Gmail).

## Options Considered

### Option 1: Label Only, No Archive

- **Pros**: Safest option. No risk of missing emails. Gmail inbox remains unchanged.
- **Cons**: Defeats the core inbox zero promise. Users still see all emails in their inbox. Labels alone do not reduce cognitive load.

### Option 2: Server-Side Gmail Filters

- **Pros**: Runs without Emailibrium being open. Native Gmail feature.
- **Cons**: Limited to static rules. Cannot use vector-based classification. No confidence scoring. No undo buffer. Difficult to manage at scale (hundreds of filter rules).

### Option 3: Email Forwarding Through Emailibrium

- **Pros**: Full control over email flow. Could intercept before inbox delivery.
- **Cons**: Too invasive. Changes the user's email address or requires forwarding configuration. Single point of failure -- if Emailibrium is down, emails are delayed. Privacy concerns with routing through any intermediary.

### Option 4: Ingest-Tag-Archive with Safety Mechanisms (Selected)

- **Pros**: Achieves inbox zero while preserving Gmail as the canonical store. Safety mechanisms (confidence threshold, sender whitelist, undo buffer, first-run protection) mitigate classification errors. Provider abstraction enables future multi-provider support. Rollback capability provides an escape hatch.
- **Cons**: Depends on Gmail API availability and quota. Classification errors below the confidence threshold still reach inbox. 60s delay in default mode means inbox is not truly instant. Complexity of managing labels across multiple providers.
