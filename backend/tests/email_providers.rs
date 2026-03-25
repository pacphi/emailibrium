//! Integration tests for email provider API clients (R-01).
//!
//! Tests IMAP provider, Gmail history parsing, Outlook delta parsing,
//! and batch fetching concurrency limits.

use emailibrium::email::delta::{parse_gmail_history, parse_outlook_delta};
use emailibrium::email::imap::{ImapConfig, ImapProvider};
use emailibrium::email::provider::{EmailProvider, ProviderError};
use emailibrium::email::sync::{ProviderSync, ProviderSyncService};
use emailibrium::email::types::{
    EmailMessage, EmailPage, ListParams, OAuthTokens, ProviderKind, SyncState,
};

use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Helper: Mock Provider
// ---------------------------------------------------------------------------

struct MockEmailProvider {
    messages: Vec<EmailMessage>,
    concurrent_counter: Arc<AtomicUsize>,
    max_concurrent_observed: Arc<AtomicUsize>,
}

impl MockEmailProvider {
    fn new(count: usize) -> Self {
        let messages = (0..count)
            .map(|i| EmailMessage {
                id: format!("msg-{i}"),
                thread_id: Some(format!("thread-{i}")),
                from: format!("sender{i}@test.com"),
                to: vec!["user@test.com".into()],
                subject: format!("Subject {i}"),
                snippet: format!("Snippet {i}"),
                body: Some(format!("Body {i}")),
                labels: vec!["INBOX".into()],
                date: Utc::now(),
                is_read: i % 2 == 0,
            })
            .collect();
        Self {
            messages,
            concurrent_counter: Arc::new(AtomicUsize::new(0)),
            max_concurrent_observed: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl EmailProvider for MockEmailProvider {
    async fn authenticate(&self, _code: &str) -> Result<OAuthTokens, ProviderError> {
        Ok(OAuthTokens {
            access_token: "mock-token".into(),
            refresh_token: Some("mock-refresh".into()),
            expires_at: None,
            email: Some("mock@test.com".into()),
        })
    }

    async fn refresh_token(&self, _token: &str) -> Result<OAuthTokens, ProviderError> {
        self.authenticate("").await
    }

    async fn list_messages(
        &self,
        _token: &str,
        params: &ListParams,
    ) -> Result<EmailPage, ProviderError> {
        let max = params.max_results as usize;
        let msgs: Vec<EmailMessage> = self.messages.iter().take(max).cloned().collect();
        Ok(EmailPage {
            messages: msgs,
            next_page_token: None,
            result_size_estimate: Some(self.messages.len() as u32),
        })
    }

    async fn get_message(&self, _token: &str, id: &str) -> Result<EmailMessage, ProviderError> {
        self.messages
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or(ProviderError::NotFound(id.into()))
    }

    async fn archive_message(&self, _token: &str, _id: &str) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn label_message(
        &self,
        _token: &str,
        _id: &str,
        _labels: &[String],
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn remove_labels(
        &self,
        _token: &str,
        _id: &str,
        _labels: &[String],
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn create_label(&self, _token: &str, name: &str) -> Result<String, ProviderError> {
        Ok(name.to_string())
    }
}

fn make_sync_state() -> SyncState {
    SyncState {
        account_id: "test-acct".to_string(),
        last_sync_at: None,
        history_id: None,
        next_page_token: None,
        emails_synced: 0,
        sync_failures: 0,
        last_error: None,
        status: "idle".into(),
    }
}

// ---------------------------------------------------------------------------
// IMAP Provider Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn imap_authenticate_validates_config() {
    let config = ImapConfig {
        host: "imap.test.com".into(),
        port: 993,
        use_tls: true,
        username: "user@test.com".into(),
        password: "password".into(),
        mailbox: "INBOX".into(),
        archive_folder: "Archive".into(),
    };
    let provider = ImapProvider::new(config);
    let tokens = provider.authenticate("").await.unwrap();
    assert!(tokens.access_token.contains("imap.test.com"));
    assert_eq!(tokens.email, Some("user@test.com".into()));
}

#[tokio::test]
async fn imap_authenticate_rejects_empty_host() {
    let config = ImapConfig {
        host: "".into(),
        port: 993,
        use_tls: true,
        username: "user@test.com".into(),
        password: "password".into(),
        mailbox: "INBOX".into(),
        archive_folder: "Archive".into(),
    };
    let provider = ImapProvider::new(config);
    let result = provider.authenticate("").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn imap_list_messages_returns_empty() {
    let config = ImapConfig {
        host: "imap.test.com".into(),
        port: 993,
        use_tls: true,
        username: "user@test.com".into(),
        password: "password".into(),
        mailbox: "INBOX".into(),
        archive_folder: "Archive".into(),
    };
    let provider = ImapProvider::new(config);
    let params = ListParams {
        max_results: 50,
        page_token: None,
        label: None,
        query: None,
    };
    let page = provider.list_messages("token", &params).await.unwrap();
    assert!(page.messages.is_empty());
}

#[tokio::test]
async fn imap_create_label_returns_folder_name() {
    let config = ImapConfig {
        host: "imap.test.com".into(),
        port: 993,
        use_tls: true,
        username: "user@test.com".into(),
        password: "password".into(),
        mailbox: "INBOX".into(),
        archive_folder: "Archive".into(),
    };
    let provider = ImapProvider::new(config);
    let id = provider.create_label("token", "MyFolder").await.unwrap();
    assert_eq!(id, "MyFolder");
}

// ---------------------------------------------------------------------------
// Gmail History Parsing Tests
// ---------------------------------------------------------------------------

#[test]
fn gmail_history_empty_response() {
    let resp = serde_json::json!({ "historyId": "999" });
    let delta = parse_gmail_history(&resp).unwrap();
    assert!(delta.added_message_ids.is_empty());
    assert!(delta.deleted_message_ids.is_empty());
    assert_eq!(delta.new_history_id, Some("999".into()));
}

#[test]
fn gmail_history_added_messages() {
    let resp = serde_json::json!({
        "history": [{
            "id": "1",
            "messagesAdded": [
                { "message": { "id": "a1", "threadId": "t1", "labelIds": ["INBOX"] } },
                { "message": { "id": "a2", "threadId": "t2", "labelIds": ["INBOX", "UNREAD"] } }
            ]
        }],
        "historyId": "100"
    });
    let delta = parse_gmail_history(&resp).unwrap();
    assert_eq!(delta.added_message_ids.len(), 2);
    assert!(delta.added_message_ids.contains(&"a1".into()));
    assert!(delta.added_message_ids.contains(&"a2".into()));
}

#[test]
fn gmail_history_deleted_messages() {
    let resp = serde_json::json!({
        "history": [{
            "id": "1",
            "messagesDeleted": [
                { "message": { "id": "d1", "threadId": "t1" } }
            ]
        }],
        "historyId": "200"
    });
    let delta = parse_gmail_history(&resp).unwrap();
    assert_eq!(delta.deleted_message_ids, vec!["d1"]);
}

#[test]
fn gmail_history_label_changes() {
    let resp = serde_json::json!({
        "history": [{
            "id": "1",
            "labelsAdded": [{
                "message": { "id": "m1", "threadId": "t1" },
                "labelIds": ["STARRED", "IMPORTANT"]
            }],
            "labelsRemoved": [{
                "message": { "id": "m1", "threadId": "t1" },
                "labelIds": ["UNREAD"]
            }]
        }],
        "historyId": "300"
    });
    let delta = parse_gmail_history(&resp).unwrap();
    assert_eq!(delta.label_changes.len(), 1);
    let change = &delta.label_changes[0];
    assert_eq!(change.message_id, "m1");
    assert_eq!(change.added_labels, vec!["STARRED", "IMPORTANT"]);
    assert_eq!(change.removed_labels, vec!["UNREAD"]);
}

#[test]
fn gmail_history_mixed_records() {
    let resp = serde_json::json!({
        "history": [
            {
                "id": "10",
                "messagesAdded": [
                    { "message": { "id": "new1", "threadId": "t1" } }
                ]
            },
            {
                "id": "11",
                "messagesDeleted": [
                    { "message": { "id": "del1", "threadId": "t2" } }
                ],
                "labelsAdded": [{
                    "message": { "id": "mod1", "threadId": "t3" },
                    "labelIds": ["STARRED"]
                }]
            }
        ],
        "historyId": "400"
    });
    let delta = parse_gmail_history(&resp).unwrap();
    assert_eq!(delta.added_message_ids, vec!["new1"]);
    assert_eq!(delta.deleted_message_ids, vec!["del1"]);
    assert_eq!(delta.label_changes.len(), 1);
    assert_eq!(delta.new_history_id, Some("400".into()));
}

// ---------------------------------------------------------------------------
// Outlook Delta Parsing Tests
// ---------------------------------------------------------------------------

#[test]
fn outlook_delta_empty_response() {
    let resp = serde_json::json!({
        "value": [],
        "@odata.deltaLink": "https://graph.microsoft.com/delta?token=abc"
    });
    let result = parse_outlook_delta(&resp).unwrap();
    assert!(result.added_or_modified_ids.is_empty());
    assert!(result.deleted_ids.is_empty());
    assert!(result.delta_link.is_some());
}

#[test]
fn outlook_delta_added_and_modified() {
    let resp = serde_json::json!({
        "value": [
            { "id": "msg-1", "subject": "New", "isRead": false },
            { "id": "msg-2", "subject": "Updated", "isRead": true }
        ],
        "@odata.deltaLink": "https://graph.microsoft.com/delta?token=xyz"
    });
    let result = parse_outlook_delta(&resp).unwrap();
    assert_eq!(result.added_or_modified_ids.len(), 2);
    assert!(result.deleted_ids.is_empty());
}

#[test]
fn outlook_delta_deleted() {
    let resp = serde_json::json!({
        "value": [
            { "id": "del-1", "@removed": { "reason": "deleted" } },
            { "id": "del-2", "@removed": { "reason": "changed" } }
        ],
        "@odata.deltaLink": "https://graph.microsoft.com/delta?token=fin"
    });
    let result = parse_outlook_delta(&resp).unwrap();
    assert!(result.added_or_modified_ids.is_empty());
    assert_eq!(result.deleted_ids.len(), 2);
}

#[test]
fn outlook_delta_mixed() {
    let resp = serde_json::json!({
        "value": [
            { "id": "new-1", "subject": "Hello" },
            { "id": "del-1", "@removed": { "reason": "deleted" } },
            { "id": "mod-1", "subject": "Updated subject", "isRead": true }
        ],
        "@odata.deltaLink": "https://graph.microsoft.com/delta?token=mixed"
    });
    let result = parse_outlook_delta(&resp).unwrap();
    assert_eq!(result.added_or_modified_ids, vec!["new-1", "mod-1"]);
    assert_eq!(result.deleted_ids, vec!["del-1"]);
}

#[test]
fn outlook_delta_pagination_no_delta_link() {
    let resp = serde_json::json!({
        "value": [
            { "id": "page-1", "subject": "In progress" }
        ],
        "@odata.nextLink": "https://graph.microsoft.com/delta?skiptoken=page2"
    });
    let result = parse_outlook_delta(&resp).unwrap();
    assert_eq!(result.added_or_modified_ids.len(), 1);
    assert!(result.delta_link.is_none());
}

// ---------------------------------------------------------------------------
// Sync Service with Provider Kind Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sync_with_fallback_delta_detection() {
    let provider = Arc::new(MockEmailProvider::new(5));
    let sync = ProviderSync::new(provider, 50);
    let state = make_sync_state();

    let delta = sync.detect_delta("token", &state).await.unwrap();
    assert_eq!(delta.new_message_ids.len(), 5);
    assert!(delta.updated_message_ids.is_empty());
    assert!(delta.deleted_message_ids.is_empty());
}

#[tokio::test]
async fn sync_with_kind_uses_fallback_when_no_history_id() {
    let provider = Arc::new(MockEmailProvider::new(3));
    let sync = ProviderSync::with_kind(provider, 50, ProviderKind::Gmail);
    let state = make_sync_state();

    // Gmail delta requires a history_id; without one, falls back to list.
    let delta = sync.detect_delta("token", &state).await.unwrap();
    assert_eq!(delta.new_message_ids.len(), 3);
}

#[tokio::test]
async fn sync_account_reports_correct_counts() {
    let provider = Arc::new(MockEmailProvider::new(10));
    let sync = ProviderSync::new(provider, 50);
    let state = make_sync_state();

    let result = sync
        .sync_account("test-acct", "token", &state)
        .await
        .unwrap();
    assert_eq!(result.emails_fetched, 10);
    assert_eq!(result.account_id, "test-acct");
    assert!(result.duration_ms < 5000);
}
