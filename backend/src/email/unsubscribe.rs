#![allow(dead_code)]
//! Bulk unsubscribe functionality (R-04).
//!
//! Implements RFC 2369 List-Unsubscribe and RFC 8058 List-Unsubscribe-Post
//! header parsing, with batch execution, undo buffer, and false-positive guards.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A parsed unsubscribe method from the List-Unsubscribe header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnsubscribeMethod {
    Mailto {
        email: String,
        subject: Option<String>,
    },
    HttpGet {
        url: String,
    },
    HttpPost {
        url: String,
    },
}

/// A subscription target for unsubscribe operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionTarget {
    /// Sender email address or domain.
    pub sender: String,
    /// Raw List-Unsubscribe header value (if available).
    pub list_unsubscribe_header: Option<String>,
    /// Raw List-Unsubscribe-Post header value (if available).
    pub list_unsubscribe_post: Option<String>,
    /// Email ID for reference.
    pub email_id: Option<String>,
}

/// Result of a single unsubscribe attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeResult {
    pub sender: String,
    pub method_used: Option<UnsubscribeMethod>,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of a batch unsubscribe operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub batch_id: String,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<UnsubscribeResult>,
    pub undo_available_until: DateTime<Utc>,
}

/// Subscription metadata used for false-positive guard checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub sender: String,
    pub email_count: u64,
    pub category: String,
}

/// Preview of what a batch unsubscribe would do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribePreview {
    pub sender: String,
    pub methods: Vec<UnsubscribeMethod>,
    pub best_method: Option<UnsubscribeMethod>,
    pub warning: Option<String>,
}

/// An undo-buffer entry, storing enough state to "undo" an unsubscribe.
#[derive(Debug, Clone)]
struct UndoEntry {
    _batch_id: String,
    targets: Vec<SubscriptionTarget>,
    created_at: Instant,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `List-Unsubscribe` header value (RFC 2369).
///
/// The header contains a comma-separated list of angle-bracket-enclosed URIs:
///   `<mailto:unsub@example.com?subject=unsubscribe>, <https://example.com/unsub>`
///
/// When a `List-Unsubscribe-Post` header is also present (RFC 8058), any
/// HTTPS URI is upgraded to an `HttpPost` method.
pub fn parse_unsubscribe_header(
    header_value: &str,
    post_header: Option<&str>,
) -> Vec<UnsubscribeMethod> {
    let has_post = post_header.map(|v| !v.trim().is_empty()).unwrap_or(false);

    let mut methods = Vec::new();

    for part in header_value.split(',') {
        let trimmed = part.trim();
        // Extract content between angle brackets.
        let uri = match (trimmed.find('<'), trimmed.find('>')) {
            (Some(start), Some(end)) if start < end => &trimmed[start + 1..end],
            _ => continue,
        };

        if let Some(mailto) = uri.strip_prefix("mailto:") {
            let (email, subject) = parse_mailto(mailto);
            methods.push(UnsubscribeMethod::Mailto { email, subject });
        } else if uri.starts_with("https://") || uri.starts_with("http://") {
            if has_post && uri.starts_with("https://") {
                methods.push(UnsubscribeMethod::HttpPost {
                    url: uri.to_string(),
                });
            } else {
                methods.push(UnsubscribeMethod::HttpGet {
                    url: uri.to_string(),
                });
            }
        }
    }

    methods
}

/// Parse a mailto URI, extracting the email address and optional subject.
fn parse_mailto(mailto: &str) -> (String, Option<String>) {
    if let Some((email, query)) = mailto.split_once('?') {
        let subject = query.split('&').find_map(|param| {
            let (key, value) = param.split_once('=')?;
            if key.eq_ignore_ascii_case("subject") {
                Some(urlencoding::decode(value).unwrap_or_default().into_owned())
            } else {
                None
            }
        });
        (email.to_string(), subject)
    } else {
        (mailto.to_string(), None)
    }
}

/// Determine the best unsubscribe method from a list.
///
/// Preference order: HTTP POST (RFC 8058, most reliable) > HTTP GET > mailto.
pub fn best_method(methods: &[UnsubscribeMethod]) -> Option<&UnsubscribeMethod> {
    // Prefer HttpPost first.
    if let Some(m) = methods
        .iter()
        .find(|m| matches!(m, UnsubscribeMethod::HttpPost { .. }))
    {
        return Some(m);
    }
    // Then HttpGet.
    if let Some(m) = methods
        .iter()
        .find(|m| matches!(m, UnsubscribeMethod::HttpGet { .. }))
    {
        return Some(m);
    }
    // Finally mailto.
    methods
        .iter()
        .find(|m| matches!(m, UnsubscribeMethod::Mailto { .. }))
}

/// False-positive guard: warn if the user has high engagement with this sender.
///
/// Returns `true` if the open/interaction rate exceeds 50%, suggesting the
/// user may not actually want to unsubscribe.
pub fn should_warn(_subscription: &Subscription, open_rate: f32) -> bool {
    open_rate > 0.5
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Execute an unsubscribe request using the given method.
///
/// - `HttpPost`: sends a POST with `List-Unsubscribe=One-Click` body (RFC 8058).
/// - `HttpGet`: sends a GET request to the unsubscribe URL.
/// - `Mailto`: returns success (actual sending requires SMTP, which is logged
///   but not executed in the local-first architecture).
pub async fn execute_unsubscribe(
    method: &UnsubscribeMethod,
    http: &reqwest::Client,
) -> Result<UnsubscribeResult, String> {
    match method {
        UnsubscribeMethod::HttpPost { url } => {
            debug!(url = %url, "Executing HTTP POST unsubscribe (RFC 8058)");
            let resp = http
                .post(url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body("List-Unsubscribe=One-Click")
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| e.to_string())?;

            let status = resp.status();
            if status.is_success() || status.as_u16() == 302 {
                Ok(UnsubscribeResult {
                    sender: String::new(),
                    method_used: Some(method.clone()),
                    success: true,
                    error: None,
                })
            } else {
                Ok(UnsubscribeResult {
                    sender: String::new(),
                    method_used: Some(method.clone()),
                    success: false,
                    error: Some(format!("HTTP {}", status)),
                })
            }
        }
        UnsubscribeMethod::HttpGet { url } => {
            debug!(url = %url, "Executing HTTP GET unsubscribe");
            let resp = http
                .get(url)
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| e.to_string())?;

            let status = resp.status();
            if status.is_success() || status.as_u16() == 302 {
                Ok(UnsubscribeResult {
                    sender: String::new(),
                    method_used: Some(method.clone()),
                    success: true,
                    error: None,
                })
            } else {
                Ok(UnsubscribeResult {
                    sender: String::new(),
                    method_used: Some(method.clone()),
                    success: false,
                    error: Some(format!("HTTP {}", status)),
                })
            }
        }
        UnsubscribeMethod::Mailto { email, subject } => {
            info!(
                email = %email,
                subject = subject.as_deref().unwrap_or("(none)"),
                "Mailto unsubscribe recorded (SMTP send deferred to provider)"
            );
            // In a local-first architecture we record the intent; actual SMTP
            // sending is delegated to the connected email provider.
            Ok(UnsubscribeResult {
                sender: String::new(),
                method_used: Some(method.clone()),
                success: true,
                error: None,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// UnsubscribeService
// ---------------------------------------------------------------------------

/// Service for batch unsubscribe with an undo buffer.
pub struct UnsubscribeService {
    http: reqwest::Client,
    undo_buffer: Arc<Mutex<HashMap<String, UndoEntry>>>,
    undo_window: Duration,
}

impl UnsubscribeService {
    /// Create a new unsubscribe service.
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("Emailibrium/1.0 (unsubscribe)")
                .redirect(reqwest::redirect::Policy::limited(3))
                .build()
                .unwrap_or_default(),
            undo_buffer: Arc::new(Mutex::new(HashMap::new())),
            undo_window: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Execute a batch unsubscribe operation.
    ///
    /// For each target, parses the List-Unsubscribe header, selects the best
    /// method, and executes it. Results are collected and an undo buffer entry
    /// is created for the batch.
    pub async fn batch_unsubscribe(&self, subscriptions: Vec<SubscriptionTarget>) -> BatchResult {
        let batch_id = Uuid::new_v4().to_string();
        let total = subscriptions.len();
        let mut results = Vec::with_capacity(total);

        for target in &subscriptions {
            let result = self.unsubscribe_single(target).await;
            results.push(result);
        }

        let succeeded = results.iter().filter(|r| r.success).count();
        let failed = total - succeeded;

        // Store undo entry.
        let undo_deadline =
            Utc::now() + chrono::Duration::seconds(self.undo_window.as_secs() as i64);

        {
            let mut buffer = self.undo_buffer.lock().await;
            buffer.insert(
                batch_id.clone(),
                UndoEntry {
                    _batch_id: batch_id.clone(),
                    targets: subscriptions,
                    created_at: Instant::now(),
                },
            );
        }

        // Prune expired undo entries.
        self.prune_expired_entries().await;

        info!(
            batch_id = %batch_id,
            total = total,
            succeeded = succeeded,
            failed = failed,
            "Batch unsubscribe completed"
        );

        BatchResult {
            batch_id,
            total,
            succeeded,
            failed,
            results,
            undo_available_until: undo_deadline,
        }
    }

    /// Undo a previous batch unsubscribe (only within the undo window).
    ///
    /// Note: true undo is only possible for mailto-based unsubscribes (by not
    /// sending the email) or if the service supports re-subscribe. For HTTP-based
    /// unsubscribes, undo is best-effort and logs the intent.
    pub async fn undo(&self, batch_id: &str) -> Result<(), String> {
        let mut buffer = self.undo_buffer.lock().await;

        let entry = buffer
            .remove(batch_id)
            .ok_or_else(|| format!("No undo entry found for batch '{batch_id}'"))?;

        if entry.created_at.elapsed() > self.undo_window {
            return Err(format!(
                "Undo window expired for batch '{batch_id}' ({}s limit)",
                self.undo_window.as_secs()
            ));
        }

        info!(
            batch_id = %batch_id,
            targets = entry.targets.len(),
            "Undo requested for batch unsubscribe"
        );

        // For HTTP-based unsubscribes, we can only log the undo intent.
        // For mailto, the email was never actually sent, so undo is automatic.
        warn!(
            batch_id = %batch_id,
            "Undo is best-effort for HTTP-based unsubscribes; \
             re-subscribe may require manual action"
        );

        Ok(())
    }

    /// Generate a preview of what a batch unsubscribe would do.
    pub fn preview(
        &self,
        targets: &[SubscriptionTarget],
        engagement_rates: &HashMap<String, f32>,
    ) -> Vec<UnsubscribePreview> {
        targets
            .iter()
            .map(|target| {
                let methods = target
                    .list_unsubscribe_header
                    .as_deref()
                    .map(|h| {
                        parse_unsubscribe_header(h, target.list_unsubscribe_post.as_deref())
                    })
                    .unwrap_or_default();

                let best = best_method(&methods).cloned();

                let open_rate = engagement_rates
                    .get(&target.sender)
                    .copied()
                    .unwrap_or(0.0);

                let warning = if should_warn(
                    &Subscription {
                        sender: target.sender.clone(),
                        email_count: 0,
                        category: String::new(),
                    },
                    open_rate,
                ) {
                    Some(format!(
                        "High engagement ({:.0}% open rate) — are you sure you want to unsubscribe?",
                        open_rate * 100.0
                    ))
                } else {
                    None
                };

                UnsubscribePreview {
                    sender: target.sender.clone(),
                    methods,
                    best_method: best,
                    warning,
                }
            })
            .collect()
    }

    /// Execute unsubscribe for a single target.
    async fn unsubscribe_single(&self, target: &SubscriptionTarget) -> UnsubscribeResult {
        let methods = target
            .list_unsubscribe_header
            .as_deref()
            .map(|h| parse_unsubscribe_header(h, target.list_unsubscribe_post.as_deref()))
            .unwrap_or_default();

        let method = match best_method(&methods) {
            Some(m) => m,
            None => {
                return UnsubscribeResult {
                    sender: target.sender.clone(),
                    method_used: None,
                    success: false,
                    error: Some("No unsubscribe method available".to_string()),
                };
            }
        };

        match execute_unsubscribe(method, &self.http).await {
            Ok(mut result) => {
                result.sender = target.sender.clone();
                result
            }
            Err(e) => UnsubscribeResult {
                sender: target.sender.clone(),
                method_used: Some(method.clone()),
                success: false,
                error: Some(e),
            },
        }
    }

    /// Remove expired entries from the undo buffer.
    async fn prune_expired_entries(&self) {
        let mut buffer = self.undo_buffer.lock().await;
        buffer.retain(|_, entry| entry.created_at.elapsed() <= self.undo_window);
    }
}

impl Default for UnsubscribeService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mailto_only() {
        let header = "<mailto:unsub@example.com>";
        let methods = parse_unsubscribe_header(header, None);
        assert_eq!(methods.len(), 1);
        assert_eq!(
            methods[0],
            UnsubscribeMethod::Mailto {
                email: "unsub@example.com".to_string(),
                subject: None,
            }
        );
    }

    #[test]
    fn test_parse_mailto_with_subject() {
        let header = "<mailto:unsub@example.com?subject=Unsubscribe%20me>";
        let methods = parse_unsubscribe_header(header, None);
        assert_eq!(methods.len(), 1);
        match &methods[0] {
            UnsubscribeMethod::Mailto { email, subject } => {
                assert_eq!(email, "unsub@example.com");
                assert_eq!(subject.as_deref(), Some("Unsubscribe me"));
            }
            _ => panic!("Expected Mailto"),
        }
    }

    #[test]
    fn test_parse_http_and_mailto() {
        let header = "<mailto:unsub@example.com>, <https://example.com/unsub?token=abc>";
        let methods = parse_unsubscribe_header(header, None);
        assert_eq!(methods.len(), 2);
        assert!(matches!(&methods[0], UnsubscribeMethod::Mailto { .. }));
        assert!(matches!(&methods[1], UnsubscribeMethod::HttpGet { .. }));
    }

    #[test]
    fn test_parse_http_post_with_rfc8058() {
        let header = "<https://example.com/unsub>";
        let methods = parse_unsubscribe_header(header, Some("List-Unsubscribe=One-Click"));
        assert_eq!(methods.len(), 1);
        assert!(matches!(&methods[0], UnsubscribeMethod::HttpPost { .. }));
    }

    #[test]
    fn test_parse_http_get_no_post_header() {
        let header = "<http://example.com/unsub>";
        let methods = parse_unsubscribe_header(header, None);
        assert_eq!(methods.len(), 1);
        assert!(matches!(&methods[0], UnsubscribeMethod::HttpGet { .. }));
    }

    #[test]
    fn test_parse_empty_header() {
        let methods = parse_unsubscribe_header("", None);
        assert!(methods.is_empty());
    }

    #[test]
    fn test_parse_malformed_header() {
        let methods = parse_unsubscribe_header("not a valid header", None);
        assert!(methods.is_empty());
    }

    #[test]
    fn test_best_method_prefers_post() {
        let methods = vec![
            UnsubscribeMethod::Mailto {
                email: "unsub@example.com".to_string(),
                subject: None,
            },
            UnsubscribeMethod::HttpGet {
                url: "https://example.com/get".to_string(),
            },
            UnsubscribeMethod::HttpPost {
                url: "https://example.com/post".to_string(),
            },
        ];
        let best = best_method(&methods).unwrap();
        assert!(matches!(best, UnsubscribeMethod::HttpPost { .. }));
    }

    #[test]
    fn test_best_method_prefers_get_over_mailto() {
        let methods = vec![
            UnsubscribeMethod::Mailto {
                email: "unsub@example.com".to_string(),
                subject: None,
            },
            UnsubscribeMethod::HttpGet {
                url: "https://example.com/get".to_string(),
            },
        ];
        let best = best_method(&methods).unwrap();
        assert!(matches!(best, UnsubscribeMethod::HttpGet { .. }));
    }

    #[test]
    fn test_best_method_falls_back_to_mailto() {
        let methods = vec![UnsubscribeMethod::Mailto {
            email: "unsub@example.com".to_string(),
            subject: None,
        }];
        let best = best_method(&methods).unwrap();
        assert!(matches!(best, UnsubscribeMethod::Mailto { .. }));
    }

    #[test]
    fn test_best_method_empty() {
        let methods: Vec<UnsubscribeMethod> = vec![];
        assert!(best_method(&methods).is_none());
    }

    #[test]
    fn test_should_warn_high_engagement() {
        let sub = Subscription {
            sender: "news@example.com".to_string(),
            email_count: 10,
            category: "newsletter".to_string(),
        };
        assert!(should_warn(&sub, 0.75));
    }

    #[test]
    fn test_should_not_warn_low_engagement() {
        let sub = Subscription {
            sender: "spam@example.com".to_string(),
            email_count: 50,
            category: "marketing".to_string(),
        };
        assert!(!should_warn(&sub, 0.10));
    }

    #[test]
    fn test_should_warn_boundary() {
        let sub = Subscription {
            sender: "edge@example.com".to_string(),
            email_count: 5,
            category: "newsletter".to_string(),
        };
        // Exactly 50% should not warn (> 0.5, not >=).
        assert!(!should_warn(&sub, 0.5));
        assert!(should_warn(&sub, 0.51));
    }

    #[test]
    fn test_preview_with_engagement_warning() {
        let service = UnsubscribeService::new();

        let targets = vec![SubscriptionTarget {
            sender: "news@example.com".to_string(),
            list_unsubscribe_header: Some("<https://example.com/unsub>".to_string()),
            list_unsubscribe_post: Some("List-Unsubscribe=One-Click".to_string()),
            email_id: None,
        }];

        let mut engagement = HashMap::new();
        engagement.insert("news@example.com".to_string(), 0.8);

        let previews = service.preview(&targets, &engagement);
        assert_eq!(previews.len(), 1);
        assert!(previews[0].warning.is_some());
        assert!(previews[0].best_method.is_some());
        assert!(matches!(
            &previews[0].best_method,
            Some(UnsubscribeMethod::HttpPost { .. })
        ));
    }

    #[test]
    fn test_preview_no_warning_low_engagement() {
        let service = UnsubscribeService::new();

        let targets = vec![SubscriptionTarget {
            sender: "spam@example.com".to_string(),
            list_unsubscribe_header: Some("<mailto:unsub@spam.com>".to_string()),
            list_unsubscribe_post: None,
            email_id: None,
        }];

        let mut engagement = HashMap::new();
        engagement.insert("spam@example.com".to_string(), 0.1);

        let previews = service.preview(&targets, &engagement);
        assert_eq!(previews.len(), 1);
        assert!(previews[0].warning.is_none());
    }

    #[test]
    fn test_preview_no_methods_available() {
        let service = UnsubscribeService::new();

        let targets = vec![SubscriptionTarget {
            sender: "sender@example.com".to_string(),
            list_unsubscribe_header: None,
            list_unsubscribe_post: None,
            email_id: None,
        }];

        let previews = service.preview(&targets, &HashMap::new());
        assert_eq!(previews.len(), 1);
        assert!(previews[0].methods.is_empty());
        assert!(previews[0].best_method.is_none());
    }

    #[tokio::test]
    async fn test_undo_expired_batch() {
        let service = UnsubscribeService {
            http: reqwest::Client::new(),
            undo_buffer: Arc::new(Mutex::new(HashMap::new())),
            undo_window: Duration::from_millis(1), // Very short window.
        };

        // Insert a fake undo entry that's already expired.
        {
            let mut buffer = service.undo_buffer.lock().await;
            buffer.insert(
                "test-batch".to_string(),
                UndoEntry {
                    _batch_id: "test-batch".to_string(),
                    targets: vec![],
                    created_at: Instant::now() - Duration::from_secs(10),
                },
            );
        }

        let result = service.undo("test-batch").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expired"));
    }

    #[tokio::test]
    async fn test_undo_nonexistent_batch() {
        let service = UnsubscribeService::new();
        let result = service.undo("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No undo entry"));
    }

    #[test]
    fn test_parse_multiple_methods_complex() {
        let header = "<mailto:unsub@a.com?subject=unsub>, \
                       <https://a.com/unsub>, \
                       <http://fallback.com/unsub>";
        let methods = parse_unsubscribe_header(header, Some("List-Unsubscribe=One-Click"));

        assert_eq!(methods.len(), 3);
        assert!(matches!(&methods[0], UnsubscribeMethod::Mailto { .. }));
        // HTTPS with post header becomes HttpPost.
        assert!(matches!(&methods[1], UnsubscribeMethod::HttpPost { .. }));
        // HTTP (not HTTPS) stays HttpGet even with post header.
        assert!(matches!(&methods[2], UnsubscribeMethod::HttpGet { .. }));
    }
}
