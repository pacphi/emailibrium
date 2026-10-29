//! Integration test: Outlook device-code sign-in end to end (milestone 6).
//!
//! An in-process mock of Microsoft's `/devicecode` and `/token` endpoints
//! (reached through `OutlookDeviceCodeConfig::authority`), the fake IMAP
//! server answering `AUTHENTICATE XOAUTH2`, and a real test database drive
//! `DeviceFlowRegistry` and `OAuthManager::load_imap_config`'s refresh.
//!
//! Not covered (needs a real tenant): Microsoft's consent screen, and signing
//! in at the verification page as a different mailbox — the fake IMAP server
//! accepts any credential, so the IMAP check can't reject a mismatch here.

#[path = "fake_imap/mod.rs"]
mod fake_imap;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Form, Json, Router};
use serde_json::{json, Value};

use emailibrium::email::device_flow::{DeviceFlowError, DeviceFlowRegistry, DeviceFlowStatus};
use emailibrium::email::imap::{ImapAuth, ProviderProfile};
use emailibrium::email::oauth::OAuthManager;
use emailibrium::vectors::config::OutlookDeviceCodeConfig;

use fake_imap::{FakeImapServer, FixtureMessage};

const EMAIL: &str = "chris@outlook.example";

/// Scripted Microsoft identity platform.
#[derive(Default)]
struct MockMs {
    interval: u64,
    /// Responses to device-code polls, consumed in order.
    poll_script: Mutex<Vec<(StatusCode, Value)>>,
    /// Lifetime of the access token the successful poll returns.
    expires_in: i64,
    token_hits: AtomicUsize,
    refresh_hits: AtomicUsize,
    refresh_had_secret: Mutex<Option<bool>>,
}

async fn devicecode(State(ms): State<Arc<MockMs>>) -> Json<Value> {
    Json(json!({
        "device_code": "secret-device-code",
        "user_code": "ABCD-EFGH",
        "verification_uri": "https://microsoft.com/devicelogin",
        "expires_in": 900,
        "interval": ms.interval,
        "message": "To sign in, use a web browser..."
    }))
}

async fn token(
    State(ms): State<Arc<MockMs>>,
    Form(form): Form<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    match form.get("grant_type").map(String::as_str) {
        Some("refresh_token") => {
            ms.refresh_hits.fetch_add(1, Ordering::SeqCst);
            *ms.refresh_had_secret.lock().unwrap() = Some(form.contains_key("client_secret"));
            (
                StatusCode::OK,
                Json(json!({
                    "token_type": "Bearer",
                    "access_token": "at-2",
                    "refresh_token": "rt-2",
                    "expires_in": 3600
                })),
            )
        }
        _ => {
            ms.token_hits.fetch_add(1, Ordering::SeqCst);
            let mut script = ms.poll_script.lock().unwrap();
            if script.is_empty() {
                (
                    StatusCode::OK,
                    Json(json!({
                        "token_type": "Bearer",
                        "access_token": "at-1",
                        "refresh_token": "rt-1",
                        "expires_in": ms.expires_in
                    })),
                )
            } else {
                let (status, body) = script.remove(0);
                (status, Json(body))
            }
        }
    }
}

async fn spawn_mock(ms: Arc<MockMs>) -> String {
    let app = Router::new()
        .route("/{tenant}/oauth2/v2.0/devicecode", post(devicecode))
        .route("/{tenant}/oauth2/v2.0/token", post(token))
        .with_state(ms);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn pending() -> (StatusCode, Value) {
    (
        StatusCode::BAD_REQUEST,
        json!({"error": "authorization_pending"}),
    )
}

/// A config pointed at the mock and the fake IMAP server, with a client-ID
/// env var unique to the calling test (tests share one process).
fn config(authority: String, imap_port: u16, env_name: &str) -> OutlookDeviceCodeConfig {
    std::env::set_var(env_name, "public-client-id");
    OutlookDeviceCodeConfig {
        client_id_env: env_name.to_string(),
        authority,
        imap_host: "127.0.0.1".to_string(),
        imap_port,
        imap_encryption: "none".to_string(),
        ..OutlookDeviceCodeConfig::default()
    }
}

async fn oauth_manager() -> OAuthManager {
    let db = emailibrium::db::test_sqlite_database().await;
    emailibrium::db::apply_sqlite_migrations(
        &db.sea_orm(),
        &[
            include_str!("../migrations/sqlite/001_initial_schema.sql"),
            include_str!("../migrations/sqlite/004_accounts.sql"),
            include_str!("../migrations/sqlite/013_account_settings.sql"),
            include_str!("../migrations/sqlite/015_sync_frequency_seconds.sql"),
            include_str!("../migrations/sqlite/028_imap_accounts.sql"),
            include_str!("../migrations/sqlite/030_account_role.sql"),
            include_str!("../migrations/sqlite/034_imap_auth_profile.sql"),
        ],
    )
    .await
    .expect("migrate");
    OAuthManager::new(db, Some("device-flow-test-master-password"))
}

async fn fake_imap() -> FakeImapServer {
    let inbox = FixtureMessage::new(
        1,
        "a@example.com",
        EMAIL,
        "Hello",
        "Mon, 26 Oct 2026 09:00:00 -0400",
        "Body.",
    );
    FakeImapServer::spawn(vec![inbox]).await
}

#[tokio::test]
async fn start_never_exposes_the_device_code() {
    let ms = Arc::new(MockMs::default());
    let imap = fake_imap().await;
    let cfg = config(
        spawn_mock(ms).await,
        imap.addr.port(),
        "EMAILIBRIUM_TEST_DC_START",
    );
    let registry = DeviceFlowRegistry::default();

    let started = registry
        .start(&oauth_manager().await, &cfg, EMAIL)
        .await
        .expect("start");

    let body = serde_json::to_value(&started).unwrap();
    assert_eq!(body["userCode"], "ABCD-EFGH");
    assert!(body["flowId"].as_str().is_some_and(|id| !id.is_empty()));
    let text = body.to_string();
    assert!(
        !text.contains("secret-device-code") && !text.to_lowercase().contains("devicecode"),
        "device code must not reach the browser: {text}"
    );
}

#[tokio::test]
async fn polling_is_throttled_and_slow_down_raises_the_interval() {
    let ms = Arc::new(MockMs {
        interval: 1,
        poll_script: Mutex::new(vec![
            pending(),
            (StatusCode::BAD_REQUEST, json!({"error": "slow_down"})),
        ]),
        ..MockMs::default()
    });
    let imap = fake_imap().await;
    let cfg = config(
        spawn_mock(ms.clone()).await,
        imap.addr.port(),
        "EMAILIBRIUM_TEST_DC_THROTTLE",
    );
    let oauth = oauth_manager().await;
    let registry = DeviceFlowRegistry::default();
    let flow = registry.start(&oauth, &cfg, EMAIL).await.unwrap().flow_id;

    // Before the interval: answered locally, Microsoft not contacted.
    assert_eq!(
        registry.poll(&oauth, &cfg, &flow).await.unwrap(),
        DeviceFlowStatus::Pending { interval: 1 }
    );
    assert_eq!(ms.token_hits.load(Ordering::SeqCst), 0);

    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        registry.poll(&oauth, &cfg, &flow).await.unwrap(),
        DeviceFlowStatus::Pending { interval: 1 }
    );
    assert_eq!(ms.token_hits.load(Ordering::SeqCst), 1);

    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        registry.poll(&oauth, &cfg, &flow).await.unwrap(),
        DeviceFlowStatus::SlowDown { interval: 6 }
    );
    assert_eq!(ms.token_hits.load(Ordering::SeqCst), 2);

    // The raised interval is enforced too.
    assert_eq!(
        registry.poll(&oauth, &cfg, &flow).await.unwrap(),
        DeviceFlowStatus::Pending { interval: 6 }
    );
    assert_eq!(ms.token_hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn success_verifies_over_imap_saves_the_account_and_ends_the_flow() {
    let ms = Arc::new(MockMs {
        interval: 0,
        expires_in: 3600,
        ..MockMs::default()
    });
    let imap = fake_imap().await;
    let cfg = config(
        spawn_mock(ms.clone()).await,
        imap.addr.port(),
        "EMAILIBRIUM_TEST_DC_SUCCESS",
    );
    let oauth = oauth_manager().await;
    let registry = DeviceFlowRegistry::default();
    let flow = registry.start(&oauth, &cfg, EMAIL).await.unwrap().flow_id;

    let status = registry.poll(&oauth, &cfg, &flow).await.unwrap();
    let DeviceFlowStatus::Connected { account_id, email } = status else {
        panic!("expected Connected, got {status:?}");
    };
    assert_eq!(email, EMAIL);

    let saved = oauth
        .load_imap_config(&account_id)
        .await
        .expect("saved account");
    assert_eq!(saved.profile, ProviderProfile::OutlookImap);
    assert_eq!(saved.username, EMAIL);
    assert!(matches!(saved.auth, ImapAuth::OAuthBearer(ref t) if t == "at-1"));
    assert_eq!(
        oauth
            .get_refresh_token(&account_id)
            .await
            .unwrap()
            .as_deref(),
        Some("rt-1")
    );

    // The flow is gone: the device code can't be redeemed twice.
    assert!(matches!(
        registry.poll(&oauth, &cfg, &flow).await,
        Err(DeviceFlowError::UnknownFlow)
    ));
}

#[tokio::test]
async fn declined_sign_in_ends_the_flow() {
    let ms = Arc::new(MockMs {
        interval: 0,
        poll_script: Mutex::new(vec![(
            StatusCode::BAD_REQUEST,
            json!({"error": "authorization_declined"}),
        )]),
        ..MockMs::default()
    });
    let imap = fake_imap().await;
    let cfg = config(
        spawn_mock(ms).await,
        imap.addr.port(),
        "EMAILIBRIUM_TEST_DC_DECLINED",
    );
    let oauth = oauth_manager().await;
    let registry = DeviceFlowRegistry::default();
    let flow = registry.start(&oauth, &cfg, EMAIL).await.unwrap().flow_id;

    assert_eq!(
        registry.poll(&oauth, &cfg, &flow).await.unwrap(),
        DeviceFlowStatus::Declined
    );
    assert!(matches!(
        registry.poll(&oauth, &cfg, &flow).await,
        Err(DeviceFlowError::UnknownFlow)
    ));
}

#[tokio::test]
async fn missing_client_id_env_var_is_reported() {
    let cfg = OutlookDeviceCodeConfig {
        client_id_env: "EMAILIBRIUM_TEST_DC_UNSET_CLIENT_ID".to_string(),
        ..OutlookDeviceCodeConfig::default()
    };
    let err = DeviceFlowRegistry::default()
        .start(&oauth_manager().await, &cfg, EMAIL)
        .await
        .expect_err("no client id");
    assert!(matches!(err, DeviceFlowError::NotConfigured(_)), "{err}");
}

#[tokio::test]
async fn expiring_access_token_is_refreshed_with_the_public_client_on_load() {
    let ms = Arc::new(MockMs {
        interval: 0,
        expires_in: 0, // already inside the refresh margin
        ..MockMs::default()
    });
    let imap = fake_imap().await;
    let cfg = config(
        spawn_mock(ms.clone()).await,
        imap.addr.port(),
        "EMAILIBRIUM_TEST_DC_REFRESH",
    );
    let oauth = oauth_manager().await.with_outlook_device_code(cfg.clone());
    let registry = DeviceFlowRegistry::default();
    let flow = registry.start(&oauth, &cfg, EMAIL).await.unwrap().flow_id;
    let DeviceFlowStatus::Connected { account_id, .. } =
        registry.poll(&oauth, &cfg, &flow).await.unwrap()
    else {
        panic!("expected Connected");
    };

    let loaded = oauth.load_imap_config(&account_id).await.expect("load");
    assert!(matches!(loaded.auth, ImapAuth::OAuthBearer(ref t) if t == "at-2"));
    assert_eq!(ms.refresh_hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        *ms.refresh_had_secret.lock().unwrap(),
        Some(false),
        "a public client must not send a client_secret"
    );
    assert_eq!(
        oauth
            .get_refresh_token(&account_id)
            .await
            .unwrap()
            .as_deref(),
        Some("rt-2"),
        "the rotated refresh token replaces the old one"
    );

    // Fresh token now: the next load doesn't refresh again.
    let again = oauth.load_imap_config(&account_id).await.expect("load");
    assert!(matches!(again.auth, ImapAuth::OAuthBearer(ref t) if t == "at-2"));
    assert_eq!(ms.refresh_hits.load(Ordering::SeqCst), 1);
}
