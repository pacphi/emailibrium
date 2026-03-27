# DDD-008: Email Operations Domain

| Field   | Value            |
| ------- | ---------------- |
| Status  | Accepted         |
| Date    | 2026-03-25       |
| Type    | Core Domain      |
| Context | Email Operations |

## Overview

The Email Operations bounded context handles the full lifecycle of email messages after they are synced from providers: reading, moving between folders/labels, starring, archiving, deleting, and thread grouping. It sits between the Account Management context (which owns provider connections and OAuth tokens) and the Email Intelligence context (which handles embeddings and categorization). This is a **core domain** because email manipulation is the primary user-facing capability that all other contexts depend on for state.

## Strategic Classification

| Aspect              | Value                                                         |
| ------------------- | ------------------------------------------------------------- |
| Domain type         | Core                                                          |
| Investment priority | High (primary email manipulation and state management)        |
| Complexity driver   | Provider-specific API translation, idempotent sync            |
| Change frequency    | Medium (new providers, new operations)                        |
| Risk                | Data loss on delete, sync drift, provider API inconsistencies |

---

## Aggregates

### 1. EmailMessageAggregate

Manages the lifecycle and state of a single email message within Emailibrium.

**Root Entity: Email**

| Field               | Type             | Description                                       |
| ------------------- | ---------------- | ------------------------------------------------- |
| id                  | EmailId          | Provider's message ID (idempotent sync key)       |
| account_id          | AccountId        | The account this email belongs to                 |
| provider            | Provider         | Gmail, Outlook, Imap, or Pop3                     |
| message_id          | String           | RFC 2822 Message-ID header                        |
| thread_id           | Option\<String\> | Provider thread/conversation ID                   |
| subject             | String           | Email subject line                                |
| from_addr           | String           | Sender email address                              |
| from_name           | Option\<String\> | Sender display name                               |
| to_addrs            | Vec\<String\>    | Recipient email addresses                         |
| cc_addrs            | Vec\<String\>    | CC recipient email addresses                      |
| received_at         | DateTime         | When the email was received                       |
| body_text           | Option\<String\> | Plain-text body                                   |
| body_html           | Option\<String\> | HTML body                                         |
| labels              | String           | Comma-separated current folder/label state        |
| is_read             | bool             | Whether the email has been read                   |
| is_starred          | bool             | Whether the email is starred/flagged              |
| has_attachments     | bool             | Whether the email has attachments                 |
| embedding_status    | EmbeddingStatus  | Embedding pipeline state                          |
| category            | Option\<String\> | Assigned category (from Email Intelligence)       |
| category_confidence | Option\<f32\>    | Confidence score of category assignment (0.0-1.0) |

**Invariants:**

- Each email belongs to exactly one account.
- `id` is the provider's message ID, serving as the idempotent sync key. Re-syncing the same message is a no-op.
- `labels` is a comma-separated string representing the current folder/label state in the provider.
- `embedding_status` transitions follow a strict state machine: `pending` --> `embedded` --> `stale`. Transitioning backward (e.g., `embedded` --> `pending`) is forbidden; use `stale` to trigger re-embedding.
- `category` and `category_confidence` are set together or both null. Setting one without the other is invalid.
- Deleting an email is a soft-delete (marks as deleted in provider) unless the user explicitly requests permanent deletion.

**Commands:**

- `MarkRead { email_id }` -- marks an email as read
- `MarkUnread { email_id }` -- marks an email as unread
- `StarEmail { email_id }` -- stars/flags an email
- `UnstarEmail { email_id }` -- removes star/flag from an email
- `MoveEmail { email_id, target_id, kind }` -- moves an email to a folder or applies a label
- `ArchiveEmail { email_id }` -- archives an email (provider-specific behavior)
- `DeleteEmail { email_id, permanent }` -- deletes an email (soft or permanent)
- `UpdateEmbeddingStatus { email_id, status }` -- transitions embedding status
- `AssignCategory { email_id, category, confidence }` -- assigns a category from Email Intelligence

### 2. ThreadAggregate

Groups related emails into a conversation thread. This is a derived aggregate, not persisted independently.

**Value Object: EmailThread**

| Field         | Type           | Description                                  |
| ------------- | -------------- | -------------------------------------------- |
| thread_id     | String         | Provider thread/conversation ID              |
| emails        | Vec\<EmailId\> | Ordered list of email IDs in the thread      |
| subject       | String         | Thread subject (from the first email)        |
| participants  | Vec\<String\>  | All unique email addresses in the thread     |
| last_activity | DateTime       | Timestamp of the most recent email in thread |

**Invariants:**

- A thread is derived from emails sharing the same `thread_id`. It has no independent lifecycle.
- Thread subject is taken from the earliest email in the thread.
- Participants are the union of all from/to/cc addresses across thread emails.
- Threads are rebuilt on each query; they are not stored as separate entities.

### 3. FolderOrLabelAggregate

Represents a provider-specific organizational unit, unified through the `MoveKind` abstraction.

**Value Object: FolderOrLabel**

| Field     | Type     | Description                                        |
| --------- | -------- | -------------------------------------------------- |
| id        | String   | Provider-specific folder/label ID                  |
| name      | String   | Display name                                       |
| kind      | MoveKind | Folder or Label                                    |
| is_system | bool     | Whether this is a system folder (Inbox, Sent, etc) |

**Invariants:**

- System folders/labels cannot be deleted or renamed.
- Label names must be unique within an account.
- Folder hierarchy is flattened to a single level within Emailibrium (nested folders are represented with path separators).

---

## Value Objects

### EmbeddingStatus

```rust
enum EmbeddingStatus {
    Pending,    -- Email synced but not yet embedded
    Embedded,   -- Embedding generated and stored in vector DB
    Stale,      -- Email content or model changed; needs re-embedding
}
```

### MoveKind

```rust
enum MoveKind {
    Folder,  -- Mutually exclusive container (Inbox, Archive, Trash)
    Label,   -- Non-exclusive tag (can have multiple simultaneously)
}
```

---

## Domain Events

| Event                  | Fields                           | Published When                           |
| ---------------------- | -------------------------------- | ---------------------------------------- |
| EmailSynced            | email_id, account_id, thread_id  | New email ingested from provider sync    |
| EmailMoved             | email_id, target_id, kind        | Email moved to a folder or label applied |
| EmailStarred           | email_id, starred (bool)         | Email starred or unstarred               |
| EmailArchived          | email_id                         | Email archived in provider               |
| EmailDeleted           | email_id, permanent (bool)       | Email deleted (soft or permanent)        |
| EmailRead              | email_id, is_read (bool)         | Email marked as read or unread           |
| EmailCategorized       | email_id, category, confidence   | Category assigned by Email Intelligence  |
| EmbeddingStatusChanged | email_id, old_status, new_status | Embedding status transitioned            |

### Event Consumers

| Event                  | Consumed By        | Purpose                                     |
| ---------------------- | ------------------ | ------------------------------------------- |
| EmailSynced            | Email Intelligence | Triggers embedding generation for new email |
| EmailSynced            | Rules              | Triggers rule evaluation against new email  |
| EmailCategorized       | UI/API             | Updates displayed category in the interface |
| EmbeddingStatusChanged | Email Intelligence | Re-embeds stale emails                      |
| EmailDeleted           | Email Intelligence | Removes embedding from vector store         |

---

## Domain Services

### EmailOperationsService

Orchestrates email actions across the provider ACL and local state.

**Responsibilities:**

- Coordinates local state updates with provider-side API calls.
- Ensures consistency: if a provider API call fails, local state is not updated.
- Handles batch operations (e.g., archive 50 emails) with per-item error reporting.
- Respects provider rate limits via exponential backoff.

### ThreadBuilder

Constructs `EmailThread` value objects from stored emails.

**Responsibilities:**

- Groups emails by `thread_id` and orders them by `received_at`.
- Extracts unique participants from all emails in a thread.
- Computes `last_activity` from the most recent email.
- Returns threads sorted by `last_activity` descending (most recent first).

---

## Anti-Corruption Layer

The `EmailProvider` trait translates provider-specific APIs into a unified interface for email operations. This extends the trait defined in DDD-005 (Account Management) with operation-specific methods.

### EmailOperationsProvider Trait

```rust
trait EmailOperationsProvider {
    fn mark_read(email_id: &str, read: bool) -> Result<()>;
    fn star(email_id: &str, starred: bool) -> Result<()>;
    fn move_email(email_id: &str, target_id: &str, kind: MoveKind) -> Result<()>;
    fn archive(email_id: &str) -> Result<()>;
    fn delete(email_id: &str, permanent: bool) -> Result<()>;
    fn list_folders() -> Result<Vec<FolderOrLabel>>;
    fn create_folder(name: &str) -> Result<String>;
}
```

### Provider-Specific Translations

**MoveKind mapping:**

| Operation   | Gmail                                     | Outlook                                    | IMAP                         | POP3                   |
| ----------- | ----------------------------------------- | ------------------------------------------ | ---------------------------- | ---------------------- |
| Folder move | `addLabelIds` + `removeLabelIds(INBOX)`   | `POST /messages/{id}/move`                 | `COPY` + `DELETE`            | Local-only (no server) |
| Label add   | `addLabelIds` only                        | `PATCH categories`                         | Not supported (emulated)     | Local-only (no server) |
| Archive     | `removeLabelIds(INBOX)`                   | `POST /messages/{id}/move` to Archive      | `COPY` to Archive + `DELETE` | Local-only (no server) |
| Delete      | `addLabelIds(TRASH)` or `messages.delete` | `DELETE /messages/{id}` or move to Deleted | `DELETE` flag + `EXPUNGE`    | Local-only (no server) |
| Star        | `addLabelIds(STARRED)`                    | `PATCH {flag: {flagStatus: "flagged"}}`    | `STORE +FLAGS (\Flagged)`    | Local-only (no server) |
| Mark read   | `removeLabelIds(UNREAD)`                  | `PATCH {isRead: true}`                     | `STORE +FLAGS (\Seen)`       | Local-only (no server) |

**POP3 limitations:**

POP3 does not support server-side operations (move, label, archive, delete). All operations are applied locally only. The `Pop3OperationsProvider` implementation logs a warning and updates local state without making provider API calls.

---

## Context Map

### Upstream Dependencies

| Context            | Relationship      | What It Provides                              |
| ------------------ | ----------------- | --------------------------------------------- |
| Account Management | Customer/Supplier | OAuth tokens, provider config, account status |

### Downstream Consumers

| Context            | Relationship       | What Email Operations Publishes                        |
| ------------------ | ------------------ | ------------------------------------------------------ |
| Email Intelligence | Published Language | EmailSynced triggers embedding; EmailDeleted cleans up |
| Rules              | Published Language | EmailSynced triggers rule evaluation                   |
| UI/API             | Published Language | All events consumed for display and interaction        |

---

## Repositories

### EmailRepository

SQLite-backed via `sqlx`, operating on the `emails` table.

**Operations:**

- `find_by_id(email_id)` -- retrieve a single email
- `find_by_account(account_id, pagination)` -- list emails for an account
- `find_by_thread(thread_id)` -- retrieve all emails in a thread
- `upsert(email)` -- insert or update (idempotent sync)
- `update_labels(email_id, labels)` -- update label state
- `update_flags(email_id, is_read, is_starred)` -- update read/star state
- `update_category(email_id, category, confidence)` -- update category assignment
- `update_embedding_status(email_id, status)` -- update embedding status
- `delete(email_id)` -- remove from local store
- `search(account_id, query, filters)` -- filtered search with pagination

### SyncStateRepository

Tracks per-account sync progress. Shared with the Account Management context via the `sync_state` table.

**Operations:**

- `get_by_account(account_id)` -- retrieve sync state
- `update_history_id(account_id, history_id)` -- update incremental sync cursor
- `increment_synced_count(account_id, count)` -- update total emails synced
- `record_failure(account_id, error)` -- record sync failure

---

## Ubiquitous Language

| Term                     | Definition                                                                              |
| ------------------------ | --------------------------------------------------------------------------------------- |
| **Email**                | A synced email message with its metadata, body, and operational state                   |
| **Thread**               | A group of related emails sharing a thread ID, displayed as a conversation              |
| **Folder**               | A mutually exclusive email container (Inbox, Archive, Trash). An email is in one folder |
| **Label**                | A non-exclusive tag applied to emails. An email can have multiple labels simultaneously |
| **MoveKind**             | The abstraction that unifies provider-specific folder/label concepts                    |
| **Archive**              | The action of removing an email from the inbox without deleting it                      |
| **Embedding status**     | The state of an email's vector embedding (pending, embedded, stale)                     |
| **Idempotent sync**      | Syncing the same email multiple times produces the same result (upsert by provider ID)  |
| **Soft delete**          | Moving an email to trash (recoverable) vs. permanent deletion (irrecoverable)           |
| **Provider translation** | The ACL process of converting a unified operation into a provider-specific API call     |

---

## Boundaries

- This context does NOT manage account connections, OAuth flows, or credentials. That belongs to **Account Management**.
- This context does NOT generate embeddings or run categorization models. That belongs to **Email Intelligence**.
- This context does NOT evaluate automation rules. That belongs to **Rules**.
- This context does NOT manage search queries or vector similarity search. That belongs to **Search**.
- This context DOES own:
  - Email CRUD and state management (read, star, labels, archive, delete)
  - Thread grouping and display
  - Folder/label abstraction across providers
  - Provider-specific operation translation (ACL)
  - Embedding status tracking (but not embedding generation)
  - Category storage (but not category computation)
  - Idempotent sync storage via `EmailRepository`
