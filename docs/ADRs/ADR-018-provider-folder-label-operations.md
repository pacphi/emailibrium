# ADR-018: Provider-Aware Folder and Label Operations

- **Status**: Accepted
- **Date**: 2026-03-25
- **Updated**: 2026-10-29 — IMAP is now built: single-message moves that never erase other mail, and a label strategy per IMAP provider (mail-agent milestone 6). See section 8.
- **Extends**: DDD-005 (Account Management Domain)

## Context

Email providers have fundamentally different folder/label semantics:

- **Gmail** uses labels (multiple per message, additive). System labels: `INBOX`, `SENT`, `TRASH`, `SPAM`, `STARRED`, `DRAFT`. Custom labels are user-created and can be nested.
- **Outlook** uses folders (exclusive -- a message lives in one folder) plus categories (additive, similar to labels). System folders: `Inbox`, `Drafts`, `Sent Items`, `Deleted Items`, `Archive`, `Junk Email`.
- **IMAP** uses mailbox folders (exclusive). Standard folders: `INBOX`, `Sent`, `Trash`, `Drafts`. Many servers also keep custom keywords on a message (additive, like labels) — whether a mailbox keeps them is announced per mailbox in `PERMANENTFLAGS` (RFC 3501 section 7.1). Proton Mail Bridge shows Proton labels as folders under `Labels/`; Outlook over IMAP keeps no custom keywords.
- **POP3** has no server-side folder concept. Messages are downloaded and deleted from the server; organization is entirely local.

Without a provider-aware abstraction layer, every UI component and API handler would need provider-specific branching logic. This violates the open/closed principle and makes adding new providers disproportionately expensive.

## Decision

### 1. Unified Type System

Introduce a `MoveKind` enum to distinguish the two fundamental operations:

- `MoveKind::Folder` -- exclusive move (message leaves its current container)
- `MoveKind::Label` -- additive tag (message gains an additional classification)

Introduce a `FolderOrLabel` struct as the unified type returned by all providers:

```rust
pub struct FolderOrLabel {
    pub id: String,          // Provider-specific identifier
    pub name: String,        // Human-readable display name
    pub kind: MoveKind,      // Folder or Label
    pub is_system: bool,     // true for provider-defined, false for user-created
}
```

### 2. EmailProvider Trait Extensions

Add `list_folders`, `move_message`, and `star_message` to the `EmailProvider` trait. All three have default implementations that return `Err(ProviderError::NotSupported)` for backward compatibility, so existing providers (POP3, future providers) compile without changes.

```rust
#[async_trait]
pub trait EmailProvider: Send + Sync {
    // ... existing methods ...

    async fn list_folders(&self, account_id: &str) -> Result<Vec<FolderOrLabel>, ProviderError> {
        Err(ProviderError::NotSupported("list_folders"))
    }

    async fn move_message(&self, message_id: &str, target_id: &str, kind: MoveKind) -> Result<(), ProviderError> {
        Err(ProviderError::NotSupported("move_message"))
    }

    async fn star_message(&self, message_id: &str, starred: bool) -> Result<(), ProviderError> {
        Err(ProviderError::NotSupported("star_message"))
    }
}
```

### 3. Gmail Implementation

- `list_folders`: Calls the Gmail Labels API (`GET /gmail/v1/users/me/labels`). Returns both system labels (`INBOX`, `SENT`, `TRASH`, `SPAM`, `STARRED`, `DRAFT`) and user-created labels. All are returned with `kind: MoveKind::Label`.
- `move_message`: Uses the Gmail modify endpoint (`POST /gmail/v1/users/me/messages/{id}/modify`) to add the target label and remove `INBOX` when the intent is a folder-like move. For label-only additions, only the `addLabelIds` field is populated.
- `star_message`: Adds or removes the `STARRED` label via the same modify endpoint.

### 4. Outlook Implementation

- `list_folders`: Combines two Graph API calls -- `GET /me/mailFolders` (returns folders with `kind: MoveKind::Folder`) and `GET /me/outlook/masterCategories` (returns categories with `kind: MoveKind::Label`). Results are merged into a single `Vec<FolderOrLabel>`.
- `move_message`: For `MoveKind::Folder`, calls `POST /me/messages/{id}/move` with `destinationId`. For `MoveKind::Label`, calls `PATCH /me/messages/{id}` to update the `categories` array.
- `star_message`: Calls `PATCH /me/messages/{id}` to set `flag.flagStatus` to `"flagged"` or `"notFlagged"`.

### 5. Provider Resolution Helper

Provider-specific logic for resolving account type to provider implementation is extracted to a shared `provider_helpers.rs` module. This avoids duplicating match-on-provider-type blocks across API handlers for move, star, archive, and delete operations.

### 6. Best-Effort Provider Sync

Archive, star, delete, and move operations follow a best-effort sync pattern:

1. Call the provider API first (Gmail, Outlook, IMAP).
2. If the provider call succeeds, update the local database to reflect the change.
3. If the provider call fails (network error, rate limit, token expired), log the failure and still update the local database. The local state change is not blocked by provider failures.
4. Failed provider operations are queued for retry by the existing sync mechanism.

This ensures the UI remains responsive even when provider APIs are slow or unavailable.

### 7. Frontend Integration

The frontend dynamically loads available folders and labels from `GET /api/v1/emails/labels?accountId=X`. The response is grouped and rendered in the move dropdown as:

1. **System folders** (Inbox, Sent, Trash, etc.) -- displayed first, in a fixed order
2. **Custom labels/folders** -- displayed below system items, sorted alphabetically

The frontend does not need to know which provider the account uses. The `kind` field on each `FolderOrLabel` determines whether the move operation sends `MoveKind::Folder` or `MoveKind::Label` in the request body.

### 8. IMAP Implementation (added 2026-10-29)

`ImapProvider` (`backend/src/email/imap.rs`) implements the trait over IMAP for app-password providers (Gmail, Google Workspace, iCloud, Fastmail, Yahoo, Zoho), Proton Mail Bridge, and Outlook with OAuth (XOAUTH2).

**Moving one message** (`archive_message`, `unarchive_message`, `move_message` with `MoveKind::Folder`, and every folder-based label) goes through one helper that touches no other message:

1. `UID MOVE` when the server advertises `MOVE` (RFC 6851).
2. Otherwise `UID COPY`, flag `\Deleted`, then `UID EXPUNGE` of that one UID when the server advertises `UIDPLUS` (RFC 4315).
3. Otherwise (iCloud is reported to lack `MOVE`) a plain `EXPUNGE`, but only when `UID SEARCH DELETED` shows no other message is marked — checked before copying and again just before the expunge. A plain `EXPUNGE` erases every `\Deleted` message in the mailbox, including ones another mail client flagged.

**Labels** follow a `ProviderProfile` stored per account (`connected_accounts.imap_auth_profile`):

| Profile | Apply a label | Remove a label | Kind/Topic | State |
| --- | --- | --- | --- | --- |
| `Generic`, mailbox keeps keywords | `STORE +FLAGS` keyword | `STORE -FLAGS` | keyword | keyword |
| `Generic`, mailbox refuses keywords | move into a folder named after the label | refused for Kind/Topic (undo is a move back) | folder | not written (ledger only) |
| `ProtonBridge` | `COPY` into `Labels/<label>` (message stays where it is) | delete that copy (`UID EXPUNGE`) | `Labels/` copy | `Labels/` copy |
| `OutlookImap` | move into a folder named after the label | refused for Kind/Topic (undo is a move back) | folder | not written (ledger only) |

A `Generic` mailbox keeps keywords when its `PERMANENTFLAGS` contains `\*`, names the keyword, or is not sent at all (RFC 3501 section 7.1). Every keyword is checked against the RFC 3501 `atom` rule before any `STORE` is sent, so label text can't split into two keywords or inject command text; a label that fails the check is rejected and nothing is written.

A provider-adapter conformance suite (`backend/tests/provider_conformance.rs`) applies the same labels on every profile against an in-process fake IMAP server and checks that undo restores the same mailbox state, and that archive → unarchive does too under each move strategy.

## Consequences

### Positive

- Single UI codebase works identically across Gmail, Outlook, and IMAP accounts
- New providers only need to implement the three trait methods; the API layer and frontend are unchanged
- Optimistic local updates with provider sync keep the UI responsive regardless of provider latency
- The `FolderOrLabel` type is extensible (adding fields like `color`, `parent_id` for nested labels) without breaking existing consumers

### Negative

- Folder semantics are not perfectly represented: Gmail's "move to folder" is internally a label add/remove, which means a Gmail message could appear in multiple "folders" if manipulated outside Emailibrium
- POP3 is not built; its trait default methods return `NotSupported`
- A Kind/Topic label that became a folder (Outlook, or a mailbox that refuses keywords) has no undo path in production code yet: `remove_labels` refuses it and `move_message` only moves out of the account's configured mailbox, so moving the message back out of the label folder needs a provider built for that folder
- Gmail over IMAP uses the `Generic` profile (keywords), not Gmail labels via `X-GM-LABELS`
- On a server with neither `MOVE` nor `UIDPLUS`, a message another client flags as `\Deleted` in the instant between the last check and the plain `EXPUNGE` is lost
- Best-effort sync means local state can temporarily diverge from provider state if retries fail repeatedly

### Neutral

- The `MoveKind` enum may grow in the future (for example, `MoveKind::Category` if a provider distinguishes categories from both folders and labels)
- Provider-specific quirks (Gmail's label nesting, Outlook's well-known folder IDs) are encapsulated within each implementation and do not leak into the unified API

## API Surface

| Method | Endpoint                     | Description                                        |
| ------ | ---------------------------- | -------------------------------------------------- |
| GET    | `/api/v1/emails/labels`      | List folders and labels (`?accountId=X`)           |
| POST   | `/api/v1/emails/:id/move`    | Move email (body: `accountId`, `targetId`, `kind`) |
| POST   | `/api/v1/emails/:id/star`    | Toggle star/flag (provider-aware)                  |
| POST   | `/api/v1/emails/:id/archive` | Archive email (provider-aware)                     |

## Alternatives Considered

### Provider-Specific UI Components

- **Pros**: Exact representation of each provider's semantics, no abstraction leakage
- **Cons**: Triples the frontend code, every new feature must be implemented per-provider, inconsistent UX across accounts
- **Verdict**: Rejected. The abstraction cost is minimal compared to the maintenance cost of per-provider UI code.

### Folder-Only Abstraction (Ignore Labels)

- **Pros**: Simpler model, every provider maps to a flat folder list
- **Cons**: Loses Gmail's multi-label capability entirely, forces Gmail users into a folder mental model that does not match their provider
- **Verdict**: Rejected. The `MoveKind` enum preserves the distinction at minimal complexity cost.

### Synchronous Provider Calls (Block on Failure)

- **Pros**: Local state always matches provider state, no divergence
- **Cons**: UI blocks on slow/failed provider calls, poor UX on flaky networks, operations fail entirely when provider is down
- **Verdict**: Rejected. Best-effort sync with retry is the standard pattern for email clients.
