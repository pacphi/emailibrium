//! Authentication boundary for the local, single-user HTTP engine.
//!
//! `JWT_SECRET` is the operator-managed bearer credential delivered by the
//! existing secret plumbing. Browsers exchange it for an in-memory, eight-hour
//! HttpOnly cookie. Restart revokes every session; rotating the secret requires
//! restart. This is one local principal, not multi-tenant authorization.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderName, HeaderValue, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json, Router,
};
use base64::Engine;
use rand::Rng;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tower_http::cors::CorsLayer;

const SESSION_SECONDS: u64 = 8 * 60 * 60;
const MAX_SESSIONS: usize = 64;
const COOKIE_NAME: &str = "emailibrium_session";
type CallbackValidator = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct LocalAuth {
    bearer_digest: blake3::Hash,
    origins: Vec<HeaderValue>,
    sessions: Arc<Mutex<HashMap<blake3::Hash, Instant>>>,
    callback_validator: Option<CallbackValidator>,
}

impl LocalAuth {
    pub fn new(jwt_secret: &str, allowed_origins: &[String]) -> Result<Self, String> {
        if !(32..=512).contains(&jwt_secret.len())
            || !jwt_secret.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err("Local engine authentication requires JWT_SECRET with 32–512 printable non-space characters; use scripts/setup-secrets.sh or an explicit environment override".into());
        }
        let origins = allowed_origins
            .iter()
            .map(|origin| {
                if origin == "null"
                    || origin.contains('*')
                    || !(origin.starts_with("http://") || origin.starts_with("https://"))
                {
                    return Err(
                        "Security allowed_origins must contain exact HTTP(S) origins".to_string(),
                    );
                }
                HeaderValue::from_str(origin).map_err(|_| "Invalid allowed origin".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            bearer_digest: blake3::hash(jwt_secret.as_bytes()),
            origins,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            callback_validator: None,
        })
    }

    pub fn from_environment(secret_env: &str, allowed_origins: &[String]) -> Result<Self, String> {
        let name = if secret_env.is_empty() {
            "JWT_SECRET"
        } else {
            secret_env
        };
        let secret = zeroize::Zeroizing::new(std::env::var(name).map_err(|_| format!("Local engine authentication requires {name}; run scripts/setup-secrets.sh and supply the generated jwt_secret"))?);
        Self::new(secret.trim(), allowed_origins)
    }

    /// Only a state issued by an authenticated OAuth initiation can pass the
    /// callback exception. The callback handler must also consume it once.
    pub fn with_oauth_state_validator(mut self, validator: CallbackValidator) -> Self {
        self.callback_validator = Some(validator);
        self
    }

    fn valid_bearer(&self, request: &Request<Body>) -> bool {
        let Some(token) = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return false;
        };
        // blake3::Hash equality is constant-time; comparing raw strings is not.
        (32..=512).contains(&token.len()) && self.bearer_digest == blake3::hash(token.as_bytes())
    }

    fn session_digest(request: &Request<Body>) -> Option<blake3::Hash> {
        let cookie = request.headers().get(header::COOKIE)?.to_str().ok()?;
        let token = cookie
            .split(';')
            .find_map(|part| part.trim().strip_prefix(&format!("{COOKIE_NAME}=")))?;
        if token.len() != 43 {
            return None;
        }
        Some(blake3::hash(token.as_bytes()))
    }

    fn authenticated(&self, request: &Request<Body>) -> bool {
        // An explicitly supplied invalid credential must not silently fall back
        // to a previously authenticated browser cookie.
        if request.headers().contains_key(header::AUTHORIZATION) {
            return self.valid_bearer(request);
        }
        let Some(digest) = Self::session_digest(request) else {
            return false;
        };
        let now = Instant::now();
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        sessions.retain(|_, expires| *expires > now);
        sessions.contains_key(&digest)
    }

    fn start_session(&self) -> Response {
        let mut sessions = self.sessions.lock().expect("session lock poisoned");
        let now = Instant::now();
        sessions.retain(|_, expires| *expires > now);
        if sessions.len() >= MAX_SESSIONS {
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many active engine sessions; close a session or restart the engine",
            );
        }
        let mut random = [0u8; 32];
        rand::rng().fill_bytes(&mut random);
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random);
        sessions.insert(
            blake3::hash(token.as_bytes()),
            now + Duration::from_secs(SESSION_SECONDS),
        );
        let mut response = Json(json!({"authenticated": true})).into_response();
        // Local native HTTP is supported, so Secure is not set unconditionally.
        // SameSite + explicit Origin checks prevent browser cross-site writes.
        response.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&format!("{COOKIE_NAME}={token}; Path=/api; HttpOnly; SameSite=Strict; Max-Age={SESSION_SECONDS}")).expect("generated cookie is ASCII"));
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    let mut response = (status, Json(json!({"error": message}))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"emailibrium-local\""),
        );
    }
    response
}

async fn authorize(State(auth): State<LocalAuth>, request: Request<Body>, next: Next) -> Response {
    let origin = request.headers().get(header::ORIGIN).cloned();
    if origin
        .as_ref()
        .is_some_and(|origin| !auth.origins.contains(origin))
    {
        return error(
            StatusCode::FORBIDDEN,
            "This origin is not allowed to access the local engine",
        );
    }
    let mut response = authorize_request(auth, request, next).await;
    // Session and rejection responses originate before CorsLayer; trusted
    // browser clients must still be able to read their status and use cookies.
    if let Some(origin) = origin {
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        response
            .headers_mut()
            .append(header::VARY, HeaderValue::from_static("Origin"));
    }
    response
}

async fn authorize_request(auth: LocalAuth, request: Request<Body>, next: Next) -> Response {
    let origin = request.headers().get(header::ORIGIN);
    if request.method() == Method::OPTIONS
        && origin.is_some()
        && request
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
    {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if request.method() == Method::GET && path == "/healthz" {
        return Json(json!({"status": "ok"})).into_response();
    }
    if request.method() == Method::GET && path == "/api/v1/auth/callback" {
        let state = request
            .uri()
            .query()
            .and_then(|query| {
                query
                    .split('&')
                    .find_map(|part| part.strip_prefix("state="))
            })
            .and_then(|value| urlencoding::decode(value).ok());
        if state.as_deref().is_some_and(|state| {
            auth.callback_validator
                .as_ref()
                .is_some_and(|validate| validate(state))
        }) {
            return next.run(request).await;
        }
        return error(
            StatusCode::BAD_REQUEST,
            "OAuth request expired or invalid; restart the account connection",
        );
    }
    if request.method() == Method::POST && path == "/api/v1/session" {
        if !auth.valid_bearer(&request) {
            return error(
                StatusCode::UNAUTHORIZED,
                "The local access token is missing or invalid",
            );
        }
        return auth.start_session();
    }
    if !auth.authenticated(&request) {
        return error(
            StatusCode::UNAUTHORIZED,
            "Connect to the local engine with your access token",
        );
    }
    if path == "/api/v1/session" && request.method() == Method::GET {
        let mut response = Json(json!({"authenticated": true})).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    if path == "/api/v1/session" && request.method() == Method::DELETE {
        if let Some(digest) = LocalAuth::session_digest(&request) {
            auth.sessions
                .lock()
                .expect("session lock poisoned")
                .remove(&digest);
        }
        let mut response = Json(json!({"authenticated": false})).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static(
                "emailibrium_session=; Path=/api; HttpOnly; SameSite=Strict; Max-Age=0",
            ),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    next.run(request).await
}

/// Wrap the outer router after mounting both REST and the sibling MCP service.
pub fn protect_http_router(router: Router, auth: LocalAuth) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(auth.origins.clone())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
            HeaderName::from_static("mcp-session-id"),
            HeaderName::from_static("mcp-protocol-version"),
        ])
        .allow_credentials(true);
    router
        .layer(cors)
        .layer(axum::middleware::from_fn_with_state(auth, authorize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, routing::post};
    use reqwest::{Client, StatusCode};

    const SECRET: &str = "synthetic-local-secret-at-least-32-bytes";
    const ORIGIN: &str = "http://localhost:3000";

    async fn server() -> (String, tokio::task::JoinHandle<()>) {
        server_with_auth(LocalAuth::new(SECRET, &[ORIGIN.to_string()]).unwrap()).await
    }

    async fn server_with_auth(auth: LocalAuth) -> (String, tokio::task::JoinHandle<()>) {
        let rest = Router::new().route("/emails", get(|| async { "private mailbox" }));
        let mcp = Router::new().route("/", post(|| async { "private MCP result" }));
        let router = Router::new()
            .nest("/api/v1", rest)
            .nest_service("/api/v1/mcp", mcp);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, protect_http_router(router, auth))
                .await
                .unwrap();
        });
        (url, task)
    }

    #[tokio::test]
    async fn unauthenticated_rest_and_sibling_mcp_are_rejected_over_http() {
        let (url, task) = server().await;
        let client = Client::new();
        assert_eq!(
            client
                .get(format!("{url}/api/v1/emails"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .post(format!("{url}/api/v1/mcp/"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        task.abort();
    }

    #[tokio::test]
    async fn correct_bearer_reaches_rest_and_mcp_but_wrong_bearer_does_not() {
        let (url, task) = server().await;
        let client = Client::new();
        for (method, path) in [
            (reqwest::Method::GET, "/api/v1/emails"),
            (reqwest::Method::POST, "/api/v1/mcp/"),
        ] {
            assert_eq!(
                client
                    .request(method.clone(), format!("{url}{path}"))
                    .bearer_auth("wrong")
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                client
                    .request(method, format!("{url}{path}"))
                    .bearer_auth(SECRET)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
        task.abort();
    }

    #[tokio::test]
    async fn malicious_origin_is_rejected_even_with_a_correct_mcp_bearer() {
        let (url, task) = server().await;
        let client = Client::new();
        for origin in [
            "http://evil.example",
            "null",
            "http://localhost:3000.evil.example",
        ] {
            assert_eq!(
                client
                    .post(format!("{url}/api/v1/mcp/"))
                    .bearer_auth(SECRET)
                    .header("Origin", origin)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            client
                .post(format!("{url}/api/v1/mcp/"))
                .bearer_auth(SECRET)
                .header("Origin", ORIGIN)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        task.abort();
    }

    #[tokio::test]
    async fn browser_session_cookie_authenticates_streams_and_logout_revokes_it() {
        let (url, task) = server().await;
        let client = Client::new();
        let response = client
            .post(format!("{url}/api/v1/session"))
            .bearer_auth(SECRET)
            .header("Origin", ORIGIN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            ORIGIN
        );
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=28800"));
        assert!(!cookie.contains(SECRET));
        let cookie = cookie.split(';').next().unwrap();
        assert_eq!(
            client
                .get(format!("{url}/api/v1/emails"))
                .header("Cookie", cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .delete(format!("{url}/api/v1/session"))
                .header("Cookie", cookie)
                .header("Origin", ORIGIN)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .get(format!("{url}/api/v1/emails"))
                .header("Cookie", cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        task.abort();
    }

    #[tokio::test]
    async fn cors_preflight_is_public_only_for_explicitly_allowed_origins() {
        let (url, task) = server().await;
        let client = Client::new();
        let response = client
            .request(reqwest::Method::OPTIONS, format!("{url}/api/v1/mcp/"))
            .header("Origin", ORIGIN)
            .header("Access-Control-Request-Method", "POST")
            .header("Access-Control-Request-Headers", "authorization")
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            ORIGIN
        );
        assert_eq!(
            client
                .request(reqwest::Method::OPTIONS, format!("{url}/api/v1/mcp/"))
                .header("Origin", "http://evil.example")
                .header("Access-Control-Request-Method", "POST")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        task.abort();
    }

    #[tokio::test]
    async fn health_is_minimal_and_callback_is_not_a_general_auth_bypass() {
        let (url, task) = server().await;
        let client = Client::new();
        let response = client.get(format!("{url}/healthz")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "{\"status\":\"ok\"}");
        assert!(!client
            .get(format!(
                "{url}/api/v1/auth/callback?code=fake&state=gmail:fake"
            ))
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        assert_eq!(
            client
                .get(format!("{url}/api/v1/vectors/health"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        task.abort();
    }

    #[test]
    fn weak_missing_and_oversized_credentials_fail_closed() {
        for secret in ["", "short", &"x".repeat(1025)] {
            assert!(LocalAuth::new(secret, &[ORIGIN.to_string()]).is_err());
        }
    }
    #[tokio::test]
    async fn expired_and_restarted_sessions_cannot_authenticate() {
        let auth = LocalAuth::new(SECRET, &[ORIGIN.to_string()]).unwrap();
        let (url, task) = server_with_auth(auth.clone()).await;
        let client = Client::new();
        let response = client
            .post(format!("{url}/api/v1/session"))
            .bearer_auth(SECRET)
            .send()
            .await
            .unwrap();
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        for expires in auth.sessions.lock().unwrap().values_mut() {
            *expires = Instant::now() - Duration::from_secs(1);
        }
        assert_eq!(
            client
                .get(format!("{url}/api/v1/emails"))
                .header("Cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let (fresh_url, fresh_task) = server().await;
        assert_eq!(
            client
                .get(format!("{fresh_url}/api/v1/emails"))
                .header("Cookie", &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        task.abort();
        fresh_task.abort();
    }

    #[tokio::test]
    async fn session_issuance_is_bounded_and_cross_origin_login_is_rejected() {
        let auth = LocalAuth::new(SECRET, &[ORIGIN.to_string()]).unwrap();
        let (url, task) = server_with_auth(auth.clone()).await;
        let client = Client::new();
        assert_eq!(
            client
                .post(format!("{url}/api/v1/session"))
                .bearer_auth(SECRET)
                .header("Origin", "http://evil.example")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert!(auth.sessions.lock().unwrap().is_empty());
        for _ in 0..MAX_SESSIONS {
            assert_eq!(
                client
                    .post(format!("{url}/api/v1/session"))
                    .bearer_auth(SECRET)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            client
                .post(format!("{url}/api/v1/session"))
                .bearer_auth(SECRET)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(auth.sessions.lock().unwrap().len(), MAX_SESSIONS);
        task.abort();
    }
}
