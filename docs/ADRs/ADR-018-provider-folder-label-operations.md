# ADR-018: Provider-Aware Folder and Label Operations

- **Status**: Accepted
- **Date**: 2026-03-25
- **Extends**: DDD-005 (Account Management Domain)

## Context

Email providers have fundamentally different folder/label semantics:

- **Gmail** uses labels (multiple per message, additive). System labels: `INBOX`, `SENT`, `TRASH`, `SPAM`, `STARRED`, `DRAFT`. Custom labels are user-created and can be nested.
- **Outlook** uses folders (exclusive -- a message lives in one folder) plus categories (additive, similar to labels). System folders: `Inbox`, `Drafts`, `Sent Items`, `Deleted Items`, `Archive`, `Junk Email`.
- **IMAP** uses mailbox folders (exclusive). Standard folders: `INBOX`, `Sent`, `Trash`, `Drafts`.
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

## Consequences

### Positive

- Single UI codebase works identically across Gmail, Outlook, and IMAP accounts
- New providers only need to implement the three trait methods; the API layer and frontend are unchanged
- Optimistic local updates with provider sync keep the UI responsive regardless of provider latency
- The `FolderOrLabel` type is extensible (adding fields like `color`, `parent_id` for nested labels) without breaking existing consumers

### Negative

- Folder semantics are not perfectly represented: Gmail's "move to folder" is internally a label add/remove, which means a Gmail message could appear in multiple "folders" if manipulated outside Emailibrium
- IMAP and POP3 implementations are not yet built; trait default methods return `NotSupported`
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
