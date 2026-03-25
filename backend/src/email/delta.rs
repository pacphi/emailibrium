//! Provider-specific delta sync types and parsing (R-01).
//!
//! Shared types used by Gmail history.list and Outlook delta query
//! implementations. Kept separate from `sync.rs` to avoid bloating
//! the sync orchestrator with provider-specific JSON parsing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Gmail History Types
// ---------------------------------------------------------------------------

/// A single Gmail history record from the `history.list` API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailHistoryRecord {
    /// The history ID for this record.
    pub id: String,
    /// Messages added to the mailbox since the previous history ID.
    #[serde(default)]
    pub messages_added: Vec<GmailHistoryMessage>,
    /// Messages deleted from the mailbox.
    #[serde(default)]
    pub messages_deleted: Vec<GmailHistoryMessage>,
    /// Labels added to existing messages.
    #[serde(default)]
    pub labels_added: Vec<GmailHistoryLabelChange>,
    /// Labels removed from existing messages.
    #[serde(default)]
    pub labels_removed: Vec<GmailHistoryLabelChange>,
}

/// A message reference within a history record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailHistoryMessage {
    pub message: GmailMessageRef,
}

/// Minimal message reference (id + threadId).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailMessageRef {
    pub id: String,
    pub thread_id: Option<String>,
    #[serde(default)]
    pub label_ids: Vec<String>,
}

/// A label change event within a history record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailHistoryLabelChange {
    pub message: GmailMessageRef,
    pub label_ids: Vec<String>,
}

/// Full response from `GET /gmail/v1/users/me/history`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailHistoryResponse {
    #[serde(default)]
    pub history: Vec<GmailHistoryRecord>,
    pub next_page_token: Option<String>,
    pub history_id: Option<String>,
}

/// Aggregated result of processing Gmail history records.
#[derive(Debug, Clone, Default)]
pub struct GmailHistoryDelta {
    pub added_message_ids: Vec<String>,
    pub deleted_message_ids: Vec<String>,
    pub label_changes: Vec<GmailLabelDelta>,
    pub new_history_id: Option<String>,
}

/// A label change for a specific message.
#[derive(Debug, Clone)]
pub struct GmailLabelDelta {
    pub message_id: String,
    pub added_labels: Vec<String>,
    pub removed_labels: Vec<String>,
}

/// Parse a Gmail history.list JSON response into an aggregated delta.
pub fn parse_gmail_history(resp: &serde_json::Value) -> Result<GmailHistoryDelta, String> {
    let history_resp: GmailHistoryResponse =
        serde_json::from_value(resp.clone()).map_err(|e| e.to_string())?;

    let mut delta = GmailHistoryDelta {
        new_history_id: history_resp.history_id,
        ..Default::default()
    };

    // Track label changes per message for merging.
    let mut label_map: std::collections::HashMap<String, (Vec<String>, Vec<String>)> =
        std::collections::HashMap::new();

    for record in &history_resp.history {
        for added in &record.messages_added {
            let id = &added.message.id;
            if !delta.added_message_ids.contains(id) {
                delta.added_message_ids.push(id.clone());
            }
        }

        for deleted in &record.messages_deleted {
            let id = &deleted.message.id;
            if !delta.deleted_message_ids.contains(id) {
                delta.deleted_message_ids.push(id.clone());
            }
        }

        for label_add in &record.labels_added {
            let entry = label_map.entry(label_add.message.id.clone()).or_default();
            for label in &label_add.label_ids {
                if !entry.0.contains(label) {
                    entry.0.push(label.clone());
                }
            }
        }

        for label_remove in &record.labels_removed {
            let entry = label_map
                .entry(label_remove.message.id.clone())
                .or_default();
            for label in &label_remove.label_ids {
                if !entry.1.contains(label) {
                    entry.1.push(label.clone());
                }
            }
        }
    }

    for (msg_id, (added, removed)) in label_map {
        delta.label_changes.push(GmailLabelDelta {
            message_id: msg_id,
            added_labels: added,
            removed_labels: removed,
        });
    }

    Ok(delta)
}

// ---------------------------------------------------------------------------
// Outlook Delta Types
// ---------------------------------------------------------------------------

/// A message from the Outlook delta query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlookDeltaMessage {
    pub id: String,
    /// Present when the message has been soft-deleted.
    #[serde(rename = "@removed")]
    pub removed: Option<OutlookRemovedReason>,
    pub subject: Option<String>,
    #[serde(rename = "receivedDateTime")]
    pub received_date_time: Option<String>,
    #[serde(rename = "isRead")]
    pub is_read: Option<bool>,
    pub categories: Option<Vec<String>>,
    #[serde(rename = "bodyPreview")]
    pub body_preview: Option<String>,
    pub from: Option<serde_json::Value>,
    #[serde(rename = "toRecipients")]
    pub to_recipients: Option<Vec<serde_json::Value>>,
}

/// Reason a message was removed in a delta response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlookRemovedReason {
    pub reason: String,
}

/// Full response from `GET /me/mailFolders/inbox/messages/delta`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlookDeltaResponse {
    #[serde(default)]
    pub value: Vec<OutlookDeltaMessage>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

/// Aggregated result of processing Outlook delta responses.
#[derive(Debug, Clone, Default)]
pub struct OutlookDeltaResult {
    pub added_or_modified_ids: Vec<String>,
    pub deleted_ids: Vec<String>,
    pub delta_link: Option<String>,
}

/// Parse an Outlook delta query JSON response into an aggregated result.
pub fn parse_outlook_delta(resp: &serde_json::Value) -> Result<OutlookDeltaResult, String> {
    let delta_resp: OutlookDeltaResponse =
        serde_json::from_value(resp.clone()).map_err(|e| e.to_string())?;

    let mut result = OutlookDeltaResult {
        delta_link: delta_resp.delta_link,
        ..Default::default()
    };

    for msg in &delta_resp.value {
        if msg.removed.is_some() {
            result.deleted_ids.push(msg.id.clone());
        } else {
            result.added_or_modified_ids.push(msg.id.clone());
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gmail_history_empty() {
        let resp = serde_json::json!({
            "historyId": "12345"
        });
        let delta = parse_gmail_history(&resp).unwrap();
        assert!(delta.added_message_ids.is_empty());
        assert!(delta.deleted_message_ids.is_empty());
        assert_eq!(delta.new_history_id, Some("12345".to_string()));
    }

    #[test]
    fn test_parse_gmail_history_messages_added() {
        let resp = serde_json::json!({
            "history": [
                {
                    "id": "100",
                    "messagesAdded": [
                        { "message": { "id": "msg-1", "threadId": "t-1", "labelIds": ["INBOX"] } },
                        { "message": { "id": "msg-2", "threadId": "t-2", "labelIds": ["INBOX", "UNREAD"] } }
                    ]
                }
            ],
            "historyId": "200"
        });
        let delta = parse_gmail_history(&resp).unwrap();
        assert_eq!(delta.added_message_ids, vec!["msg-1", "msg-2"]);
        assert!(delta.deleted_message_ids.is_empty());
        assert_eq!(delta.new_history_id, Some("200".to_string()));
    }

    #[test]
    fn test_parse_gmail_history_messages_deleted() {
        let resp = serde_json::json!({
            "history": [
                {
                    "id": "100",
                    "messagesDeleted": [
                        { "message": { "id": "msg-3", "threadId": "t-3" } }
                    ]
                }
            ],
            "historyId": "300"
        });
        let delta = parse_gmail_history(&resp).unwrap();
        assert!(delta.added_message_ids.is_empty());
        assert_eq!(delta.deleted_message_ids, vec!["msg-3"]);
    }

    #[test]
    fn test_parse_gmail_history_label_changes() {
        let resp = serde_json::json!({
            "history": [
                {
                    "id": "100",
                    "labelsAdded": [
                        {
                            "message": { "id": "msg-1", "threadId": "t-1" },
                            "labelIds": ["STARRED"]
                        }
                    ],
                    "labelsRemoved": [
                        {
                            "message": { "id": "msg-1", "threadId": "t-1" },
                            "labelIds": ["UNREAD"]
                        }
                    ]
                }
            ],
            "historyId": "400"
        });
        let delta = parse_gmail_history(&resp).unwrap();
        assert_eq!(delta.label_changes.len(), 1);
        assert_eq!(delta.label_changes[0].message_id, "msg-1");
        assert_eq!(delta.label_changes[0].added_labels, vec!["STARRED"]);
        assert_eq!(delta.label_changes[0].removed_labels, vec!["UNREAD"]);
    }

    #[test]
    fn test_parse_gmail_history_deduplicates() {
        let resp = serde_json::json!({
            "history": [
                {
                    "id": "100",
                    "messagesAdded": [
                        { "message": { "id": "msg-1", "threadId": "t-1" } }
                    ]
                },
                {
                    "id": "101",
                    "messagesAdded": [
                        { "message": { "id": "msg-1", "threadId": "t-1" } }
                    ]
                }
            ],
            "historyId": "500"
        });
        let delta = parse_gmail_history(&resp).unwrap();
        // msg-1 appears twice but should be deduplicated.
        assert_eq!(delta.added_message_ids.len(), 1);
    }

    #[test]
    fn test_parse_outlook_delta_empty() {
        let resp = serde_json::json!({
            "value": [],
            "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=abc"
        });
        let result = parse_outlook_delta(&resp).unwrap();
        assert!(result.added_or_modified_ids.is_empty());
        assert!(result.deleted_ids.is_empty());
        assert!(result.delta_link.is_some());
    }

    #[test]
    fn test_parse_outlook_delta_added_and_deleted() {
        let resp = serde_json::json!({
            "value": [
                {
                    "id": "AAMkAG1",
                    "subject": "New email",
                    "isRead": false
                },
                {
                    "id": "AAMkAG2",
                    "subject": "Modified email",
                    "isRead": true
                },
                {
                    "id": "AAMkAG3",
                    "@removed": { "reason": "deleted" }
                }
            ],
            "@odata.deltaLink": "https://graph.microsoft.com/delta?token=xyz"
        });
        let result = parse_outlook_delta(&resp).unwrap();
        assert_eq!(result.added_or_modified_ids, vec!["AAMkAG1", "AAMkAG2"]);
        assert_eq!(result.deleted_ids, vec!["AAMkAG3"]);
        assert_eq!(
            result.delta_link,
            Some("https://graph.microsoft.com/delta?token=xyz".to_string())
        );
    }

    #[test]
    fn test_parse_outlook_delta_with_next_link() {
        let resp = serde_json::json!({
            "value": [
                { "id": "AAMkAG1", "subject": "Page 1" }
            ],
            "@odata.nextLink": "https://graph.microsoft.com/delta?skiptoken=abc"
        });
        let result = parse_outlook_delta(&resp).unwrap();
        assert_eq!(result.added_or_modified_ids.len(), 1);
        // No deltaLink means more pages to fetch.
        assert!(result.delta_link.is_none());
    }

    #[test]
    fn test_parse_gmail_history_invalid_json() {
        let resp = serde_json::json!("not an object");
        let result = parse_gmail_history(&resp);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_outlook_delta_invalid_json() {
        let resp = serde_json::json!(42);
        let result = parse_outlook_delta(&resp);
        assert!(result.is_err());
    }
}
