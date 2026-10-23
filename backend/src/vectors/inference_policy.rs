//! Request-time inference authorization shared by chat, classification and embeddings.
//!
//! No cached consent decisions: each cloud call checks the persisted records.
//! A read permit remains held until inference completes; API revocation takes the
//! corresponding write permit before persisting, so after revocation returns no
//! already-admitted cloud call remains in flight in this application instance.
use std::{net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::{OwnedRwLockReadGuard, RwLock};

use super::{
    consent::ConsentManager,
    embedding::EmbeddingModel,
    error::VectorError,
    generative::GenerativeModel,
    privacy::{ConsentDecision, PrivacyService},
    tool_calling::{
        ChatCompletion, ToolCallingMode, ToolCallingProvider, ToolDefinition, ToolMessage,
    },
};

#[derive(Clone, Debug)]
pub enum InferenceTarget {
    Unverified,
    InProcess,
    Ollama { endpoint: String, model: String },
    Cloud { provider: String, endpoint: String },
}

pub struct InferencePolicy {
    allow_cloud: bool,
    consent: Arc<ConsentManager>,
    privacy: Arc<PrivacyService>,
    gate: Arc<RwLock<()>>,
    metadata_client: reqwest::Client,
}

impl InferencePolicy {
    pub fn new(
        allow_cloud: bool,
        consent: Arc<ConsentManager>,
        privacy: Arc<PrivacyService>,
    ) -> Self {
        Self {
            allow_cloud,
            consent,
            privacy,
            gate: Arc::new(RwLock::new(())),
            metadata_client: http_client(Duration::from_secs(5)),
        }
    }

    pub async fn grant_consent(
        &self,
        provider: &str,
        acknowledgment: &str,
    ) -> Result<(), VectorError> {
        let _guard = self.gate.write().await;
        self.consent.grant_consent(provider, acknowledgment).await
    }

    pub async fn revoke_consent(&self, provider: &str) -> Result<(), VectorError> {
        let _guard = self.gate.write().await;
        self.consent.revoke_consent(provider).await
    }

    pub async fn record_cloud_consent(
        &self,
        granted: bool,
    ) -> Result<ConsentDecision, VectorError> {
        let _guard = self.gate.write().await;
        self.privacy
            .record_consent("cloud_ai", granted, None, None)
            .await
    }

    async fn authorize(
        &self,
        target: &InferenceTarget,
    ) -> Result<Option<OwnedRwLockReadGuard<()>>, VectorError> {
        match target {
            InferenceTarget::Unverified => {
                Err(denied("inference backend has no locality classification"))
            }
            InferenceTarget::InProcess => Ok(None),
            InferenceTarget::Ollama { endpoint, model } => {
                self.verify_local_ollama(endpoint, model).await?;
                Ok(None)
            }
            InferenceTarget::Cloud { provider, endpoint } => {
                if !self.allow_cloud {
                    return Err(denied("cloud inference is disabled by private mode"));
                }
                validate_cloud_endpoint(provider, endpoint)?;
                let permit = self.gate.clone().read_owned().await;
                let global = self.privacy.get_consent("cloud_ai").await?;
                if !global.is_some_and(|c| c.granted && c.revoked_at.is_none())
                    || !self.consent.has_consent(provider).await?
                {
                    return Err(denied("active cloud_ai and provider consent are required"));
                }
                Ok(Some(permit))
            }
        }
    }

    async fn verify_local_ollama(&self, endpoint: &str, model: &str) -> Result<(), VectorError> {
        let base = local_endpoint(endpoint)?;
        // Ollama's official API exposes cloud.disabled at GET /api/status.
        // A missing/old/unknown response fails closed; model tag spelling is not evidence.
        // https://github.com/ollama/ollama/blob/main/api/types.go (StatusResponse/ListModelResponse)
        #[derive(Deserialize)]
        struct CloudStatus {
            disabled: bool,
        }
        #[derive(Deserialize)]
        struct Status {
            cloud: CloudStatus,
        }
        let response = self
            .metadata_client
            .get(format!("{base}/api/status"))
            .send()
            .await
            .map_err(|_| denied("cannot verify Ollama local-only status"))?;
        if !response.status().is_success() {
            return Err(denied("Ollama local-only status is unavailable"));
        }
        let status: Status = response
            .json()
            .await
            .map_err(|_| denied("invalid Ollama local-only status"))?;
        if !status.cloud.disabled {
            return Err(denied(
                "disable Ollama cloud features before using local inference",
            ));
        }

        #[derive(Deserialize)]
        struct Model {
            name: String,
            digest: String,
            size: u64,
            #[serde(default)]
            remote_model: Option<String>,
            #[serde(default)]
            remote_host: Option<String>,
        }
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<Model>,
        }
        let response = self
            .metadata_client
            .get(format!("{base}/api/tags"))
            .send()
            .await
            .map_err(|_| denied("cannot verify Ollama model locality"))?;
        if !response.status().is_success() {
            return Err(denied("Ollama model metadata unavailable"));
        }
        let tags: Tags = response
            .json()
            .await
            .map_err(|_| denied("invalid Ollama model metadata"))?;
        let canonical = if model.rsplit('/').next().unwrap_or(model).contains(':') {
            model.to_owned()
        } else {
            format!("{model}:latest")
        };
        let local = tags.models.iter().any(|m| {
            (m.name == *model || m.name == canonical)
                && m.size > 0
                && m.digest.len() == 64
                && m.digest.bytes().all(|b| b.is_ascii_hexdigit())
                && m.remote_model.as_deref().is_none_or(str::is_empty)
                && m.remote_host.as_deref().is_none_or(str::is_empty)
        });
        if !local {
            return Err(denied(
                "selected Ollama model is not a verified local artifact",
            ));
        }
        Ok(())
    }

    pub fn wrap_generative(
        self: &Arc<Self>,
        inner: Arc<dyn GenerativeModel>,
        target: InferenceTarget,
        classification_target: Option<InferenceTarget>,
    ) -> Arc<dyn GenerativeModel> {
        Arc::new(GuardedGenerative {
            inner,
            policy: self.clone(),
            classification_target: classification_target.unwrap_or_else(|| target.clone()),
            target,
        })
    }

    pub fn wrap_embedding(
        self: &Arc<Self>,
        inner: Arc<dyn EmbeddingModel>,
        target: InferenceTarget,
    ) -> Arc<dyn EmbeddingModel> {
        Arc::new(GuardedEmbedding {
            inner,
            policy: self.clone(),
            target,
        })
    }

    pub fn wrap_tools(
        self: &Arc<Self>,
        inner: Arc<dyn ToolCallingProvider>,
        target: InferenceTarget,
    ) -> Arc<dyn ToolCallingProvider> {
        Arc::new(GuardedTools {
            inner,
            policy: self.clone(),
            target,
        })
    }
}

fn denied(reason: &str) -> VectorError {
    VectorError::ConfigError(format!("Inference privacy policy denied request: {reason}"))
}

/// Inference transport never follows redirects or inherits an outbound proxy.
pub(crate) fn http_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .expect("static inference HTTP client configuration")
}

/// Canonicalize localhost to a literal loopback address so DNS cannot change its destination.
pub(crate) fn canonical_endpoint(endpoint: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(endpoint) else {
        return endpoint.to_owned();
    };
    if url.host_str() == Some("localhost") {
        let _ = url.set_host(Some("127.0.0.1"));
    }
    url.as_str().trim_end_matches('/').to_owned()
}

fn parsed_endpoint(endpoint: &str) -> Result<reqwest::Url, VectorError> {
    let url = reqwest::Url::parse(&canonical_endpoint(endpoint))
        .map_err(|_| denied("invalid inference endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(denied("unsupported inference endpoint"));
    }
    Ok(url)
}

fn is_loopback(url: &reqwest::Url) -> bool {
    url.host_str()
        .and_then(|h| h.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
}

fn local_endpoint(endpoint: &str) -> Result<String, VectorError> {
    let url = parsed_endpoint(endpoint)?;
    if !is_loopback(&url) || url.path() != "/" {
        return Err(denied("Ollama must use a literal loopback endpoint"));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn validate_cloud_endpoint(provider: &str, endpoint: &str) -> Result<(), VectorError> {
    let url = parsed_endpoint(endpoint)?;
    // Loopback-compatible servers can be tested/used, but still require cloud consent.
    if is_loopback(&url) {
        return Ok(());
    }
    let host = match provider {
        "openai" => "api.openai.com",
        "anthropic" => "api.anthropic.com",
        "gemini" => "generativelanguage.googleapis.com",
        "openrouter" => "openrouter.ai",
        "cohere" => "api.cohere.com",
        _ => return Err(denied("unsupported cloud provider")),
    };
    if url.scheme() != "https"
        || url.host_str() != Some(host)
        || url.port_or_known_default() != Some(443)
    {
        return Err(denied(
            "cloud endpoint does not match the consented provider",
        ));
    }
    Ok(())
}

struct GuardedGenerative {
    inner: Arc<dyn GenerativeModel>,
    policy: Arc<InferencePolicy>,
    target: InferenceTarget,
    classification_target: InferenceTarget,
}

#[async_trait]
impl GenerativeModel for GuardedGenerative {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        let _permit = self.policy.authorize(&self.target).await?;
        self.inner.generate(prompt, max_tokens).await
    }
    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let _permit = self.policy.authorize(&self.classification_target).await?;
        self.inner.classify(text, categories).await
    }
    async fn classify_batch(
        &self,
        texts: &[&str],
        categories: &[&str],
    ) -> Result<Vec<String>, VectorError> {
        let _permit = self.policy.authorize(&self.classification_target).await?;
        self.inner.classify_batch(texts, categories).await
    }
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }
    async fn is_available(&self) -> bool {
        let Ok(_permit) = self.policy.authorize(&self.target).await else {
            return false;
        };
        self.inner.is_available().await
    }
    async fn is_available_for_classification(&self) -> bool {
        let Ok(_permit) = self.policy.authorize(&self.classification_target).await else {
            return false;
        };
        self.inner.is_available_for_classification().await
    }
    fn configured_max_tokens(&self) -> Option<u32> {
        self.inner.configured_max_tokens()
    }
}

struct GuardedEmbedding {
    inner: Arc<dyn EmbeddingModel>,
    policy: Arc<InferencePolicy>,
    target: InferenceTarget,
}
#[async_trait]
impl EmbeddingModel for GuardedEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, VectorError> {
        let _permit = self.policy.authorize(&self.target).await?;
        self.inner.embed(text).await
    }
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, VectorError> {
        let _permit = self.policy.authorize(&self.target).await?;
        self.inner.embed_batch(texts).await
    }
    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }
    async fn is_available(&self) -> bool {
        let Ok(_permit) = self.policy.authorize(&self.target).await else {
            return false;
        };
        self.inner.is_available().await
    }
}

struct GuardedTools {
    inner: Arc<dyn ToolCallingProvider>,
    policy: Arc<InferencePolicy>,
    target: InferenceTarget,
}
#[async_trait]
impl ToolCallingProvider for GuardedTools {
    async fn chat_with_tools(
        &self,
        messages: &[ToolMessage],
        tools: &[ToolDefinition],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<ChatCompletion, Box<dyn std::error::Error + Send + Sync>> {
        let _permit = self.policy.authorize(&self.target).await?;
        self.inner
            .chat_with_tools(messages, tools, temperature, max_tokens)
            .await
    }
    fn tool_calling_mode(&self) -> ToolCallingMode {
        self.inner.tool_calling_mode()
    }
}
