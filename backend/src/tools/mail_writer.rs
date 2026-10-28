//! `MailWriter` — live, write-capable access to a connected account's real
//! mailbox, for write-capable staff tools (milestone 3).
//!
//! This trait exists only to cross a crate boundary cleanly. `main.rs`
//! declares its own `mod cleanup;` — a second, independent compilation of
//! `src/cleanup/` distinct from this lib crate's `pub mod cleanup { domain;
//! repository; }` (which deliberately does not re-export `orchestrator`:
//! `cleanup::orchestrator::ApplyOrchestrator` and the REST route handlers
//! around it are binary-side concerns). Because Rust gives every crate
//! compilation its own, non-interchangeable copy of a type, a value of
//! `main.rs`'s `cleanup::orchestrator::ApplyOrchestrator` cannot be stored
//! in a struct defined in this lib crate, even though the source is
//! identical.
//!
//! The fix is the standard one for this shape of problem: define the
//! capability as a trait here, in the lib crate, and let the binary provide
//! the implementation, wrapping whatever binary-only types it needs. See
//! `main.rs`'s `OrchestratorMailWriter` for the real implementation, which
//! resolves a live `EmailProvider` via `ApplyOrchestrator`'s existing
//! `provider_factory` — the same hardened, already-wired connection path
//! `AccountWorker` uses for every other apply — rather than reimplementing
//! any provider or connection logic here.

use async_trait::async_trait;
use thiserror::Error;

use crate::email::types::ProviderKind;

#[derive(Debug, Error)]
pub enum MailWriteError {
    #[error("account not found or not connected: {0}")]
    AccountNotFound(String),
    #[error("provider error: {0}")]
    Provider(String),
    /// IMAP UIDs are scoped per folder (`UIDVALIDITY`/`UIDNEXT` per folder —
    /// docs/plan/mail-agent-2026-10.md, "Sync model"). `archive_message`'s
    /// real IMAP implementation does `UID COPY` then `EXPUNGE` across two
    /// folders, so the message gets a *different* UID in the Archive
    /// folder; the UID captured before archiving is stale by the time undo
    /// would use it there, and acting on it risks silently mutating an
    /// unrelated message that happens to hold that UID in the Archive
    /// folder's own namespace. Gmail and Outlook are unaffected (both use
    /// stable, provider-assigned global message IDs that don't change on
    /// archive), so this only refuses IMAP-provider undo. The real fix —
    /// capturing the server's real destination UID from the `COPYUID`
    /// (UIDPLUS) response at archive time, or a Message-ID-based lookup at
    /// undo time — is deliberately not implemented here: it's real
    /// IMAP-protocol work that needs a real server to build and test
    /// against, not something to guess at against a fixture. Deferred as a
    /// named follow-up, not silently skipped.
    #[error(
        "undo is not yet safe for IMAP-connected accounts: IMAP UIDs are per-folder, so the \
         UID captured before archiving no longer identifies the message after the \
         COPY+EXPUNGE that moved it — acting on it could mutate the wrong message. Refusing \
         rather than risking that. (account: {0})"
    )]
    UndoUnsafeForImapUidReuse(String),
}

/// Archive/unarchive a message on the account's real mailbox. Scoped to
/// exactly what milestone 3's write tools need today (era-archive,
/// close-out); label/unlabel is not yet required by any built tool and is
/// deliberately not added speculatively.
#[async_trait]
pub trait MailWriter: Send + Sync {
    async fn archive(
        &self,
        account_id: &str,
        provider_message_id: &str,
    ) -> Result<(), MailWriteError>;

    async fn unarchive(
        &self,
        account_id: &str,
        provider_message_id: &str,
    ) -> Result<(), MailWriteError>;
}

/// Whether undo is currently safe for a given provider — see
/// [`MailWriteError::UndoUnsafeForImapUidReuse`] for the full rationale.
/// A pure, directly-testable policy check: the real `MailWriter`
/// implementation (`main.rs`'s `OrchestratorMailWriter`) calls this before
/// issuing any real unarchive request.
pub fn check_undo_supported(kind: ProviderKind, account_id: &str) -> Result<(), MailWriteError> {
    if kind == ProviderKind::Imap {
        return Err(MailWriteError::UndoUnsafeForImapUidReuse(
            account_id.to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_is_refused_for_imap_accounts() {
        let err = check_undo_supported(ProviderKind::Imap, "acct-1").unwrap_err();
        assert!(matches!(err, MailWriteError::UndoUnsafeForImapUidReuse(ref a) if a == "acct-1"));
    }

    #[test]
    fn undo_is_permitted_for_gmail_and_outlook_which_use_stable_global_ids() {
        assert!(check_undo_supported(ProviderKind::Gmail, "acct-1").is_ok());
        assert!(check_undo_supported(ProviderKind::Outlook, "acct-1").is_ok());
    }
}
