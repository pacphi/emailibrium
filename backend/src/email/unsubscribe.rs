#![allow(dead_code)]
//! Bulk unsubscribe functionality (R-04).
//!
//! Implements RFC 2369 List-Unsubscribe and RFC 8058 List-Unsubscribe-Post
//! header parsing, with batch execution, undo buffer, and false-positive guards.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeResult {
    pub sender: String,
    pub method_used: Option<UnsubscribeMethod>,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of a batch unsubscribe operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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

/// Reject a resolved address that would let an email's `List-Unsubscribe`
/// header turn this local engine into an SSRF primitive: the URL in that
/// header is attacker-controlled, so every address it resolves to is
/// checked, not just the hostname's apparent shape. Loopback, RFC 1918
/// private, link-local, IPv6 unique-local (`fc00::/7`), CGNAT
/// (`100.64.0.0/10`), multicast, broadcast, unspecified, "this network"
/// (`0.0.0.0/8`), IETF protocol assignments (`192.0.0.0/24`, which includes
/// the NAT64 discovery address `192.0.0.8/32`), benchmarking
/// (`198.18.0.0/15`) and reserved (`240.0.0.0/4`) addresses are all
/// rejected. An IPv4 address embedded in an IPv6 encoding — IPv4-mapped
/// (`::ffff:a.b.c.d`), NAT64 (`64:ff9b::/96`), 6to4 (`2002::/16`), or the
/// deprecated IPv4-compatible form (`::a.b.c.d`) — is extracted and checked
/// as its IPv4 form, so none of those encodings can smuggle a private
/// address past the IPv4-specific checks. `64:ff9b:1::/48` (RFC 8215's
/// local-use NAT64 prefix) is rejected outright rather than decoded: unlike
/// the other encodings, extracting its embedded address for prefix lengths
/// other than /96 requires RFC 6052 §2.2's non-contiguous bit layout
/// (a reserved "u" field splits the IPv4 payload), and the prefix exists
/// specifically to carry private IPv4 addresses, so treating the whole
/// prefix as disallowed is both simpler and strictly safer than attempting
/// a bit-exact decode.
///
/// A std `IpAddr::is_global()` allowlist check exists but is nightly-only
/// (feature `ip`, confirmed unstable against this repo's pinned 1.97
/// toolchain by direct compilation, not assumed) and out of reach on a
/// stable toolchain; no third-party crate was added for this because the
/// available candidate already in this dependency tree (`ipnet`) is CIDR
/// arithmetic, not a maintained IANA special-purpose registry, and this
/// sandbox has no network access to vet an unfamiliar crate for a security
/// boundary. The existing hand-rolled, individually-documented denylist
/// pattern is extended instead.
fn is_disallowed_unsubscribe_target(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_disallowed_ipv4(v4),
        IpAddr::V6(v6) => {
            if is_fully_disallowed_v6_prefix(v6) {
                return true;
            }
            if let Some(embedded) = embedded_ipv4(v6) {
                return is_disallowed_ipv4(embedded);
            }
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || is_unique_local_v6(v6)
                || is_link_local_v6(v6)
                || is_deprecated_site_local_v6(v6)
        }
    }
}

fn is_disallowed_ipv4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || is_cgnat_v4(v4)
        || is_this_network_v4(v4)
        || is_ietf_protocol_assignment_v4(v4)
        || is_benchmarking_v4(v4)
        || is_reserved_v4(v4)
}

/// `100.64.0.0/10`, the shared address space reserved for carrier-grade NAT —
/// not covered by `Ipv4Addr::is_private()`, which only knows RFC 1918.
fn is_cgnat_v4(v4: std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 100 && (o[1] & 0b1100_0000) == 64
}

/// `0.0.0.0/8`, "this network" — not covered by `is_unspecified()`, which
/// only matches the single address `0.0.0.0`.
fn is_this_network_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.octets()[0] == 0
}

/// `192.0.0.0/24`, IETF protocol assignments — includes `192.0.0.8/32`, the
/// NAT64 discovery address.
fn is_ietf_protocol_assignment_v4(v4: std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 192 && o[1] == 0 && o[2] == 0
}

/// `198.18.0.0/15`, reserved for network benchmarking.
fn is_benchmarking_v4(v4: std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 198 && (o[1] & 0b1111_1110) == 18
}

/// `240.0.0.0/4`, reserved for future use (includes the broadcast address,
/// already covered separately by `is_broadcast()`).
fn is_reserved_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.octets()[0] >= 240
}

/// `fc00::/7`, IPv6 unique-local addresses — the IPv6 analog of RFC 1918,
/// with no stable `is_unique_local()` in std at this edition.
fn is_unique_local_v6(v6: std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// `fe80::/10`, IPv6 link-local addresses — no stable named method in std
/// at this edition.
fn is_link_local_v6(v6: std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

/// `fec0::/10`, the deprecated IPv6 site-local range (formally deprecated by
/// RFC 3879, but still routable on hosts/stacks that haven't dropped it) —
/// no stable named method in std at this edition.
fn is_deprecated_site_local_v6(v6: std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfec0
}

/// `64:ff9b:1::/48` (RFC 8215) and Teredo (`2001:0000::/32`, RFC 4380),
/// rejected outright — see this module's top-level doc comment for why
/// they aren't decoded. Teredo's embedded client address is XOR-obfuscated
/// per RFC 4380, not a simple bit-extraction like NAT64/6to4, and nothing
/// legitimate would ever serve a List-Unsubscribe target over a Teredo
/// tunnel.
fn is_fully_disallowed_v6_prefix(v6: std::net::Ipv6Addr) -> bool {
    let s = v6.segments();
    (s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0x0001) || (s[0] == 0x2001 && s[1] == 0x0000)
}

/// Extract an IPv4 address embedded in an IPv6 encoding, where the payload
/// is recoverable without RFC 6052's non-contiguous bit layout: IPv4-mapped
/// (`::ffff:a.b.c.d`), the NAT64 well-known `/96` prefix (`64:ff9b::/96`,
/// last 32 bits), 6to4 (`2002::/16`, next 32 bits), and the deprecated
/// IPv4-compatible form (`::a.b.c.d`, last 32 bits with the top 96 bits
/// zero). Returns `None` for `::` and `::1` (unspecified/loopback), which
/// are handled by the existing IPv6-specific checks instead.
fn embedded_ipv4(v6: std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    use std::net::Ipv4Addr;

    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }

    let s = v6.segments();
    let last_two_as_v4 = |s: &[u16; 8]| {
        Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8)
    };

    // NAT64 well-known prefix: 64:ff9b::/96 — last 32 bits are the IPv4.
    if s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 {
        return Some(last_two_as_v4(&s));
    }

    // 6to4: 2002::/16 — the next 32 bits (segments 1-2) are the IPv4.
    if s[0] == 0x2002 {
        return Some(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }

    // Deprecated IPv4-compatible form: top 96 bits zero, excluding `::`
    // and `::1` (unspecified/loopback), which the caller checks itself.
    let top_96_zero = s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0;
    let is_unspecified_or_loopback = s[6] == 0 && (s[7] == 0 || s[7] == 1);
    if top_96_zero && !is_unspecified_or_loopback {
        return Some(last_two_as_v4(&s));
    }

    None
}

/// Parse and validate a `List-Unsubscribe` URL before this engine fetches
/// it. Requires `https`; resolves the host itself (rather than letting the
/// HTTP client resolve blind) and rejects it if any resolved address is
/// disallowed per [`is_disallowed_unsubscribe_target`]. Returns the parsed
/// URL and the one address the actual connection will be pinned to, so a
/// second DNS lookup inside the HTTP client can't land on a different,
/// unvalidated address between this check and the request
/// (DNS-rebinding).
async fn validate_unsubscribe_url(url_str: &str) -> Result<(reqwest::Url, SocketAddr), String> {
    let url = reqwest::Url::parse(url_str).map_err(|e| format!("invalid unsubscribe url: {e}"))?;

    if url.scheme() != "https" {
        return Err(format!(
            "unsubscribe url must use https, got {:?}",
            url.scheme()
        ));
    }

    let host = url
        .host_str()
        .ok_or_else(|| "unsubscribe url has no host".to_string())?
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);

    let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| format!("dns resolution failed for {host}: {e}"))?
        .collect();

    if resolved.is_empty() {
        return Err(format!("dns resolution for {host} returned no addresses"));
    }

    for addr in &resolved {
        if is_disallowed_unsubscribe_target(addr.ip()) {
            return Err(format!(
                "unsubscribe url {host} resolves to a disallowed address ({}); refusing to fetch an internal/private/loopback target",
                addr.ip()
            ));
        }
    }

    // Every resolved address was just validated above, so pinning to the
    // first one is as safe as pinning to any other.
    Ok((url, resolved[0]))
}

/// Build a client scoped to exactly one validated, pinned address: no
/// automatic redirect-following (a validated host could otherwise hand back
/// a redirect to an internal target and have it followed silently — a
/// redirect target must be validated from scratch, which means re-entering
/// this module's checks, not quietly trusting the client's redirect logic),
/// `.resolve()` so the connection cannot be re-resolved to a different
/// address than the one just validated, and `.no_proxy()` so an
/// `HTTP_PROXY`/`HTTPS_PROXY` environment variable can't silently bypass
/// the pinned address entirely: reqwest honors those variables by default,
/// and a proxy does its own DNS resolution using the original hostname, so
/// `.resolve()` alone would never even apply once a proxy is in the path.
/// This is scoped to this one client; any other HTTP client elsewhere in
/// the app that legitimately needs proxy support is unaffected.
fn pinned_https_client(host: &str, pinned: SocketAddr) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Emailibrium/1.0 (unsubscribe)")
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, pinned)
        .no_proxy()
        .build()
        .map_err(|e| format!("failed to build unsubscribe http client: {e}"))
}

/// Execute an unsubscribe request using the given method.
///
/// - `HttpPost`: sends a POST with `List-Unsubscribe=One-Click` body (RFC 8058).
/// - `HttpGet`: sends a GET request to the unsubscribe URL.
/// - `Mailto`: returns success (actual sending requires SMTP, which is logged
///   but not executed in the local-first architecture).
///
/// `HttpPost` and `HttpGet` both validate the URL first (see
/// [`validate_unsubscribe_url`]) — it comes straight from an email header,
/// so it is treated as untrusted input, never fetched blind.
pub async fn execute_unsubscribe(method: &UnsubscribeMethod) -> Result<UnsubscribeResult, String> {
    match method {
        UnsubscribeMethod::HttpPost { url } => {
            let (parsed, pinned) = validate_unsubscribe_url(url).await?;
            let host = parsed.host_str().unwrap_or_default().to_string();
            let client = pinned_https_client(&host, pinned)?;
            debug!(url = %url, pinned = %pinned, "Executing HTTP POST unsubscribe (RFC 8058)");
            let resp = client
                .post(parsed)
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
            let (parsed, pinned) = validate_unsubscribe_url(url).await?;
            let host = parsed.host_str().unwrap_or_default().to_string();
            let client = pinned_https_client(&host, pinned)?;
            debug!(url = %url, pinned = %pinned, "Executing HTTP GET unsubscribe");
            let resp = client
                .get(parsed)
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
///
/// Holds no shared `reqwest::Client`: each `HttpPost`/`HttpGet` execution
/// builds its own client, scoped to the one validated, pinned address that
/// request's URL resolved to (see `validate_unsubscribe_url` and
/// `pinned_https_client` above) — a client shared across different
/// unsubscribe targets couldn't carry a single pinned address correctly.
pub struct UnsubscribeService {
    undo_buffer: Arc<Mutex<HashMap<String, UndoEntry>>>,
    undo_window: Duration,
}

impl UnsubscribeService {
    /// Create a new unsubscribe service.
    pub fn new() -> Self {
        Self {
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

        match execute_unsubscribe(method).await {
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

    // -----------------------------------------------------------------
    // SSRF fix: `is_disallowed_unsubscribe_target` / `validate_unsubscribe_url`
    // -----------------------------------------------------------------

    #[test]
    fn disallowed_targets_cover_loopback_private_link_local_cgnat_and_v6_equivalents() {
        let disallowed: &[&str] = &[
            "127.0.0.1",     // loopback v4
            "10.0.0.1",      // RFC 1918 private
            "172.16.0.1",    // RFC 1918 private
            "192.168.1.1",   // RFC 1918 private
            "169.254.1.1",   // link-local v4
            "100.64.0.1",    // CGNAT, not covered by Ipv4Addr::is_private()
            "0.0.0.0",       // unspecified
            "255.255.255.255", // broadcast
            "224.0.0.1",     // multicast v4
            "::1",           // loopback v6
            "fc00::1",       // unique-local v6
            "fe80::1",       // link-local v6
            "ff02::1",       // multicast v6
            "::ffff:127.0.0.1", // IPv4-mapped IPv6 loopback — must unwrap, not bypass
            "::ffff:10.0.0.1",  // IPv4-mapped IPv6 private — must unwrap, not bypass
        ];
        for ip_str in disallowed {
            let ip: IpAddr = ip_str.parse().unwrap();
            assert!(
                is_disallowed_unsubscribe_target(ip),
                "{ip_str} should be disallowed"
            );
        }
    }

    #[test]
    fn disallowed_targets_cover_the_round_2_ipv6_encodings_and_ipv4_ranges() {
        let disallowed: &[&str] = &[
            "64:ff9b::7f00:1",   // NAT64 /96, encodes 127.0.0.1
            "64:ff9b::a00:1",    // NAT64 /96, encodes 10.0.0.1
            "2002:c0a8:101::",   // 6to4, encodes 192.168.1.1
            "2002:a00:1::",      // 6to4, encodes 10.0.0.1
            "::127.0.0.1",       // deprecated IPv4-compatible, loopback
            "::10.0.0.1",        // deprecated IPv4-compatible, private
            "64:ff9b:1::7f00:1", // RFC 8215 local-use NAT64 — rejected outright, not decoded
            "0.0.0.0",           // already covered via is_unspecified, kept for 0.0.0.0/8 overlap
            "0.1.2.3",           // 0.0.0.0/8, "this network"
            "192.0.0.1",         // 192.0.0.0/24, IETF protocol assignment
            "192.0.0.8",         // NAT64 discovery address, inside 192.0.0.0/24
            "198.18.0.1",        // 198.18.0.0/15, benchmarking
            "198.19.255.254",    // still inside 198.18.0.0/15
            "240.0.0.1",         // 240.0.0.0/4, reserved
            "255.0.0.1",         // still inside 240.0.0.0/4
            "fec0::1",           // deprecated IPv6 site-local, fec0::/10
            "feff:ffff::1",      // still inside fec0::/10
            "2001:0:4136:e378::1", // Teredo, 2001:0000::/32 — rejected outright
        ];
        for ip_str in disallowed {
            let ip: IpAddr = ip_str.parse().unwrap();
            assert!(
                is_disallowed_unsubscribe_target(ip),
                "{ip_str} should be disallowed"
            );
        }
    }

    #[test]
    fn deprecated_site_local_and_teredo_do_not_overreach_their_boundaries() {
        // fec0::/10 is fec0:: through feff:ffff:...:ffff. fe80::1 (link-local,
        // already covered) and a plain global address must not be caught by
        // the site-local check specifically.
        assert!(!is_deprecated_site_local_v6("2001:4860:4860::8888".parse().unwrap()));
        // 2001:1::1 is outside Teredo's 2001:0000::/32 (note the second
        // segment must be zero) — a global 2001:: address must not be
        // rejected just for sharing Teredo's top 16 bits.
        let ip: IpAddr = "2001:1::1".parse().unwrap();
        assert!(!is_disallowed_unsubscribe_target(ip));
    }

    #[test]
    fn benchmarking_range_does_not_overreach_its_boundaries() {
        // 198.18.0.0/15 is 198.18.0.0-198.19.255.255. Addresses just
        // outside that boundary on both sides must NOT be disallowed by
        // this check, proving the bitmask is exact rather than overbroad.
        let just_outside: &[&str] = &["198.17.255.255", "198.20.0.0"];
        for ip_str in just_outside {
            let ip: IpAddr = ip_str.parse().unwrap();
            assert!(
                !is_disallowed_unsubscribe_target(ip),
                "{ip_str} is just outside 198.18.0.0/15 and should NOT be disallowed"
            );
        }
    }

    #[test]
    fn six_to_four_encoding_of_a_genuinely_public_address_is_not_disallowed() {
        // 2002:0808:0808:: encodes 8.8.8.8 via 6to4 — a public address, so
        // the 6to4 decode path must not reject it just for being 6to4.
        let ip: IpAddr = "2002:808:808::".parse().unwrap();
        assert!(
            !is_disallowed_unsubscribe_target(ip),
            "6to4 encoding of a public IPv4 address should not be disallowed"
        );
    }

    #[test]
    fn public_looking_addresses_are_not_disallowed() {
        // Proof that the validation isn't simply rejecting everything: known
        // public addresses (not reachable/dialed in this test, just checked
        // against the same predicate production code uses) must pass.
        let public: &[&str] = &["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"];
        for ip_str in public {
            let ip: IpAddr = ip_str.parse().unwrap();
            assert!(
                !is_disallowed_unsubscribe_target(ip),
                "{ip_str} should NOT be disallowed"
            );
        }
    }

    #[tokio::test]
    async fn validate_rejects_plain_http_before_any_dns_or_network_activity() {
        // No network access in this sandbox for a real host, and none
        // should be needed: the scheme check must short-circuit before DNS.
        let err = validate_unsubscribe_url("http://example.invalid/unsub")
            .await
            .unwrap_err();
        assert!(err.contains("https"), "error should mention https: {err}");
    }

    #[tokio::test]
    async fn validate_rejects_a_url_that_resolves_to_loopback() {
        let err = validate_unsubscribe_url("https://localhost/unsub")
            .await
            .unwrap_err();
        assert!(
            err.contains("disallowed address"),
            "error should name the disallowed-address reason: {err}"
        );
    }

    #[tokio::test]
    async fn validate_rejects_a_literal_private_ip_target() {
        let err = validate_unsubscribe_url("https://192.168.1.1/unsub")
            .await
            .unwrap_err();
        assert!(
            err.contains("disallowed address"),
            "error should name the disallowed-address reason: {err}"
        );
    }

    /// Proves the exact redirect policy `pinned_https_client` builds with
    /// (`redirect::Policy::none()`) does not chase a 3xx response: the
    /// server's hit counter stays at 1, and the 302 status is returned to
    /// the caller as-is rather than silently followed to its `Location`.
    ///
    /// This runs over plain HTTP against a local 127.0.0.1 test server,
    /// not through `validate_unsubscribe_url`/`pinned_https_client`'s full
    /// pipeline: that pipeline requires `https` and rejects loopback by
    /// design (proven above), so it cannot be driven end-to-end against a
    /// same-process test server without building self-signed-TLS test
    /// infrastructure this repo doesn't otherwise have. What's proven here
    /// is the one mechanism production code actually relies on to stop a
    /// validated host from redirecting the engine to an internal target:
    /// the client simply never follows a redirect, anywhere, ever.
    #[tokio::test]
    async fn redirect_none_policy_does_not_chase_a_3xx_response() {
        use axum::{response::Redirect, routing::get, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let hits = Arc::new(AtomicUsize::new(0));
        let hits_for_handler = hits.clone();
        let router = Router::new().route(
            "/start",
            get(move || {
                let hits = hits_for_handler.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Redirect::to("http://169.254.169.254/latest/meta-data/")
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            // Hermetic against an ambient HTTP_PROXY/HTTPS_PROXY in the
            // environment (the user's shell, CI, or this file's own
            // proxy-bypass test running concurrently in the same binary)
            // — this test is about redirect policy, not proxy behavior,
            // so it must not be sensitive to either.
            .no_proxy()
            .build()
            .unwrap();
        let resp = client
            .get(format!("http://{addr}/start"))
            .send()
            .await
            .unwrap();

        // axum's `Redirect::to` issues 303 See Other; what's under test is
        // that the client doesn't chase a redirect at all, regardless of
        // which 3xx code it carries.
        assert_eq!(resp.status().as_u16(), 303);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the redirect target must never be fetched, so the origin server \
             must have been hit exactly once"
        );

        task.abort();
    }

    /// Proves `.no_proxy()` actually suppresses a configured proxy — not
    /// just that the builder call compiles. A "fake proxy" TCP listener
    /// counts connection attempts but never speaks a real proxy protocol
    /// (it just drops whatever connects); a real target server answers
    /// directly. A control client with an explicit `reqwest::Proxy`
    /// configured (no `.no_proxy()`) must route through the fake proxy —
    /// proving the proxy is live and reqwest's default behavior honors
    /// it, the exact thing `pinned_https_client`'s `.no_proxy()` call
    /// exists to prevent (production relies on the env-var-detection form
    /// of the same mechanism, since that's what `HTTP_PROXY`/
    /// `HTTPS_PROXY` drive, but `.no_proxy()`'s documented job is to
    /// disable proxy use outright, so overriding an explicitly-configured
    /// proxy is at least as strong a guarantee as overriding env-var
    /// auto-detection — this avoids mutating real process-wide
    /// `HTTP_PROXY`/`HTTPS_PROXY` env vars, which would otherwise risk
    /// racing any other test in this binary that builds a bare
    /// `reqwest::Client` concurrently; an earlier version of this test did
    /// exactly that and broke `redirect_none_policy_does_not_chase_a_3xx_response`
    /// by sheer timing, which is why this version doesn't touch the
    /// environment at all). The same `.resolve(..).no_proxy()`
    /// combination `pinned_https_client` builds with must then reach the
    /// target directly, with the fake proxy's hit count unchanged.
    ///
    /// Runs over plain HTTP, not through `pinned_https_client` itself,
    /// for the same reason the redirect test above does: that function
    /// requires `https` and `validate_unsubscribe_url` rejects loopback
    /// targets by design, so the full pipeline can't be driven end to end
    /// against a same-process test server without self-signed-TLS test
    /// infrastructure this repo doesn't have.
    #[tokio::test]
    async fn no_proxy_setting_overrides_an_explicitly_configured_proxy() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let target_hits = Arc::new(AtomicUsize::new(0));
        let target_hits_for_handler = target_hits.clone();
        let target_router = axum::Router::new().route(
            "/",
            axum::routing::get(move || {
                let hits = target_hits_for_handler.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    "direct-ok"
                }
            }),
        );
        let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            axum::serve(target_listener, target_router).await.unwrap();
        });

        let proxy_hits = Arc::new(AtomicUsize::new(0));
        let proxy_hits_for_task = proxy_hits.clone();
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            loop {
                let Ok((_stream, _)) = proxy_listener.accept().await else {
                    break;
                };
                // Not a real proxy: just record the attempt and drop the
                // connection. A control client routed through this must
                // fail (or hang until it errors) rather than succeed —
                // the assertion below only needs the hit count, not a
                // successful response.
                proxy_hits_for_task.fetch_add(1, Ordering::SeqCst);
            }
        });

        let explicit_proxy = reqwest::Proxy::http(format!("http://{proxy_addr}")).unwrap();

        // Control: an explicitly configured proxy, no `.no_proxy()`, must
        // be used.
        let control_client = reqwest::Client::builder()
            .proxy(explicit_proxy.clone())
            .build()
            .unwrap();
        let _ = control_client
            .get(format!("http://{target_addr}/"))
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        assert!(
            proxy_hits.load(Ordering::SeqCst) >= 1,
            "control client (configured proxy, no .no_proxy()) must have attempted \
             the fake proxy, otherwise this test's proxy setup isn't actually \
             exercising reqwest's proxy path at all"
        );
        let hits_after_control = proxy_hits.load(Ordering::SeqCst);

        // Subject: the same proxy configured, PLUS the exact
        // `.resolve(..).no_proxy()` combination `pinned_https_client`
        // builds with — `.no_proxy()` must win.
        let pinned = SocketAddr::new(target_addr.ip(), target_addr.port());
        let no_proxy_client = reqwest::Client::builder()
            .proxy(explicit_proxy)
            .resolve("example.invalid", pinned)
            .no_proxy()
            .build()
            .unwrap();
        let resp = no_proxy_client
            .get("http://example.invalid/")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .expect("no_proxy client must reach the target directly, overriding the configured proxy");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(resp.text().await.unwrap(), "direct-ok");
        assert_eq!(target_hits.load(Ordering::SeqCst), 1, "target must receive exactly the no_proxy client's request");
        assert_eq!(
            proxy_hits.load(Ordering::SeqCst),
            hits_after_control,
            ".no_proxy() must mean the fake proxy receives no further connections \
             after the control request"
        );

        target_task.abort();
        proxy_task.abort();
    }
}
