//! Sender-volume trend: the top inflow contributors over a trailing
//! window, each with its volume delta against the window before it.
//! Shared by `review` (docs/plan/mail-agent-2026-10.md section 8, "noise
//! trend with the three biggest levers", 7-day window) and `health`
//! (section 8's monthly "sender drift", 30-day window). Pure over an
//! already-scanned message list — no I/O of its own.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::scan::ScannedMessage;
use crate::cleanup::domain::sender_key::SenderKey;

/// Rank senders by message count in the trailing `window_days`, each with
/// its count from the `window_days` before that, descending by current
/// volume, capped at `top_n`.
pub fn sender_volume_trend(
    messages: &[ScannedMessage],
    window_days: i64,
    top_n: usize,
) -> Vec<Value> {
    let now = chrono::Utc::now().naive_utc();
    let current_start = now - chrono::Duration::days(window_days);
    let prior_start = now - chrono::Duration::days(window_days * 2);

    let mut current: HashMap<SenderKey, (String, usize)> = HashMap::new();
    let mut prior: HashMap<SenderKey, usize> = HashMap::new();

    for m in messages {
        let Some(key) = SenderKey::from_address(&m.from_addr) else {
            continue;
        };
        if m.received_at >= current_start {
            let entry = current
                .entry(key)
                .or_insert_with(|| (m.from_addr.clone(), 0));
            entry.1 += 1;
        } else if m.received_at >= prior_start {
            *prior.entry(key).or_insert(0) += 1;
        }
    }

    let mut ranked: Vec<_> = current.into_iter().collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1 .1));

    ranked
        .into_iter()
        .take(top_n)
        .map(|(key, (addr, count))| {
            let prior_count = prior.get(&key).copied().unwrap_or(0);
            json!({
                "sender_key": key.as_str(),
                "from_addr": addr,
                "current_count": count,
                "prior_count": prior_count,
                "trend": count as i64 - prior_count as i64,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::domain::kind::{classify, ClassificationContext};
    use crate::email::types::EmailMessage;

    fn scanned(from: &str, days_ago: i64) -> ScannedMessage {
        let received_at = (chrono::Utc::now() - chrono::Duration::days(days_ago)).naive_utc();
        let msg = EmailMessage {
            id: String::new(),
            thread_id: None,
            from: from.to_string(),
            to: vec!["me@example.com".into()],
            cc: vec![],
            subject: String::new(),
            snippet: String::new(),
            body: None,
            body_html: None,
            labels: vec![],
            date: received_at.and_utc(),
            is_read: true,
            list_unsubscribe: Some("<mailto:unsub@example.com>".into()),
            list_unsubscribe_post: None,
        };
        ScannedMessage {
            email_id: "e".into(),
            account_id: "a1".into(),
            from_addr: from.to_string(),
            received_at,
            is_read: true,
            thread_key: "t".into(),
            classification: classify(&msg, &ClassificationContext::default()),
        }
    }

    #[test]
    fn ranks_the_highest_current_volume_sender_first() {
        let messages = vec![
            scanned("a@example.com", 1),
            scanned("a@example.com", 2),
            scanned("b@example.com", 1),
        ];
        let out = sender_volume_trend(&messages, 7, 3);
        assert_eq!(out[0]["sender_key"], json!("a@example.com"));
        assert_eq!(out[0]["current_count"], json!(2));
    }

    #[test]
    fn computes_trend_against_the_prior_window() {
        let messages = vec![
            scanned("a@example.com", 1),  // this window
            scanned("a@example.com", 10), // prior window (7-14 days ago)
        ];
        let out = sender_volume_trend(&messages, 7, 3);
        assert_eq!(out[0]["current_count"], json!(1));
        assert_eq!(out[0]["prior_count"], json!(1));
        assert_eq!(out[0]["trend"], json!(0));
    }

    #[test]
    fn caps_results_at_top_n() {
        let messages = vec![
            scanned("a@example.com", 1),
            scanned("b@example.com", 1),
            scanned("c@example.com", 1),
            scanned("d@example.com", 1),
        ];
        let out = sender_volume_trend(&messages, 7, 2);
        assert_eq!(out.len(), 2);
    }
}
