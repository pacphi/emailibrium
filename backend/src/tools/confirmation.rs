//! Confirmation gate for `requires_confirmation` tools, enforced in
//! [`super::registry::ToolRegistry::dispatch`] itself — not declared in
//! `config/tools.yaml` and left unchecked, which was the gap found in
//! milestone 3 (nothing in the MCP dispatch path read that field at all).
//!
//! A denied call is handed back only a `confirmation_id` — never a secret
//! that redeems itself. Approving a pending confirmation requires a
//! *separate* call to a *separate* endpoint
//! (`POST /api/v1/tools/confirmations/{id}/approve`, wired in the binary),
//! so a caller cannot turn its own denial into an approval inside the same
//! round trip the way a self-contained token would allow.
//!
//! **Honest limit, not fixed here:** this engine issues one shared
//! credential to every caller (`middleware::local_auth`'s single
//! `JWT_SECRET`-derived bearer/session) — there is no credential that is
//! provably "a human, not the calling agent". Anything holding the bearer
//! can also call the approval endpoint. What this *does* buy: the exploit
//! shape changes from "one self-contained call denies-and-redeems itself,
//! fully scriptable, invisible to anyone" to "a second, separate,
//! audited call is required" — real friction and real visibility, not
//! cryptographic separation of principals. True agent-cannot-self-approve
//! separation needs a channel the calling agent cannot drive at all (e.g.
//! MCP elicitation surfaced by the *client application* around the model,
//! or a human-only credential distinct from the engine's one shared
//! secret) — tracked as a follow-up, not attempted here.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

/// How long an unapproved confirmation stays pending before it must be
/// re-requested. Short enough that a stale approval can't be sprung much
/// later on a changed set of arguments.
const PENDING_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
struct PendingEntry {
    tool_name: String,
    /// Hash of the exact arguments this confirmation was issued for — an
    /// approval only ever satisfies a retry with byte-identical arguments,
    /// never "the same tool, whatever the arguments turn out to be".
    args_fingerprint: blake3::Hash,
    approved: bool,
    consumed: bool,
    created_at: Instant,
}

/// Shared, process-wide store of pending/approved tool confirmations.
///
/// Owned by [`super::registry::ToolRegistry`] exactly like the rate limiter
/// it sits beside, so one store backs every caller (MCP, chat, tests) and a
/// confirmation issued to one cannot be approved by resetting another.
pub struct ConfirmationStore {
    entries: Mutex<HashMap<String, PendingEntry>>,
}

/// What approving a confirmation id can fail with.
#[derive(Debug, PartialEq, Eq)]
pub enum ApproveError {
    NotFound,
    Expired,
}

impl ConfirmationStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn fingerprint(tool_name: &str, args: &Value) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(tool_name.as_bytes());
        hasher.update(b"\0");
        hasher.update(args.to_string().as_bytes());
        hasher.finalize()
    }

    fn sweep_expired(entries: &mut HashMap<String, PendingEntry>) {
        let now = Instant::now();
        entries.retain(|_, e| now.duration_since(e.created_at) < PENDING_TTL);
    }

    /// Check whether `(tool_name, args)` already has an approved,
    /// not-yet-consumed, not-expired confirmation. If so, consumes it
    /// (single-use) and returns `Ok(())` — the caller may proceed. If not,
    /// issues (or reuses) a pending confirmation id for this exact call and
    /// returns it as the thing the caller must get approved.
    ///
    /// Returns `Err(confirmation_id)` on every path that does not clear the
    /// gate — the id alone grants nothing; it only names what a human
    /// (through the separate approval endpoint) would need to approve.
    pub fn check_and_consume(&self, tool_name: &str, args: &Value) -> Result<(), String> {
        let fingerprint = Self::fingerprint(tool_name, args);
        let mut entries = self
            .entries
            .lock()
            .expect("confirmation store lock poisoned");
        Self::sweep_expired(&mut entries);

        if let Some((id, entry)) = entries
            .iter_mut()
            .find(|(_, e)| e.tool_name == tool_name && e.args_fingerprint == fingerprint)
        {
            if entry.approved && !entry.consumed {
                entry.consumed = true;
                return Ok(());
            }
            if entry.consumed {
                // Already spent — a fresh identical call needs a fresh
                // confirmation, not a replay of one already used.
                let new_id = Self::new_id();
                let id_out = new_id.clone();
                entries.insert(
                    new_id,
                    PendingEntry {
                        tool_name: tool_name.to_string(),
                        args_fingerprint: fingerprint,
                        approved: false,
                        consumed: false,
                        created_at: Instant::now(),
                    },
                );
                return Err(id_out);
            }
            return Err(id.clone());
        }

        let id = Self::new_id();
        entries.insert(
            id.clone(),
            PendingEntry {
                tool_name: tool_name.to_string(),
                args_fingerprint: fingerprint,
                approved: false,
                consumed: false,
                created_at: Instant::now(),
            },
        );
        Err(id)
    }

    /// Approve a pending confirmation by id. Called only from the separate
    /// approval endpoint — never from anything in the MCP dispatch path, so
    /// a denied caller cannot reach this through the call that was denied.
    pub fn approve(&self, id: &str) -> Result<(), ApproveError> {
        let mut entries = self
            .entries
            .lock()
            .expect("confirmation store lock poisoned");
        Self::sweep_expired(&mut entries);
        match entries.get_mut(id) {
            Some(entry) if !entry.consumed => {
                entry.approved = true;
                Ok(())
            }
            Some(_) => Err(ApproveError::NotFound), // consumed — treat as gone
            None => Err(ApproveError::Expired),
        }
    }

    fn new_id() -> String {
        use rand::Rng;
        let mut bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        format!("confirm_{}", hex_encode(&bytes))
    }
}

impl Default for ConfirmationStore {
    fn default() -> Self {
        Self::new()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_fresh_call_is_denied_and_issues_an_id_that_is_not_itself_a_secret() {
        let store = ConfirmationStore::new();
        let err = store
            .check_and_consume("era_archive_apply", &json!({"batch_key": "abc"}))
            .unwrap_err();
        assert!(err.starts_with("confirm_"));
    }

    #[test]
    fn the_denied_caller_cannot_turn_its_own_denial_into_an_approval_by_retrying() {
        let store = ConfirmationStore::new();
        let args = json!({"batch_key": "abc"});
        // The denied caller only ever has the error string (the id) — there
        // is no field in it that, fed back in, satisfies the gate.
        let id = store
            .check_and_consume("era_archive_apply", &args)
            .unwrap_err();
        // Retrying the exact same call, exact same args, with nothing else
        // done, must still be denied: the id alone is not an approval.
        let second = store
            .check_and_consume("era_archive_apply", &args)
            .unwrap_err();
        assert_eq!(id, second, "same pending confirmation, not a new one");
        // Even explicitly passing the id back as if it were a credential
        // (the shape of the original, broken design) must not help — this
        // store's API has no "redeem with id" path at all, only approve()
        // from the separate endpoint, so there is nothing to call.
    }

    #[test]
    fn only_a_separate_approve_call_lets_the_matching_retry_through() {
        let store = ConfirmationStore::new();
        let args = json!({"batch_key": "abc"});
        let id = store
            .check_and_consume("era_archive_apply", &args)
            .unwrap_err();

        store.approve(&id).expect("approval endpoint call succeeds");

        // Now the identical call succeeds, exactly once.
        store
            .check_and_consume("era_archive_apply", &args)
            .expect("approved call now proceeds");
    }

    #[test]
    fn approval_is_single_use() {
        let store = ConfirmationStore::new();
        let args = json!({"batch_key": "abc"});
        let id = store
            .check_and_consume("era_archive_apply", &args)
            .unwrap_err();
        store.approve(&id).unwrap();
        store.check_and_consume("era_archive_apply", &args).unwrap();

        // A second identical call after the approval was spent is denied
        // again, with a fresh id — the old approval cannot be replayed.
        let err = store
            .check_and_consume("era_archive_apply", &args)
            .unwrap_err();
        assert_ne!(err, id, "a spent confirmation is not reusable");
    }

    #[test]
    fn approval_only_satisfies_the_exact_arguments_it_was_issued_for() {
        let store = ConfirmationStore::new();
        let id = store
            .check_and_consume("era_archive_apply", &json!({"batch_key": "abc"}))
            .unwrap_err();
        store.approve(&id).unwrap();

        // Different arguments, even for the same tool, are a different
        // fingerprint and were never approved.
        let err = store.check_and_consume("era_archive_apply", &json!({"batch_key": "xyz"}));
        assert!(err.is_err());
    }

    #[test]
    fn approving_an_unknown_id_fails_closed() {
        let store = ConfirmationStore::new();
        assert_eq!(
            store.approve("confirm_doesnotexist"),
            Err(ApproveError::Expired)
        );
    }
}
