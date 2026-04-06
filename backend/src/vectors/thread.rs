//! Email thread awareness for search and RAG (ADR-029).
//!
//! Provides thread key derivation from email headers, thread collapsing
//! in search results, and thread expansion for context building.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Trait for items that may belong to a thread.
pub trait HasThreadKey {
    /// The thread key for this item, if known.
    fn thread_key(&self) -> Option<&str>;
    /// A unique identifier for this item (e.g. email ID).
    fn id(&self) -> &str;
}

/// Trait for items that carry a relevance score.
pub trait HasScore {
    fn score(&self) -> f32;
}

// ---------------------------------------------------------------------------
// Thread key derivation
// ---------------------------------------------------------------------------

/// Extract the first Message-ID from a `References` header value.
///
/// The References header contains space- or comma-separated Message-IDs
/// enclosed in angle brackets.  The **first** one is the thread root.
///
/// ```text
/// References: <root@example.com> <mid@example.com>
/// ```
fn extract_first_reference(references: &str) -> Option<&str> {
    // Find the first angle-bracket-enclosed Message-ID.
    let start = references.find('<')?;
    let end = references[start..].find('>')? + start;
    // Return the content between (and including) the angle brackets is common,
    // but for key purposes the bare id (without brackets) is cleaner.
    let raw = &references[start + 1..end];
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

/// Derive a thread key from email headers.
///
/// Priority:
/// 1. Provider thread ID (Gmail `X-GM-THRID`, Outlook `conversationId`)
/// 2. First Message-ID from `References` header (thread root)
/// 3. `In-Reply-To` header (parent message)
/// 4. The email's own `Message-ID` (standalone email)
///
/// Returns a non-empty `String` that can be used as the `thread_key` column
/// value.  If **all** inputs are `None`, falls back to an empty string (the
/// caller should substitute the email's database ID in that case).
pub fn derive_thread_key(
    message_id: Option<&str>,
    references: Option<&str>,
    in_reply_to: Option<&str>,
    provider_thread_id: Option<&str>,
) -> String {
    // 1. Provider thread ID takes highest priority.
    if let Some(ptid) = provider_thread_id {
        let trimmed = ptid.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // 2. First reference in the References header (thread root).
    if let Some(refs) = references {
        if let Some(root) = extract_first_reference(refs) {
            return root.to_string();
        }
    }

    // 3. In-Reply-To (parent message — not root, but best we have).
    if let Some(irt) = in_reply_to {
        let cleaned = strip_angle_brackets(irt);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }

    // 4. Standalone email — use its own Message-ID.
    if let Some(mid) = message_id {
        let cleaned = strip_angle_brackets(mid);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }

    String::new()
}

/// Remove surrounding angle brackets and whitespace from a Message-ID string.
fn strip_angle_brackets(s: &str) -> String {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed)
        .to_string()
}

// ---------------------------------------------------------------------------
// Thread collapsing
// ---------------------------------------------------------------------------

/// Collapse search results by thread, keeping only the highest-scoring
/// email per thread.
///
/// Items without a `thread_key` are treated as standalone and always kept.
/// The returned vec is sorted by descending score.
pub fn collapse_by_thread<T: HasThreadKey + HasScore + Clone>(results: Vec<T>) -> Vec<T> {
    let mut best_per_thread: HashMap<String, T> = HashMap::new();

    for result in results {
        let key = result
            .thread_key()
            .map(|k| k.to_string())
            .unwrap_or_else(|| result.id().to_string());

        best_per_thread
            .entry(key)
            .and_modify(|existing| {
                if result.score() > existing.score() {
                    *existing = result.clone();
                }
            })
            .or_insert(result);
    }

    let mut collapsed: Vec<T> = best_per_thread.into_values().collect();
    collapsed.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    collapsed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helpers for testing collapse_by_thread --

    #[derive(Debug, Clone, PartialEq)]
    struct MockResult {
        id: String,
        thread_key: Option<String>,
        score: f32,
    }

    impl HasThreadKey for MockResult {
        fn thread_key(&self) -> Option<&str> {
            self.thread_key.as_deref()
        }
        fn id(&self) -> &str {
            &self.id
        }
    }

    impl HasScore for MockResult {
        fn score(&self) -> f32 {
            self.score
        }
    }

    // -- derive_thread_key tests --

    #[test]
    fn provider_thread_id_takes_priority() {
        let key = derive_thread_key(
            Some("<msg1@example.com>"),
            Some("<root@example.com> <msg1@example.com>"),
            Some("<root@example.com>"),
            Some("gmail-thread-123"),
        );
        assert_eq!(key, "gmail-thread-123");
    }

    #[test]
    fn references_header_extracts_first_message_id() {
        let key = derive_thread_key(
            Some("<msg3@example.com>"),
            Some("<root@example.com> <msg2@example.com>"),
            Some("<msg2@example.com>"),
            None,
        );
        assert_eq!(key, "root@example.com");
    }

    #[test]
    fn in_reply_to_used_when_no_references() {
        let key = derive_thread_key(
            Some("<msg2@example.com>"),
            None,
            Some("<parent@example.com>"),
            None,
        );
        assert_eq!(key, "parent@example.com");
    }

    #[test]
    fn standalone_email_uses_own_message_id() {
        let key = derive_thread_key(Some("<standalone@example.com>"), None, None, None);
        assert_eq!(key, "standalone@example.com");
    }

    #[test]
    fn all_none_returns_empty_string() {
        let key = derive_thread_key(None, None, None, None);
        assert_eq!(key, "");
    }

    #[test]
    fn references_with_commas_parsed() {
        // Some MTAs separate with commas instead of spaces.
        let key = derive_thread_key(
            Some("<msg@example.com>"),
            Some("<first@example.com>,<second@example.com>"),
            None,
            None,
        );
        assert_eq!(key, "first@example.com");
    }

    #[test]
    fn strip_angle_brackets_works() {
        assert_eq!(strip_angle_brackets("<foo@bar>"), "foo@bar");
        assert_eq!(strip_angle_brackets("foo@bar"), "foo@bar");
        assert_eq!(strip_angle_brackets("  <foo@bar>  "), "foo@bar");
    }

    // -- collapse_by_thread tests --

    #[test]
    fn collapse_keeps_highest_score_per_thread() {
        let results = vec![
            MockResult {
                id: "a".into(),
                thread_key: Some("t1".into()),
                score: 0.5,
            },
            MockResult {
                id: "b".into(),
                thread_key: Some("t1".into()),
                score: 0.9,
            },
            MockResult {
                id: "c".into(),
                thread_key: Some("t2".into()),
                score: 0.7,
            },
        ];

        let collapsed = collapse_by_thread(results);
        assert_eq!(collapsed.len(), 2);
        // Highest-scored first
        assert_eq!(collapsed[0].id, "b");
        assert_eq!(collapsed[0].score, 0.9);
        assert_eq!(collapsed[1].id, "c");
        assert_eq!(collapsed[1].score, 0.7);
    }

    #[test]
    fn collapse_preserves_order_by_score() {
        let results = vec![
            MockResult {
                id: "x".into(),
                thread_key: Some("t1".into()),
                score: 0.3,
            },
            MockResult {
                id: "y".into(),
                thread_key: Some("t2".into()),
                score: 0.8,
            },
            MockResult {
                id: "z".into(),
                thread_key: Some("t3".into()),
                score: 0.5,
            },
        ];

        let collapsed = collapse_by_thread(results);
        assert_eq!(collapsed.len(), 3);
        // Should be sorted descending by score
        assert!(collapsed[0].score >= collapsed[1].score);
        assert!(collapsed[1].score >= collapsed[2].score);
    }

    #[test]
    fn collapse_standalone_emails_kept() {
        let results = vec![
            MockResult {
                id: "a".into(),
                thread_key: None,
                score: 0.5,
            },
            MockResult {
                id: "b".into(),
                thread_key: None,
                score: 0.9,
            },
        ];

        // Standalone emails (no thread_key) use their own id as key,
        // so they are never collapsed together.
        let collapsed = collapse_by_thread(results);
        assert_eq!(collapsed.len(), 2);
    }

    #[test]
    fn collapse_empty_input() {
        let results: Vec<MockResult> = vec![];
        let collapsed = collapse_by_thread(results);
        assert!(collapsed.is_empty());
    }

    #[test]
    fn collapse_single_item() {
        let results = vec![MockResult {
            id: "only".into(),
            thread_key: Some("t1".into()),
            score: 1.0,
        }];
        let collapsed = collapse_by_thread(results);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].id, "only");
    }
}
