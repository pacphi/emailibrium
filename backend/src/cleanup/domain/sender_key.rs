//! Sender key: `local-part@registrable-domain` (docs/plan/mail-agent-2026-10.md §7).
//!
//! A single domain carries several Kinds in practice (a bank's `account@`
//! vs `webinar@`; Amazon's five event addresses), so the sender key is
//! local-part-plus-domain, never domain alone — and the domain half must be
//! the *registrable* domain (public suffix list applied), not the raw host,
//! so `notifications.github.com` and `github.com` key the same way a human
//! would read them.
//!
//! Plus-addressing (`user+tag@domain`) and bare display names normalize to
//! the same key as the underlying address.

use std::fmt;

/// `local-part@registrable-domain`, lower-cased, plus-tag stripped.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SenderKey(String);

impl SenderKey {
    /// Derive a sender key from a raw address (or `"Display Name <addr>"`,
    /// or occasionally a bare display name when no address was parseable —
    /// see `ImapProvider::parse_fetched`'s fallback). Returns `None` for
    /// input with no `@`, since there is no domain to key on.
    pub fn from_address(raw: &str) -> Option<Self> {
        let addr = extract_address(raw);
        let (local, domain) = addr.rsplit_once('@')?;
        if local.is_empty() || domain.is_empty() {
            return None;
        }

        let local = strip_plus_tag(local).to_ascii_lowercase();
        let domain = registrable_domain(domain).to_ascii_lowercase();

        Some(Self(format!("{local}@{domain}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SenderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Pull the address out of `"Display Name <addr@host>"`, or return the input
/// unchanged if there's no angle-bracket form.
fn extract_address(raw: &str) -> &str {
    if let Some(start) = raw.find('<') {
        if let Some(end) = raw[start..].find('>') {
            return &raw[start + 1..start + end];
        }
    }
    raw
}

/// `user+tag@...` -> `user@...`. Only the first `+` matters; everything from
/// it to the end of the local-part is the tag.
fn strip_plus_tag(local: &str) -> &str {
    match local.find('+') {
        Some(idx) => &local[..idx],
        None => local,
    }
}

/// Registrable domain via the Public Suffix List (the `psl` crate, bundled
/// at compile time — no network fetch, matching this engine's local-first
/// guarantee). Falls back to the input domain unchanged if the PSL can't
/// parse it (e.g. a bare IP or single-label host in a test fixture), so a
/// malformed domain still produces *a* key rather than no key at all.
fn registrable_domain(domain: &str) -> &str {
    psl::domain_str(domain).unwrap_or(domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_address() {
        assert_eq!(
            SenderKey::from_address("alice@example.com")
                .unwrap()
                .as_str(),
            "alice@example.com"
        );
    }

    #[test]
    fn display_name_form() {
        assert_eq!(
            SenderKey::from_address("Alice Smith <alice@example.com>")
                .unwrap()
                .as_str(),
            "alice@example.com"
        );
    }

    #[test]
    fn plus_addressing_normalizes() {
        assert_eq!(
            SenderKey::from_address("alice+newsletter@example.com")
                .unwrap()
                .as_str(),
            "alice@example.com"
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(
            SenderKey::from_address("Alice@Example.COM")
                .unwrap()
                .as_str(),
            "alice@example.com"
        );
    }

    #[test]
    fn subdomain_collapses_to_registrable_domain() {
        // GitHub notifications come from notifications.github.com; the key
        // should match a message from github.com itself.
        assert_eq!(
            SenderKey::from_address("mention@notifications.github.com")
                .unwrap()
                .as_str(),
            "mention@github.com"
        );
    }

    #[test]
    fn same_domain_different_local_part_is_a_different_key() {
        // The plan's own motivating example: a bank's account@ (protected
        // notification) vs webinar@ (subscription) must not collide.
        let account = SenderKey::from_address("account@examplebank.com").unwrap();
        let webinar = SenderKey::from_address("webinar@examplebank.com").unwrap();
        assert_ne!(account, webinar);
    }

    #[test]
    fn no_at_sign_returns_none() {
        assert_eq!(SenderKey::from_address("Not An Address"), None);
    }

    #[test]
    fn empty_local_part_returns_none() {
        assert_eq!(SenderKey::from_address("@example.com"), None);
    }
}
