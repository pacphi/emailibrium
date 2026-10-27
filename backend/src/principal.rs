//! The one local principal every tool handler and cleanup-plan query resolves to.
//!
//! `middleware::local_auth` establishes that a request carries the right
//! credential; it does not say *who* the caller is, because there is only
//! ever one real caller on this engine (`local_auth`'s own doc comment: "one
//! local principal, not multi-tenant authorization"). Before this module,
//! `user_id` was a plain string the CALLER supplied in a request — nothing
//! checked it against the credential, so a caller could claim to be anyone.
//! `Principal` closes that gap by being the only thing a tool handler or
//! cleanup-plan query can use for "whose data is this": derived once, from
//! the engine's own identity, never from a request field.
//!
//! The `user_id` columns already in `cleanup_plans`/`cleanup_audit_log`
//! (migrations 024/025) are multi-tenancy groundwork (ADR-031) nobody has
//! wired to verified identity yet. Today there is exactly one principal in
//! production; tests use a second, synthetic one to prove the scoping
//! mechanism actually filters, not to model a real second user.

use crate::tools::readonly::validate_user_id;

/// A validated caller identity. Always constructed from a fixed, known-good
/// value in production (`Principal::local_operator`); tests may construct a
/// second, distinct value to prove cross-principal scoping works.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Principal(String);

impl Principal {
    /// The one principal every production request resolves to.
    pub fn local_operator() -> Self {
        Self::new("local-operator").expect("the fixed local-operator id is always valid")
    }

    /// A validated principal from an arbitrary string. Exists for tests that
    /// need a second, distinct identity to prove isolation; production code
    /// should reach for [`Principal::local_operator`] instead of calling this.
    pub fn new(id: impl Into<String>) -> Result<Self, String> {
        let id = id.into();
        validate_user_id(&id)?;
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Principal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_operator_is_stable_and_valid() {
        assert_eq!(Principal::local_operator(), Principal::local_operator());
        assert_eq!(Principal::local_operator().as_str(), "local-operator");
    }

    #[test]
    fn new_rejects_the_same_inputs_validate_user_id_rejects() {
        assert!(Principal::new("../etc/passwd").is_err());
        assert!(Principal::new("").is_err());
        assert!(Principal::new("someone-else").is_ok());
    }

    #[test]
    fn two_principals_constructed_differently_are_not_equal() {
        assert_ne!(
            Principal::local_operator(),
            Principal::new("someone-else").unwrap()
        );
    }
}
