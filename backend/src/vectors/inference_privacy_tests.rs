//! HTTP-boundary privacy regressions. Every request uses synthetic text and loopback.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::{config::VectorConfig, VectorService};
use axum::{routing::post, Json, Router};

struct RecordingServer {
    url: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl RecordingServer {
    async fn start() -> Self {
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let other_requests = requests.clone();
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(move || {
                    let seen = seen.clone();
                    async move {
                        seen.fetch_add(1, Ordering::SeqCst);
                        Json(serde_json::json!({"choices":[{"message":{"content":"Work"}}]}))
                    }
                }),
            )
            .fallback(move || {
                let seen = other_requests.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({"data":[{"index":0,"embedding":vec![0.1;384]}]}))
                }
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            url,
            requests,
            task,
        }
    }
}

impl Drop for RecordingServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn cloud_config(server: &RecordingServer) -> VectorConfig {
    let mut config = VectorConfig::default();
    config.embedding.provider = "mock".into();
    config.store.backend = "memory".into();
    config.generative.provider = "cloud".into();
    config.generative.cloud.provider = "openai".into();
    config.generative.cloud.base_url = server.url.clone();
    config.generative.cloud.api_key_env = "EMAILIBRIUM_PRIVACY_TEST_ONLY_KEY".into();
    // Deliberately not a credential; a recording server is the only destination.
    std::env::set_var("EMAILIBRIUM_PRIVACY_TEST_ONLY_KEY", "synthetic-test-token");
    config
}

async fn database() -> Arc<crate::db::Database> {
    let db = crate::db::test_sqlite_database().await;
    db.run_migrations().await.unwrap();
    Arc::new(db)
}

#[tokio::test]
async fn privacy_default_blocks_configured_cloud_before_any_request() {
    let server = RecordingServer::start().await;
    let service = VectorService::new(cloud_config(&server), database().await, None, None)
        .await
        .unwrap();
    let result = service
        .generative
        .as_ref()
        .unwrap()
        .generate("synthetic email", 16)
        .await;
    assert!(
        result.is_err(),
        "private defaults must deny configured cloud inference"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn privacy_restart_honors_persisted_revocation() {
    let server = RecordingServer::start().await;
    let db = database().await;
    let consent = super::consent::ConsentManager::new(db.clone());
    consent
        .grant_consent("openai", "synthetic consent")
        .await
        .unwrap();
    consent.revoke_consent("openai").await.unwrap();
    super::privacy::PrivacyService::new(db.clone())
        .record_consent("cloud_ai", true, None, None)
        .await
        .unwrap();
    let mut config = cloud_config(&server);
    config.inference.allow_cloud = true;
    let service = VectorService::new(config, db, None, None).await.unwrap();
    let result = service
        .generative
        .as_ref()
        .unwrap()
        .classify("synthetic email", &["Work"])
        .await;
    assert!(
        result.is_err(),
        "startup must not restore a revoked cloud provider"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn privacy_raw_generator_clone_observes_revocation() {
    let server = RecordingServer::start().await;
    let mut config = cloud_config(&server);
    config.inference.allow_cloud = true;
    let service = VectorService::new(config, database().await, None, None)
        .await
        .unwrap();
    service
        .inference_policy
        .grant_consent("openai", "synthetic consent")
        .await
        .unwrap();
    let running_job_model = service.generative.clone().unwrap();
    service
        .inference_policy
        .record_cloud_consent(true)
        .await
        .unwrap();
    assert_eq!(
        running_job_model
            .generate("synthetic permitted email", 16)
            .await
            .unwrap(),
        "Work"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    service
        .inference_policy
        .revoke_consent("openai")
        .await
        .unwrap();
    let result = running_job_model
        .classify_batch(&["synthetic email", "second email"], &["Work"])
        .await;
    assert!(
        result.is_err(),
        "a pre-existing job clone must not bypass revocation"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn privacy_cloud_opt_in_needs_both_consents_and_global_revoke_affects_tools() {
    let server = RecordingServer::start().await;
    let mut config = cloud_config(&server);
    config.inference.allow_cloud = true;
    let service = VectorService::new(config.clone(), database().await, None, None)
        .await
        .unwrap();
    let tools = super::tool_calling_providers::create_tool_calling_provider(
        "cloud",
        &config,
        service.inference_policy.clone(),
    )
    .unwrap();
    let messages = [super::tool_calling::ToolMessage {
        role: super::tool_calling::ToolMessageRole::User,
        content: "synthetic tool request".into(),
        tool_calls: None,
        tool_call_id: None,
    }];
    assert!(tools
        .chat_with_tools(&messages, &[], 0.0, 16)
        .await
        .is_err());
    service
        .inference_policy
        .grant_consent("openai", "synthetic provider consent")
        .await
        .unwrap();
    assert!(tools
        .chat_with_tools(&messages, &[], 0.0, 16)
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    service
        .inference_policy
        .record_cloud_consent(true)
        .await
        .unwrap();
    assert!(tools.chat_with_tools(&messages, &[], 0.0, 16).await.is_ok());
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    service
        .inference_policy
        .record_cloud_consent(false)
        .await
        .unwrap();
    assert!(tools
        .chat_with_tools(&messages, &[], 0.0, 16)
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn privacy_router_cannot_fall_back_to_unconsented_cloud() {
    use super::generative_router::GenerativeRouterService;
    let server = RecordingServer::start().await;
    let mut config = cloud_config(&server);
    config.inference.allow_cloud = true;
    let service = VectorService::new(config, database().await, None, None)
        .await
        .unwrap();
    service
        .generative_router
        .register(
            super::model_registry::ProviderType::BuiltIn,
            Arc::new(FailingLocalModel),
            0,
        )
        .await;
    assert!(
        GenerativeRouterService::generate(&*service.generative_router, "synthetic email", 16)
            .await
            .is_err()
    );
    assert!(GenerativeRouterService::classify(
        &*service.generative_router,
        "synthetic email",
        &["Work"]
    )
    .await
    .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

struct FailingLocalModel;
#[async_trait::async_trait]
impl super::generative::GenerativeModel for FailingLocalModel {
    async fn generate(&self, _: &str, _: u32) -> Result<String, super::error::VectorError> {
        Err(super::error::VectorError::CategorizationFailed(
            "synthetic local failure".into(),
        ))
    }
    async fn classify(&self, _: &str, _: &[&str]) -> Result<String, super::error::VectorError> {
        Err(super::error::VectorError::CategorizationFailed(
            "synthetic local failure".into(),
        ))
    }
    fn model_name(&self) -> &str {
        "synthetic-local"
    }
    async fn is_available(&self) -> bool {
        true
    }
}

async fn ollama_server(
    status: serde_json::Value,
    remote: bool,
    redirect: Option<String>,
) -> RecordingServer {
    use axum::{
        response::{IntoResponse, Redirect},
        routing::get,
    };
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = requests.clone();
    let app = Router::new()
        .route("/api/status", get(move || { let value = status.clone(); async move { Json(value) } }))
        .route("/api/tags", get(move || async move {
            Json(serde_json::json!({"models":[{"name":"innocent-name:latest", "digest":"a".repeat(64), "size":1024,
                "remote_model":if remote { Some("cloud-model") } else { None },
                "remote_host":if remote { Some("https://ollama.com") } else { None }}]}))
        }))
        .route("/api/generate", post(move || {
            let seen = seen.clone(); let redirect = redirect.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                if let Some(target) = redirect { Redirect::temporary(&target).into_response() }
                else { Json(serde_json::json!({"response":"Work"})).into_response() }
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    RecordingServer {
        url,
        requests,
        task,
    }
}

fn ollama_config(server: &RecordingServer) -> VectorConfig {
    let mut config = cloud_config(server);
    config.generative.provider = "ollama".into();
    config.generative.ollama.base_url = server.url.clone();
    config.generative.ollama.chat_model = "innocent-name".into();
    config.generative.ollama.classification_model = "innocent-name".into();
    config
}

#[tokio::test]
async fn privacy_ollama_checks_daemon_and_artifact_metadata_not_tag_spelling() {
    for (status, remote, allowed) in [
        (serde_json::json!({"cloud":{"disabled":true}}), false, true),
        (
            serde_json::json!({"cloud":{"disabled":false}}),
            false,
            false,
        ),
        (serde_json::json!({"cloud":{}}), false, false),
        (serde_json::json!({"cloud":{"disabled":true}}), true, false),
    ] {
        let server = ollama_server(status, remote, None).await;
        let service = VectorService::new(ollama_config(&server), database().await, None, None)
            .await
            .unwrap();
        let result = service
            .generative
            .as_ref()
            .unwrap()
            .generate("synthetic private email", 16)
            .await;
        assert_eq!(result.is_ok(), allowed);
        assert_eq!(
            server.requests.load(Ordering::SeqCst),
            usize::from(allowed),
            "metadata checks must precede every prompt"
        );
    }
}

#[tokio::test]
async fn privacy_ollama_cannot_redirect_a_private_prompt() {
    let destination = RecordingServer::start().await;
    let server = ollama_server(
        serde_json::json!({"cloud":{"disabled":true}}),
        false,
        Some(format!("{}/v1/chat/completions", destination.url)),
    )
    .await;
    let service = VectorService::new(ollama_config(&server), database().await, None, None)
        .await
        .unwrap();
    assert!(service
        .generative
        .as_ref()
        .unwrap()
        .generate("synthetic private email", 16)
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        destination.requests.load(Ordering::SeqCst),
        0,
        "redirect target must receive no prompt"
    );
}

#[tokio::test]
async fn privacy_rejects_remote_ollama_endpoint_before_network() {
    let server = RecordingServer::start().await;
    let mut config = ollama_config(&server);
    config.generative.ollama.base_url = "http://localhost.example.invalid:11434".into();
    let service = VectorService::new(config, database().await, None, None)
        .await
        .unwrap();
    assert!(service
        .generative
        .as_ref()
        .unwrap()
        .generate("synthetic private email", 16)
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn privacy_router_can_classify_when_only_the_classification_model_is_ready() {
    use super::generative_router::GenerativeRouterService;
    let server = ollama_server(serde_json::json!({"cloud":{"disabled":true}}), false, None).await;
    let mut config = ollama_config(&server);
    config.generative.ollama.chat_model = "not-installed-chat".into();
    let service = VectorService::new(config, database().await, None, None)
        .await
        .unwrap();
    assert_eq!(
        GenerativeRouterService::classify(
            &*service.generative_router,
            "synthetic email",
            &["Work"]
        )
        .await
        .unwrap(),
        "Work"
    );
    assert!(
        GenerativeRouterService::generate(&*service.generative_router, "synthetic chat", 16)
            .await
            .is_err()
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn privacy_mode_cannot_be_overridden_by_persisted_consent() {
    let server = RecordingServer::start().await;
    let service = VectorService::new(cloud_config(&server), database().await, None, None)
        .await
        .unwrap();
    service
        .inference_policy
        .grant_consent("openai", "synthetic consent")
        .await
        .unwrap();
    service
        .inference_policy
        .record_cloud_consent(true)
        .await
        .unwrap();
    assert!(service
        .generative
        .as_ref()
        .unwrap()
        .generate("synthetic private email", 16)
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn privacy_revocation_drains_in_flight_calls_and_blocks_later_calls() {
    use tokio::sync::Notify;
    let entered = Arc::new(Notify::new());
    let finish = Arc::new(Notify::new());
    let requests = Arc::new(AtomicUsize::new(0));
    let request_entered = entered.clone();
    let request_finish = finish.clone();
    let request_count = requests.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let entered = request_entered.clone();
            let finish = request_finish.clone();
            let count = request_count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                entered.notify_one();
                finish.notified().await;
                Json(serde_json::json!({"choices":[{"message":{"content":"Work"}}]}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = RecordingServer {
        url: format!("http://{}", listener.local_addr().unwrap()),
        requests,
        task: tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    };
    let mut config = cloud_config(&server);
    config.inference.allow_cloud = true;
    let service = VectorService::new(config, database().await, None, None)
        .await
        .unwrap();
    service
        .inference_policy
        .grant_consent("openai", "synthetic consent")
        .await
        .unwrap();
    service
        .inference_policy
        .record_cloud_consent(true)
        .await
        .unwrap();
    let model = service.generative.clone().unwrap();
    let call = tokio::spawn(async move { model.generate("synthetic admitted email", 16).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let policy = service.inference_policy.clone();
    let mut revoke = tokio::spawn(async move { policy.revoke_consent("openai").await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), &mut revoke)
            .await
            .is_err(),
        "revocation cannot report completion while an admitted call remains in flight"
    );
    finish.notify_one();
    call.await.unwrap().unwrap();
    revoke.await.unwrap().unwrap();
    assert!(service
        .generative
        .as_ref()
        .unwrap()
        .generate("synthetic later email", 16)
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn privacy_cloud_embedding_clone_uses_the_same_policy() {
    let server = RecordingServer::start().await;
    let db = database().await;
    let consent = Arc::new(super::consent::ConsentManager::new(db.clone()));
    let privacy = Arc::new(super::privacy::PrivacyService::new(db));
    let policy = Arc::new(super::inference_policy::InferencePolicy::new(
        true, consent, privacy,
    ));
    let mut config = cloud_config(&server).embedding;
    config.provider = "cloud".into();
    config.cloud.api_key_env = "EMAILIBRIUM_PRIVACY_TEST_ONLY_KEY".into();
    config.cloud.base_url = server.url.clone();
    let pipeline = super::embedding::EmbeddingPipeline::new(&config)
        .unwrap()
        .with_inference_policy(policy.clone(), &config);
    assert!(pipeline.embed("synthetic private content").await.is_err());
    assert!(pipeline
        .embed_batch(&["synthetic private content".into()])
        .await
        .is_err());
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}
