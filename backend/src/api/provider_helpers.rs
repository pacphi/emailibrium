//! Shared helpers for resolving email providers from account state.

use std::net::IpAddr;

use axum::http::StatusCode;

use crate::email::provider::EmailProvider;
use crate::email::types::{ProviderConfig, ProviderKind};
use crate::AppState;

/// Ports a user-supplied IMAP server may use.
pub const ALLOWED_IMAP_PORTS: &[u16] = &[
    143,  // IMAP STARTTLS
    993,  // IMAP implicit TLS
    1143, // ProtonMail Bridge (local)
];

/// Ports a user-supplied SMTP server may use. Plaintext port 25 is excluded —
/// submission should use implicit TLS (465) or STARTTLS submission (587).
pub const ALLOWED_SMTP_PORTS: &[u16] = &[
    465,  // SMTP implicit TLS
    587,  // SMTP submission/STARTTLS
    1025, // ProtonMail Bridge (local)
];

/// Returns true if an IP must NOT be reachable from a user-controlled mail
/// hostname. Blocks private (RFC1918), link-local (incl. the cloud metadata
/// endpoint 169.254.169.254), CGNAT (100.64/10), multicast, unspecified, and
/// broadcast addresses. Loopback is intentionally ALLOWED so local mail
/// proxies (e.g. ProtonMail Bridge on 127.0.0.1) still work in self-hosted
/// deployments.
fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                // CGNAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 0x40)
        }
        IpAddr::V6(v6) => {
            v6.is_multicast()
                || v6.is_unspecified()
                // Unique local (fc00::/7) and link-local (fe80::/10).
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4-mapped: re-check against the v4 rules.
                || v6
                    .to_ipv4_mapped()
                    .map(|m| is_blocked_ip(&IpAddr::V4(m)))
                    .unwrap_or(false)
        }
    }
}

/// Validate a user-supplied mail host:port against SSRF abuse before any
/// connection attempt. Resolves the hostname and rejects the request if the
/// port is not in `allowed_ports` or ANY resolved address is in a blocked
/// range. Returns a generic `BAD_REQUEST` to the caller (details are logged
/// server-side) so the endpoint can't be used to probe the internal network.
///
/// On success returns the first validated `SocketAddr`. Callers should connect
/// to THIS address (pinning it) rather than re-resolving the hostname, to
/// avoid a DNS-rebinding TOCTOU between this check and the connect.
pub async fn guard_mail_host(
    host: &str,
    port: u16,
    allowed_ports: &[u16],
) -> Result<std::net::SocketAddr, (StatusCode, String)> {
    let reject = || {
        (
            StatusCode::BAD_REQUEST,
            "Invalid mail server address".to_string(),
        )
    };

    if !allowed_ports.contains(&port) {
        tracing::warn!(host, port, "Rejected mail host: port not allowed");
        return Err(reject());
    }

    // Resolve and inspect every address the hostname maps to. Reject if ANY is
    // in a blocked range (defends against multi-record rebinding tricks).
    let addrs = tokio::net::lookup_host((host, port)).await.map_err(|e| {
        tracing::warn!(host, port, error = %e, "Rejected mail host: DNS resolution failed");
        reject()
    })?;

    let mut first: Option<std::net::SocketAddr> = None;
    for addr in addrs {
        if is_blocked_ip(&addr.ip()) {
            tracing::warn!(host, port, ip = %addr.ip(), "Rejected mail host: blocked IP range");
            return Err(reject());
        }
        first.get_or_insert(addr);
    }

    first.ok_or_else(|| {
        tracing::warn!(host, port, "Rejected mail host: no addresses resolved");
        reject()
    })
}

pub fn resolve_gmail_config(state: &AppState) -> Result<ProviderConfig, (StatusCode, String)> {
    let oauth = &state.vector_service.config.oauth;
    let gmail = &oauth.gmail;

    let client_id = std::env::var(&gmail.client_id_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Gmail OAuth not configured: missing env var {}",
                gmail.client_id_env
            ),
        )
    })?;
    let client_secret = std::env::var(&gmail.client_secret_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Gmail OAuth not configured: missing env var {}",
                gmail.client_secret_env
            ),
        )
    })?;

    Ok(ProviderConfig {
        client_id,
        client_secret,
        redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
        auth_url: gmail.auth_url.clone(),
        token_url: gmail.token_url.clone(),
        scopes: gmail.scopes.clone(),
    })
}

pub fn resolve_outlook_config(state: &AppState) -> Result<ProviderConfig, (StatusCode, String)> {
    let oauth = &state.vector_service.config.oauth;
    let outlook = &oauth.outlook;

    let client_id = std::env::var(&outlook.client_id_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Outlook OAuth not configured: missing env var {}",
                outlook.client_id_env
            ),
        )
    })?;
    let client_secret = std::env::var(&outlook.client_secret_env).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Outlook OAuth not configured: missing env var {}",
                outlook.client_secret_env
            ),
        )
    })?;

    Ok(ProviderConfig {
        client_id,
        client_secret,
        redirect_uri: format!("{}/api/v1/auth/callback", oauth.redirect_base_url),
        auth_url: outlook.auth_url(),
        token_url: outlook.token_url(),
        scopes: outlook.scopes.clone(),
    })
}

/// SSRF-guard an IMAP host/port for callers outside the axum layer (e.g. the
/// cleanup factory). Returns the validated `SocketAddr` or a plain error string.
pub async fn guard_imap_addr(host: &str, port: u16) -> Result<std::net::SocketAddr, String> {
    guard_mail_host(host, port, ALLOWED_IMAP_PORTS)
        .await
        .map_err(|_| format!("IMAP host {host}:{port} rejected by SSRF guard"))
}

/// Run the SSRF guard against an `ImapConfig`'s IMAP host/port and return the
/// config with `pinned_addr` set to the validated address. Use this everywhere
/// an IMAP connection is about to be made (initial connect AND later sync), so
/// a stored malicious host can't bypass the guard at sync time and so the
/// connect pins the validated IP.
pub async fn guard_and_pin_imap_config(
    mut config: crate::email::imap::ImapConfig,
) -> Result<crate::email::imap::ImapConfig, (StatusCode, String)> {
    let addr = guard_mail_host(&config.host, config.port, ALLOWED_IMAP_PORTS).await?;
    config.pinned_addr = Some(addr);
    Ok(config)
}

/// Build a provider instance and get the access token for an account.
pub async fn resolve_provider_and_token(
    state: &AppState,
    account_id: &str,
) -> Result<(Box<dyn EmailProvider>, String, ProviderKind), (StatusCode, String)> {
    let accounts = state
        .oauth_manager
        .list_accounts()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let account = accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Account not found".to_string()))?;

    // Branch on provider BEFORE fetching an access token: IMAP/POP3 use stored
    // credentials, not OAuth tokens, so `get_access_token` does not apply.
    let (provider, access_token): (Box<dyn EmailProvider>, String) = match account.provider {
        ProviderKind::Gmail => {
            let token = state
                .oauth_manager
                .get_access_token(account_id)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Token error: {e}")))?;
            let config = resolve_gmail_config(state)?;
            (
                Box::new(crate::email::gmail::GmailProvider::new(config)),
                token,
            )
        }
        ProviderKind::Outlook => {
            let token = state
                .oauth_manager
                .get_access_token(account_id)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Token error: {e}")))?;
            let config = resolve_outlook_config(state)?;
            (
                Box::new(crate::email::outlook::OutlookProvider::new(config)),
                token,
            )
        }
        ProviderKind::Imap => {
            let config = state
                .oauth_manager
                .load_imap_config(account_id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("IMAP config: {e}"),
                    )
                })?;
            // Re-validate the stored host against SSRF and pin the resolved IP.
            let config = guard_and_pin_imap_config(config).await?;
            // IMAP re-derives its session from stored credentials; the access
            // token argument is ignored by ImapProvider.
            (
                Box::new(crate::email::imap::ImapProvider::new(config)),
                String::new(),
            )
        }
        ProviderKind::Pop3 => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Provider {} not supported for this operation",
                    account.provider.as_str()
                ),
            ));
        }
    };

    Ok((provider, access_token, account.provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn blocks_loopback_is_allowed() {
        // Loopback is intentionally ALLOWED (ProtonMail Bridge etc.).
        assert!(!is_blocked_ip(&ip("127.0.0.1")));
        assert!(!is_blocked_ip(&ip("::1")));
    }

    #[test]
    fn blocks_private_ranges() {
        assert!(is_blocked_ip(&ip("10.0.0.1")));
        assert!(is_blocked_ip(&ip("172.16.5.4")));
        assert!(is_blocked_ip(&ip("192.168.1.1")));
    }

    #[test]
    fn blocks_link_local_and_cloud_metadata() {
        assert!(is_blocked_ip(&ip("169.254.0.1")));
        // The AWS/GCP metadata endpoint is the canonical SSRF target.
        assert!(is_blocked_ip(&ip("169.254.169.254")));
    }

    #[test]
    fn blocks_cgnat_multicast_unspecified() {
        assert!(is_blocked_ip(&ip("100.64.0.1"))); // CGNAT 100.64/10
        assert!(is_blocked_ip(&ip("100.127.255.255")));
        assert!(is_blocked_ip(&ip("224.0.0.1"))); // multicast
        assert!(is_blocked_ip(&ip("0.0.0.0"))); // unspecified
    }

    #[test]
    fn allows_public_ips() {
        assert!(!is_blocked_ip(&ip("8.8.8.8")));
        assert!(!is_blocked_ip(&ip("142.250.80.46"))); // a Google IP
        assert!(!is_blocked_ip(&ip("2607:f8b0:4005:80a::200e")));
    }

    #[test]
    fn blocks_ipv6_ula_and_linklocal_and_mapped() {
        assert!(is_blocked_ip(&ip("fc00::1"))); // unique local
        assert!(is_blocked_ip(&ip("fe80::1"))); // link-local
                                                // IPv4-mapped IPv6 must be re-checked against v4 rules.
        assert!(!is_blocked_ip(&ip("::ffff:127.0.0.1"))); // mapped loopback -> allowed
        assert!(is_blocked_ip(&ip("::ffff:10.0.0.1"))); // mapped private -> blocked
    }

    #[tokio::test]
    async fn guard_rejects_disallowed_port() {
        // Port 9999 isn't in the IMAP allowlist; must reject before any DNS.
        let res = guard_mail_host("imap.gmail.com", 9999, ALLOWED_IMAP_PORTS).await;
        assert!(res.is_err());
    }
}
