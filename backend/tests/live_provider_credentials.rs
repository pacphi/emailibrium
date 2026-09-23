//! Opt-in credential contracts for dedicated test accounts.
//!
//! Default runs execute synthetic guards only. Each ignored live test requires
//! explicit selection and all four dedicated TEST settings; missing configuration
//! is an error, never a skip-pass. No .env loading or application startup occurs.
//!
//! Live scope: refresh a token, verify the primary profile identity, then read
//! label/category definitions. Never call list_messages: current adapters fetch
//! message bodies there. No mail, label, read-state or filing mutations occur.
//! Tokens, actual identities, label names and raw provider errors are not printed.
//! Google uses gmail.readonly; Graph uses User.Read and MailboxSettings.Read.

use emailibrium::email::gmail::GmailProvider;
use emailibrium::email::outlook::OutlookProvider;
use emailibrium::email::provider::{EmailProvider, ProviderError};
use emailibrium::email::types::ProviderConfig;
use std::future::Future;
use std::time::Duration;
use tracing::instrument::WithSubscriber;

const MICROSOFT_TENANT: &str = "EMAILIBRIUM_TEST_MICROSOFT_TENANT_ID";
const LIVE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
enum Service {
    Google,
    Microsoft,
}

impl Service {
    fn keys(self) -> [&'static str; 4] {
        match self {
            Self::Google => [
                "EMAILIBRIUM_TEST_GOOGLE_CLIENT_ID",
                "EMAILIBRIUM_TEST_GOOGLE_CLIENT_SECRET",
                "EMAILIBRIUM_TEST_GOOGLE_REFRESH_TOKEN",
                "EMAILIBRIUM_TEST_GOOGLE_EXPECTED_EMAIL",
            ],
            Self::Microsoft => [
                "EMAILIBRIUM_TEST_MICROSOFT_CLIENT_ID",
                "EMAILIBRIUM_TEST_MICROSOFT_CLIENT_SECRET",
                "EMAILIBRIUM_TEST_MICROSOFT_REFRESH_TOKEN",
                "EMAILIBRIUM_TEST_MICROSOFT_EXPECTED_EMAIL",
            ],
        }
    }
}

// Deliberately no Debug implementation: credentials must never enter assertions.
struct Credentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    expected_email: String,
    tenant: String,
}

#[derive(Debug, thiserror::Error)]
enum ProbeError {
    #[error("required test setting is missing: {0}")]
    MissingConfiguration(&'static str),
    #[error("Microsoft test tenant is invalid")]
    InvalidTenant,
    #[error("{stage}: {category}")]
    Provider {
        stage: &'static str,
        category: &'static str,
    },
    #[error("provider returned an unexpected account identity")]
    IdentityMismatch,
    #[error("provider returned an empty access token")]
    EmptyAccessToken,
    #[error("provider credential check timed out")]
    Timeout,
    #[error("Gmail metadata is missing required system fields")]
    InvalidMetadata,
}

fn required(
    key: &'static str,
    lookup: &mut impl FnMut(&str) -> Option<String>,
) -> Result<String, ProbeError> {
    lookup(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ProbeError::MissingConfiguration(key))
}

fn tenant(value: Option<String>) -> Result<String, ProbeError> {
    let value = value.unwrap_or_else(|| "common".into());
    if matches!(value.as_str(), "common" | "organizations" | "consumers")
        || uuid::Uuid::parse_str(&value).is_ok()
    {
        Ok(value)
    } else {
        Err(ProbeError::InvalidTenant)
    }
}

fn credentials(
    service: Service,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> Result<Credentials, ProbeError> {
    let [id, secret, refresh, email] = service.keys();
    Ok(Credentials {
        client_id: required(id, &mut lookup)?,
        client_secret: required(secret, &mut lookup)?,
        refresh_token: required(refresh, &mut lookup)?,
        expected_email: required(email, &mut lookup)?,
        tenant: match service {
            Service::Google => "common".into(),
            Service::Microsoft => tenant(lookup(MICROSOFT_TENANT))?,
        },
    })
}

// ProviderError strings can contain response bodies, URLs and fresh tokens.
// Retain only the typed category; the adapters do not preserve a typed status.
fn provider_failure(stage: &'static str, error: ProviderError) -> ProbeError {
    let category = match error {
        ProviderError::OAuthError(_) => "oauth",
        ProviderError::RequestFailed(_) => "request",
        ProviderError::TokenExpired(_) => "token_refresh",
        ProviderError::RateLimited { .. } => "rate_limited",
        ProviderError::NotFound(_) => "not_found",
        ProviderError::ConfigError(_) => "configuration",
        ProviderError::ParseError(_) => "parse",
    };
    ProbeError::Provider { stage, category }
}

fn require_access_token(token: &str) -> Result<(), ProbeError> {
    if token.trim().is_empty() {
        Err(ProbeError::EmptyAccessToken)
    } else {
        Ok(())
    }
}

async fn verified_metadata<T, F, Fut>(
    expected: &str,
    actual: &str,
    read: F,
) -> Result<T, ProbeError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, ProbeError>>,
{
    if actual.trim().is_empty() || !expected.trim().eq_ignore_ascii_case(actual.trim()) {
        return Err(ProbeError::IdentityMismatch);
    }
    read().await
}

async fn quiet_operation<T>(
    deadline: Duration,
    operation: impl Future<Output = Result<T, ProbeError>>,
) -> Result<T, ProbeError> {
    tokio::time::timeout(
        deadline,
        operation.with_subscriber(tracing::subscriber::NoSubscriber::default()),
    )
    .await
    .map_err(|_| ProbeError::Timeout)?
}

fn validate_gmail_metadata(labels: &[(String, String)]) -> Result<(), ProbeError> {
    if labels.iter().any(|(id, _)| id == "INBOX")
        && labels
            .iter()
            .all(|(id, name)| !id.is_empty() && !name.is_empty())
    {
        Ok(())
    } else {
        Err(ProbeError::InvalidMetadata)
    }
}

fn provider_config(service: Service, credentials: &Credentials) -> ProviderConfig {
    let (auth_url, token_url, scopes) = match service {
        Service::Google => (
            "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            "https://oauth2.googleapis.com/token".to_owned(),
            vec!["https://www.googleapis.com/auth/gmail.readonly".to_owned()],
        ),
        Service::Microsoft => (
            format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
                credentials.tenant
            ),
            format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                credentials.tenant
            ),
            vec![
                "https://graph.microsoft.com/User.Read".to_owned(),
                "https://graph.microsoft.com/MailboxSettings.Read".to_owned(),
                "offline_access".to_owned(),
            ],
        ),
    };
    ProviderConfig {
        client_id: credentials.client_id.clone(),
        client_secret: credentials.client_secret.clone(),
        // Refresh-token requests do not use a redirect or start a callback server.
        redirect_uri: "http://127.0.0.1/unused-test-callback".into(),
        auth_url,
        token_url,
        scopes,
    }
}

#[tokio::test]
#[ignore = "explicit dedicated Google test-account credentials required; read-only live network"]
async fn gmail_credentials_read_only() -> Result<(), ProbeError> {
    quiet_operation(LIVE_TIMEOUT, async {
        let settings = credentials(Service::Google, |key| std::env::var(key).ok())?;
        let provider = GmailProvider::new(provider_config(Service::Google, &settings));
        let tokens = provider
            .refresh_token(&settings.refresh_token)
            .await
            .map_err(|error| provider_failure("gmail refresh", error))?;
        require_access_token(&tokens.access_token)?;
        let identity = provider
            .get_user_email(&tokens.access_token)
            .await
            .map_err(|error| provider_failure("gmail identity", error))?;
        let labels = verified_metadata(&settings.expected_email, &identity, || async {
            provider
                .list_labels(&tokens.access_token)
                .await
                .map_err(|error| provider_failure("gmail metadata", error))
        })
        .await?;
        validate_gmail_metadata(&labels)?;
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "explicit dedicated Microsoft test-account credentials required; read-only live network"]
async fn outlook_credentials_read_only() -> Result<(), ProbeError> {
    quiet_operation(LIVE_TIMEOUT, async {
        let settings = credentials(Service::Microsoft, |key| std::env::var(key).ok())?;
        let provider = OutlookProvider::new(provider_config(Service::Microsoft, &settings));
        let tokens = provider
            .refresh_token(&settings.refresh_token)
            .await
            .map_err(|error| provider_failure("outlook refresh", error))?;
        require_access_token(&tokens.access_token)?;
        let identity = provider
            .get_user_email(&tokens.access_token)
            .await
            .map_err(|error| provider_failure("outlook identity", error))?;
        let categories = verified_metadata(&settings.expected_email, &identity, || async {
            provider
                .list_labels(&tokens.access_token)
                .await
                .map_err(|error| provider_failure("outlook metadata", error))
        })
        .await?;
        assert!(
            categories
                .iter()
                .all(|(id, name)| !id.is_empty() && !name.is_empty()),
            "provider metadata is malformed"
        );
        Ok(())
    })
    .await
}

#[cfg(test)]
mod synthetic {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing::instrument::WithSubscriber;

    fn settings(service: Service) -> BTreeMap<String, String> {
        service
            .keys()
            .into_iter()
            .map(|key| (key.into(), "SYNTHETIC-configuration-canary".into()))
            .collect()
    }

    #[test]
    fn every_dedicated_setting_is_required_even_with_ambient_app_values() {
        for service in [Service::Google, Service::Microsoft] {
            for missing in service.keys() {
                for value in [None, Some(" \n\t".to_owned())] {
                    let mut values = settings(service);
                    values.remove(missing);
                    if let Some(value) = value {
                        values.insert(missing.into(), value);
                    }
                    values.insert(
                        missing.replace("EMAILIBRIUM_TEST_", ""),
                        "SYNTHETIC-production-fallback".into(),
                    );
                    let error = credentials(service, |key| values.get(key).cloned())
                        .err()
                        .expect("missing test setting must fail");
                    assert!(
                        matches!(error, ProbeError::MissingConfiguration(key) if key == missing)
                    );
                    let output = format!("{error:?} {error}");
                    assert!(!output.contains("SYNTHETIC"));
                }
            }
        }
    }

    #[test]
    fn provider_failure_output_never_contains_error_payloads() {
        let payload = "SYNTHETIC-token-canary https://provider.test/?refresh_token=SYNTHETIC-secret email=private@example.test";
        let cases = [
            ProviderError::OAuthError(payload.into()),
            ProviderError::RequestFailed(payload.into()),
            ProviderError::TokenExpired(payload.into()),
            ProviderError::NotFound(payload.into()),
            ProviderError::ConfigError(payload.into()),
            ProviderError::ParseError(payload.into()),
            ProviderError::RateLimited {
                retry_after_secs: 17,
            },
        ];
        for error in cases {
            let safe = provider_failure("synthetic refresh", error);
            let output = format!("{safe:?} {safe}");
            assert!(
                !output.contains("SYNTHETIC"),
                "provider payload must not be rendered"
            );
            assert!(!output.contains("private@example.test"));
            assert!(!output.contains("https://"));
        }
    }

    #[tokio::test]
    async fn mismatched_identity_prevents_even_constructing_metadata_read() {
        for actual in ["other@example.test", "owner+alias@example.test", ""] {
            let called = std::cell::Cell::new(false);
            let result = verified_metadata("owner@example.test", actual, || async {
                called.set(true);
                Ok(())
            })
            .await;
            assert!(matches!(result, Err(ProbeError::IdentityMismatch)));
            assert!(!called.get());
        }
    }

    #[tokio::test]
    async fn verified_identity_permits_metadata_without_alias_normalization() {
        let value = verified_metadata("Owner@Example.Test", "owner@example.test", || async {
            Ok(42)
        })
        .await
        .unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn tenant_cannot_change_the_authority_path() {
        assert_eq!(tenant(None).unwrap(), "common");
        for allowed in [
            "common",
            "organizations",
            "consumers",
            "00000000-0000-0000-0000-000000000001",
        ] {
            assert_eq!(tenant(Some(allowed.into())).unwrap(), allowed);
        }
        for invalid in [
            "",
            "../other",
            "tenant?secret=SYNTHETIC",
            "https://attacker.test",
            "tenant/path",
        ] {
            assert!(matches!(
                tenant(Some(invalid.into())),
                Err(ProbeError::InvalidTenant)
            ));
        }
    }

    #[test]
    fn empty_access_tokens_fail_before_profile_reads() {
        for value in ["", " \t\n"] {
            assert!(matches!(
                require_access_token(value),
                Err(ProbeError::EmptyAccessToken)
            ));
        }
        assert!(require_access_token("SYNTHETIC-valid-token").is_ok());
    }

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn provider_tracing_is_suppressed_across_awaits() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let capture = bytes.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || Capture(capture.clone()))
            .finish();
        async {
            tracing::warn!("outer-visible-marker");
            quiet_operation(LIVE_TIMEOUT, async {
                tokio::task::yield_now().await;
                tracing::warn!("SYNTHETIC-provider-body-canary");
                Ok(())
            })
            .await
            .unwrap();
        }
        .with_subscriber(subscriber)
        .await;
        let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert!(output.contains("outer-visible-marker"));
        assert!(!output.contains("SYNTHETIC-provider-body-canary"));
    }

    #[tokio::test]
    async fn network_deadline_returns_only_sanitized_timeout() {
        let result = quiet_operation(
            Duration::ZERO,
            std::future::pending::<Result<(), ProbeError>>(),
        )
        .await;
        assert!(matches!(result, Err(ProbeError::Timeout)));
    }
    #[test]
    fn gmail_metadata_requires_inbox_without_disclosing_label_values() {
        for labels in [
            Vec::new(),
            vec![("Label_1".into(), "SYNTHETIC-private-label".into())],
        ] {
            let error = validate_gmail_metadata(&labels)
                .err()
                .expect("Gmail must include system INBOX metadata");
            assert!(!format!("{error:?} {error}").contains("SYNTHETIC"));
        }
        assert!(validate_gmail_metadata(&[("INBOX".into(), "Inbox".into())]).is_ok());
    }
}
