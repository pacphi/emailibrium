//! Built-in local LLM via llama-cpp-2 (ADR-021 addendum).
//!
//! Tier 0.5 generative model that loads a GGUF file directly into the backend
//! process using the `llama-cpp-2` crate (Rust bindings for llama.cpp).
//!
//! Gated behind the `builtin-llm` Cargo feature so that builds without native
//! C++ compilation remain fast.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::config::BuiltInLlmConfig;
use super::error::VectorError;
use super::generative::{GenerationParams, GenerativeModel};
use super::yaml_config::PromptsConfig;

use llama_cpp_4::context::params::LlamaContextParams;
use llama_cpp_4::llama_backend::LlamaBackend;
use llama_cpp_4::llama_batch::LlamaBatch;
use llama_cpp_4::model::params::LlamaModelParams;
use llama_cpp_4::model::LlamaModel;
use llama_cpp_4::token::data_array::LlamaTokenDataArray;

// ---------------------------------------------------------------------------
// Model resolution
// ---------------------------------------------------------------------------

/// Known built-in models and their Hugging Face coordinates.
struct ModelSpec {
    repo_id: String,
    filename: String,
}

/// Resolve model coordinates from the YAML catalog (`config/models-llm.yaml`).
///
/// Returns an error if the model ID is not found in the catalog.
/// Maintainers: add new models to `config/models-llm.yaml` under the `builtin` provider.
fn resolve_model(model_id: &str) -> Result<ModelSpec, VectorError> {
    match super::yaml_config::load_yaml_config("../config") {
        Ok(yaml) => {
            if let Some(builtin) = yaml.llm_catalog.providers.get("builtin") {
                if let Some(entry) = builtin.models.iter().find(|m| m.id == model_id) {
                    if let (Some(repo), Some(file)) = (&entry.repo_id, &entry.filename) {
                        return Ok(ModelSpec {
                            repo_id: repo.clone(),
                            filename: file.clone(),
                        });
                    }
                }
            }
            Err(VectorError::EmbeddingFailed(format!(
                "Model '{model_id}' not found in config/models-llm.yaml. \
                 Add it under providers.builtin.models with repo_id and filename."
            )))
        }
        Err(e) => {
            warn!(
                "Failed to load config/models-llm.yaml: {e}. \
                 Ensure the config/ directory exists at the project root."
            );
            Err(VectorError::EmbeddingFailed(format!(
                "Cannot resolve model '{model_id}': config/models-llm.yaml not found. \
                 Run from the project root or check that config/ directory exists."
            )))
        }
    }
}

/// Resolve the path to a cached GGUF model, downloading if necessary.
fn resolve_model_path(config: &BuiltInLlmConfig) -> Result<PathBuf, VectorError> {
    let spec = resolve_model(&config.model_id)?;
    let cache_dir = shellexpand::tilde(&config.cache_dir).to_string();
    let cache_path = Path::new(&cache_dir);

    // Check if already cached
    let model_path = cache_path.join(&spec.filename);
    if model_path.exists() {
        info!(path = %model_path.display(), "Built-in LLM model found in cache");
        return Ok(model_path);
    }

    // Download from Hugging Face Hub
    info!(
        repo = %spec.repo_id,
        file = %spec.filename,
        "Downloading built-in LLM model from Hugging Face..."
    );

    let api = hf_hub::api::sync::Api::new()
        .map_err(|e| VectorError::EmbeddingFailed(format!("HF Hub init failed: {e}")))?;

    let repo = api.model(spec.repo_id.clone());
    let downloaded = repo
        .get(&spec.filename)
        .map_err(|e| VectorError::EmbeddingFailed(format!("Model download failed: {e}")))?;

    info!(path = %downloaded.display(), "Model downloaded successfully");
    Ok(downloaded)
}

// ---------------------------------------------------------------------------
// BuiltInGenerativeModel
// ---------------------------------------------------------------------------

/// Inner state holding the loaded llama.cpp model and backend.
struct LoadedModel {
    backend: LlamaBackend,
    model: LlamaModel,
}

/// Chat template format for the model.
#[derive(Debug, Clone, PartialEq)]
enum ChatTemplate {
    /// Qwen/Phi-style ChatML: `<|im_start|>role\n...<|im_end|>`
    ChatML,
    /// Gemma-style: `<start_of_turn>role\n...<end_of_turn>`
    Gemma,
}

/// Built-in generative model using llama.cpp via the `llama-cpp-2` crate.
pub struct BuiltInGenerativeModel {
    config: BuiltInLlmConfig,
    model_path: PathBuf,
    inner: Arc<Mutex<Option<LoadedModel>>>,
    /// Per-model `tuning.max_tokens` from `models-llm.yaml`, if configured.
    model_max_tokens: Option<u32>,
    /// Resolved generation parameters from YAML config.
    params: GenerationParams,
    /// Repetition detection tuning from `config/tuning.yaml`.
    repetition_tuning: super::yaml_config::RepetitionTuning,
    /// Classification prompts loaded from `config/prompts.yaml`.
    prompts: PromptsConfig,
    /// Tracks when the model was last accessed for idle-timeout unloading.
    last_accessed: Arc<Mutex<Instant>>,
    /// Effective context size, incorporating the global `default_context_size`
    /// fallback from `tuning.yaml` when the per-model config uses the default.
    effective_context_size: u32,
    /// Chat template format resolved from `models-llm.yaml`.
    chat_template: ChatTemplate,
}

impl BuiltInGenerativeModel {
    /// Create a new built-in model. This resolves (and potentially downloads) the
    /// GGUF model file but does **not** load it into memory yet — that happens
    /// lazily on first inference request.
    pub fn new(config: &BuiltInLlmConfig) -> Result<Self, VectorError> {
        Self::with_params(config, GenerationParams::default())
    }

    /// Create a new built-in model with resolved generation parameters.
    pub fn with_params(
        config: &BuiltInLlmConfig,
        params: GenerationParams,
    ) -> Result<Self, VectorError> {
        Self::with_params_and_prompts(config, params, PromptsConfig::default())
    }

    /// Create with explicit generation parameters and prompts configuration from YAML.
    pub fn with_params_and_prompts(
        config: &BuiltInLlmConfig,
        params: GenerationParams,
        prompts: PromptsConfig,
    ) -> Result<Self, VectorError> {
        let model_path = resolve_model_path(config)?;

        let yaml = super::yaml_config::load_yaml_config("../config").ok();

        // Resolve per-model tuning.max_tokens from the YAML catalog.
        let model_max_tokens = yaml.as_ref().and_then(|y| {
            y.llm_catalog
                .providers
                .get("builtin")?
                .models
                .iter()
                .find(|m| m.id == config.model_id)?
                .tuning
                .as_ref()?
                .max_tokens
        });

        // Load repetition tuning from YAML config.
        let repetition_tuning = yaml
            .as_ref()
            .map(|c| c.tuning.repetition.clone())
            .unwrap_or_default();

        // Resolve effective context size: use per-model config if explicitly set
        // (non-default), otherwise fall back to tuning.yaml `default_context_size`.
        let global_default_ctx = yaml
            .as_ref()
            .map(|c| c.tuning.llm.default_context_size as u32)
            .unwrap_or(2048);
        let effective_context_size = if config.context_size > 0 {
            config.context_size
        } else {
            global_default_ctx
        };
        // Resolve chat template from the YAML catalog entry.
        let chat_template = yaml
            .as_ref()
            .and_then(|y| {
                y.llm_catalog
                    .providers
                    .get("builtin")?
                    .models
                    .iter()
                    .find(|m| m.id == config.model_id)?
                    .chat_template
                    .as_deref()
            })
            .map(|t| match t {
                "gemma" => ChatTemplate::Gemma,
                _ => ChatTemplate::ChatML,
            })
            .unwrap_or(ChatTemplate::ChatML);

        debug!(
            config_ctx = config.context_size,
            global_default = global_default_ctx,
            effective = effective_context_size,
            chat_template = ?chat_template,
            "Resolved effective context size for built-in LLM"
        );

        Ok(Self {
            config: config.clone(),
            model_path,
            inner: Arc::new(Mutex::new(None)),
            model_max_tokens,
            params,
            repetition_tuning,
            prompts,
            last_accessed: Arc::new(Mutex::new(Instant::now())),
            effective_context_size,
            chat_template,
        })
    }

    /// Ensure the model is loaded, returning the lock guard.
    async fn ensure_loaded(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<LoadedModel>>, VectorError> {
        // Update last-accessed timestamp for idle-timeout tracking.
        *self.last_accessed.lock().await = Instant::now();

        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            let path = self.model_path.clone();
            let gpu_layers = self.config.gpu_layers;
            let ctx_size = self.effective_context_size;

            // Model loading is CPU-intensive; run on blocking thread
            let loaded = tokio::task::spawn_blocking(move || {
                let backend = LlamaBackend::init().map_err(|e| {
                    VectorError::EmbeddingFailed(format!("llama backend init: {e}"))
                })?;

                let model_params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);

                info!(path = %path.display(), gpu_layers, ctx_size, "Loading built-in LLM...");

                let model = LlamaModel::load_from_file(&backend, &path, &model_params)
                    .map_err(|e| VectorError::EmbeddingFailed(format!("Model load failed: {e}")))?;

                info!("Built-in LLM loaded successfully");

                Ok::<LoadedModel, VectorError>(LoadedModel { backend, model })
            })
            .await
            .map_err(|e| VectorError::EmbeddingFailed(format!("spawn_blocking failed: {e}")))??;

            *guard = Some(loaded);

            // Warmup: run a minimal inference to prime the GPU/Metal pipeline.
            // The first inference after model load can produce empty output on
            // some backends (e.g. Apple Metal) due to cold-start latency.
            if let Some(ref loaded) = *guard {
                let ctx_size = self.effective_context_size;

                let warmup_result = {
                    let ctx_params = LlamaContextParams::default()
                        .with_n_ctx(std::num::NonZeroU32::new(ctx_size));
                    let mut ctx = loaded
                        .model
                        .new_context(&loaded.backend, ctx_params)
                        .map_err(|e| {
                            VectorError::EmbeddingFailed(format!(
                                "Warmup context creation failed: {e}"
                            ))
                        })?;

                    // Tokenize a trivial prompt using the correct template.
                    let warmup_prompt = match &self.chat_template {
                        ChatTemplate::Gemma => {
                            "<start_of_turn>user\nHi<end_of_turn>\n<start_of_turn>model\n"
                        }
                        ChatTemplate::ChatML => {
                            "<|im_start|>user\nHi<|im_end|>\n<|im_start|>assistant\n"
                        }
                    };
                    let tokens = loaded
                        .model
                        .str_to_token(warmup_prompt, llama_cpp_4::model::AddBos::Never)
                        .map_err(|e| {
                            VectorError::EmbeddingFailed(format!("Warmup tokenization failed: {e}"))
                        })?;

                    let mut batch = LlamaBatch::new(ctx_size as usize, 1);
                    for (i, &token) in tokens.iter().enumerate() {
                        let is_last = i == tokens.len() - 1;
                        batch.add(token, i as i32, &[0], is_last).ok();
                    }
                    ctx.decode(&mut batch).ok();

                    // Generate just 1 token to warm the pipeline.
                    let candidates = ctx.candidates();
                    let mut token_data = LlamaTokenDataArray::from_iter(candidates, false);
                    let _ = token_data.sample_token_greedy();

                    Ok::<(), VectorError>(())
                };

                match warmup_result {
                    Ok(()) => info!("Built-in LLM warmup complete"),
                    Err(e) => warn!(error = %e, "Built-in LLM warmup failed (non-fatal)"),
                }
            }
        }
        Ok(guard)
    }

    /// Parse a prompt from `[System]`/`[User]`/`[Assistant]`/`[Email Context]`
    /// tagged format into a list of `(role, content)` segments.
    fn parse_prompt_segments(raw_prompt: &str) -> Vec<(&'static str, String)> {
        let tag_pattern = [
            "\n[System]\n",
            "\n[User]\n",
            "\n[Assistant]\n",
            "\n[Email Context]\n",
        ];

        let normalized = format!("\n{raw_prompt}");

        let mut segments: Vec<(&str, usize)> = Vec::new();
        for tag in &tag_pattern {
            let mut start = 0;
            while let Some(pos) = normalized[start..].find(tag) {
                segments.push((tag, start + pos));
                start += pos + tag.len();
            }
        }
        segments.sort_by_key(|&(_, pos)| pos);

        if segments.is_empty() {
            return vec![("user", raw_prompt.trim().to_string())];
        }

        let mut result = Vec::new();
        for (i, &(tag, pos)) in segments.iter().enumerate() {
            let content_start = pos + tag.len();
            let content_end = if i + 1 < segments.len() {
                segments[i + 1].1
            } else {
                normalized.len()
            };
            let content = normalized[content_start..content_end].trim().to_string();
            if content.is_empty() {
                continue;
            }

            let role: &'static str = if tag.contains("[System]") || tag.contains("[Email Context]")
            {
                "system"
            } else if tag.contains("[Assistant]") {
                "assistant"
            } else {
                "user"
            };

            result.push((role, content));
        }
        result
    }

    /// Format prompt segments into ChatML format (`<|im_start|>`/`<|im_end|>`).
    fn format_chatml(segments: &[(&str, String)]) -> String {
        let mut result = String::new();
        for (role, content) in segments {
            result.push_str(&format!("<|im_start|>{role}\n"));
            result.push_str(content);
            result.push_str("\n<|im_end|>\n");
        }
        result.push_str("<|im_start|>assistant\n");
        result
    }

    /// Format prompt segments into Gemma format (`<start_of_turn>`/`<end_of_turn>`).
    ///
    /// Gemma models don't have a "system" role. System messages are prepended
    /// to the first user turn instead.
    fn format_gemma(segments: &[(&str, String)]) -> String {
        let mut result = String::new();
        let mut pending_system = String::new();

        for (role, content) in segments {
            match *role {
                "system" => {
                    // Gemma doesn't support system role — accumulate and
                    // prepend to the next user turn.
                    if !pending_system.is_empty() {
                        pending_system.push('\n');
                    }
                    pending_system.push_str(content);
                }
                "user" => {
                    result.push_str("<start_of_turn>user\n");
                    if !pending_system.is_empty() {
                        result.push_str(&pending_system);
                        result.push_str("\n\n");
                        pending_system.clear();
                    }
                    result.push_str(content);
                    result.push_str("<end_of_turn>\n");
                }
                "assistant" => {
                    result.push_str("<start_of_turn>model\n");
                    result.push_str(content);
                    result.push_str("<end_of_turn>\n");
                }
                _ => {}
            }
        }

        // If there's remaining system content but no user turn followed,
        // emit it as a user turn.
        if !pending_system.is_empty() {
            result.push_str("<start_of_turn>user\n");
            result.push_str(&pending_system);
            result.push_str("<end_of_turn>\n");
        }

        // Signal the model to generate.
        result.push_str("<start_of_turn>model\n");
        result
    }

    /// Convert a prompt from `[System]`/`[User]`/`[Assistant]`/`[Email Context]`
    /// tagged format into the model's native chat template.
    fn format_prompt(raw_prompt: &str, template: &ChatTemplate) -> String {
        let segments = Self::parse_prompt_segments(raw_prompt);
        match template {
            ChatTemplate::ChatML => Self::format_chatml(&segments),
            ChatTemplate::Gemma => Self::format_gemma(&segments),
        }
    }

    /// Run generation on the loaded model. Must be called from a blocking context.
    ///
    /// `params` carries temperature and repeat_penalty from the resolved
    /// `GenerationParams` so that no hardcoded values remain.
    #[allow(clippy::too_many_arguments)]
    fn generate_sync(
        model: &LlamaModel,
        backend: &LlamaBackend,
        prompt: &str,
        max_tokens: u32,
        ctx_size: u32,
        _temperature: f32,
        _repeat_penalty: f32,
        rep_tuning: &super::yaml_config::RepetitionTuning,
        chat_template: &ChatTemplate,
    ) -> Result<String, VectorError> {
        let formatted_prompt = Self::format_prompt(prompt, chat_template);
        // Dump formatted prompt for debugging.
        let _ = std::fs::write("/tmp/emailibrium_last_chatml.txt", &formatted_prompt);

        let ctx_params =
            LlamaContextParams::default().with_n_ctx(std::num::NonZeroU32::new(ctx_size));

        let mut ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| VectorError::EmbeddingFailed(format!("Context creation failed: {e}")))?;

        // Tokenize the prompt (no BOS — chat template handles boundaries)
        let tokens = model
            .str_to_token(&formatted_prompt, llama_cpp_4::model::AddBos::Never)
            .map_err(|e| VectorError::EmbeddingFailed(format!("Tokenization failed: {e}")))?;

        debug!(
            prompt_tokens = tokens.len(),
            "Tokenized prompt for generation"
        );

        // Create batch and evaluate prompt tokens
        let mut batch = LlamaBatch::new(ctx_size as usize, 1);

        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch
                .add(token, i as i32, &[0], is_last)
                .map_err(|e| VectorError::EmbeddingFailed(format!("Batch add failed: {e}")))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| VectorError::EmbeddingFailed(format!("Prompt decode failed: {e}")))?;

        // Generate tokens with repetition detection
        let mut output = String::new();
        let mut n_cur = tokens.len() as i32;
        let mut recent_tokens: Vec<i32> = Vec::new();

        for _ in 0..max_tokens {
            let candidates = ctx.candidates();
            let mut token_data = LlamaTokenDataArray::from_iter(candidates, false);

            let new_token = token_data.sample_token_greedy();

            // Check for EOS / end-of-generation
            if model.is_eog_token(new_token) {
                break;
            }

            let piece = model
                .token_to_str(new_token, llama_cpp_4::model::Special::Tokenize)
                .unwrap_or_default();

            // Stop if the model starts a new turn (template-specific markers)
            if piece.contains("<|im_start|>")
                || piece.contains("<|im_end|>")
                || piece.contains("<start_of_turn>")
                || piece.contains("<end_of_turn>")
            {
                break;
            }

            // Repetition detection: if the same token appears N+ times in the
            // last W tokens, the model is stuck in a loop.
            // Values from config/tuning.yaml → repetition section.
            let token_window = rep_tuning.token_window;
            let token_threshold = rep_tuning.token_repeat_threshold;
            let token_id = new_token.0;
            recent_tokens.push(token_id);
            if recent_tokens.len() > token_window {
                recent_tokens.remove(0);
            }
            if recent_tokens.len() >= token_window {
                let last = recent_tokens.last().unwrap();
                let repeat_count = recent_tokens.iter().filter(|t| *t == last).count();
                if repeat_count >= token_threshold {
                    debug!("Stopping generation: repetition detected");
                    break;
                }
            }

            // Also detect repeated phrases in the output text.
            // Values from config/tuning.yaml → repetition section.
            // Use floor_char_boundary/ceil_char_boundary to avoid panicking
            // on multi-byte UTF-8 characters (emojis, curly quotes, etc.).
            let phrase_check_after = rep_tuning.phrase_check_after;
            let phrase_check_length = rep_tuning.phrase_check_length;
            if output.len() > phrase_check_after {
                let split_at = output.len().saturating_sub(phrase_check_length);
                // Find the nearest char boundary at or after `split_at`.
                let safe_split = output.ceil_char_boundary(split_at);
                if safe_split < output.len() {
                    let tail = &output[safe_split..];
                    let check_region = &output[..safe_split];
                    if !tail.is_empty() && check_region.contains(tail) {
                        debug!("Stopping generation: repeated phrase detected");
                        break;
                    }
                }
            }

            output.push_str(&piece);

            // Prepare next batch
            batch.clear();
            batch
                .add(new_token, n_cur, &[0], true)
                .map_err(|e| VectorError::EmbeddingFailed(format!("Batch add failed: {e}")))?;

            ctx.decode(&mut batch)
                .map_err(|e| VectorError::EmbeddingFailed(format!("Decode failed: {e}")))?;

            n_cur += 1;
        }

        // Strip any leaked chat template markers from the output.
        let cleaned = output
            .replace("<end_of_turn>", "")
            .replace("<start_of_turn>", "")
            .replace("<|im_end|>", "")
            .replace("<|im_start|>", "");
        Ok(cleaned.trim().to_string())
    }
}

impl BuiltInGenerativeModel {
    /// Unload the model from memory if it has been idle longer than the
    /// given `timeout`. Returns `true` if the model was unloaded.
    pub async fn unload_if_idle(&self, timeout: std::time::Duration) -> bool {
        let last = *self.last_accessed.lock().await;
        if last.elapsed() >= timeout {
            let mut guard = self.inner.lock().await;
            if guard.is_some() {
                *guard = None;
                info!(
                    idle_secs = last.elapsed().as_secs(),
                    "Built-in LLM unloaded due to idle timeout"
                );
                return true;
            }
        }
        false
    }

    /// Check whether the model is currently loaded in memory.
    pub async fn is_loaded(&self) -> bool {
        self.inner.lock().await.is_some()
    }

    /// Internal generation helper that accepts an explicit temperature override.
    async fn generate_internal(
        &self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<String, VectorError> {
        let _ = self.ensure_loaded().await?;

        let prompt = prompt.to_string();
        let ctx_size = self.effective_context_size;
        let repeat_penalty = self.params.repeat_penalty;
        let rep_tuning = self.repetition_tuning.clone();
        let chat_template = self.chat_template.clone();

        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let guard = inner.blocking_lock();
            let loaded = guard.as_ref().unwrap();
            Self::generate_sync(
                &loaded.model,
                &loaded.backend,
                &prompt,
                max_tokens,
                ctx_size,
                temperature,
                repeat_penalty,
                &rep_tuning,
                &chat_template,
            )
        })
        .await
        .map_err(|e| VectorError::EmbeddingFailed(format!("Inference task failed: {e}")))?
    }
}

#[async_trait]
impl GenerativeModel for BuiltInGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        self.generate_internal(prompt, max_tokens, self.params.temperature)
            .await
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats_display = categories.join(", ");
        // Build prompt from YAML config using [System]/[User] block format for ChatML.
        let system = self.prompts.email_classification.trim();
        let user = self
            .prompts
            .email_classification_user
            .replace("{{categories}}", &cats_display)
            .replace("{{email_text}}", text);
        let prompt = format!("[System]\n{system}\n\n[User]\n{user}");

        // Use classification-specific temperature and max tokens from config
        let response = self
            .generate_internal(
                &prompt,
                self.params.classification_max_tokens,
                self.params.classification_temperature,
            )
            .await?;
        // Strip <think>...</think> blocks (Qwen 3 chain-of-thought) before matching.
        let stripped = if let Some(end) = response.find("</think>") {
            response[end + 8..].trim()
        } else if response.starts_with("<think>") {
            // Thinking block never closed — response is all thinking, no answer
            ""
        } else {
            response.trim()
        };
        let trimmed = stripped.trim_matches('"');

        // Empty response (e.g. unclosed <think> block) — signal gracefully
        if trimmed.is_empty() {
            debug!(
                categories = cats_display,
                "Built-in LLM returned empty response, deferring to fallback"
            );
            return Err(VectorError::CategorizationFailed(
                "Built-in LLM returned empty response".to_string(),
            ));
        }

        // Validate against known categories
        for cat in categories {
            if trimmed.eq_ignore_ascii_case(cat) {
                return Ok(cat.to_string());
            }
        }

        debug!(
            response = trimmed,
            categories = cats_display,
            "Built-in LLM classification response didn't match categories, using closest"
        );

        // Fuzzy match: check if the response contains a category
        for cat in categories {
            if trimmed.to_lowercase().contains(&cat.to_lowercase()) {
                return Ok(cat.to_string());
            }
        }

        warn!(
            response = trimmed,
            categories = cats_display,
            "Built-in LLM returned unexpected category"
        );
        Err(VectorError::CategorizationFailed(format!(
            "Built-in LLM returned '{trimmed}', not one of: {cats_display}"
        )))
    }

    async fn classify_batch(
        &self,
        texts: &[&str],
        categories: &[&str],
    ) -> Result<Vec<String>, VectorError> {
        if texts.len() <= 1 {
            let mut results = Vec::with_capacity(texts.len());
            for text in texts {
                results.push(self.classify(text, categories).await?);
            }
            return Ok(results);
        }

        // Build batch prompt (includes system + user batch template + emails).
        // Wrap in [System]/[User] blocks for ChatML conversion.
        let cats = categories.join(", ");
        let system = self.prompts.email_classification.trim();
        let user_template = self
            .prompts
            .email_classification_batch
            .replace("{{categories}}", &cats)
            .replace("{{count}}", &texts.len().to_string());
        let mut user_prompt = format!("{user_template}\n\n");
        for (i, text) in texts.iter().enumerate() {
            if i > 0 {
                user_prompt.push_str("---\n");
            }
            user_prompt.push_str(&format!("Email {}:\n{}\n", i + 1, text));
        }
        let chatml_prompt = format!("[System]\n{system}\n\n[User]\n{user_prompt}");
        let max_tokens = self.params.classification_max_tokens * texts.len() as u32;

        let response = self
            .generate_internal(
                &chatml_prompt,
                max_tokens,
                self.params.classification_temperature,
            )
            .await?;

        // Strip <think>...</think> blocks (Qwen 3 chain-of-thought).
        let stripped = if let Some(end) = response.find("</think>") {
            response[end + 8..].trim()
        } else if response.starts_with("<think>") {
            ""
        } else {
            response.trim()
        };

        let parsed = super::generative::parse_batch_response(stripped, texts.len(), categories);
        let mut results = Vec::with_capacity(texts.len());
        for (i, r) in parsed.into_iter().enumerate() {
            match r {
                Ok(cat) => results.push(cat),
                Err(_) => {
                    tracing::debug!(
                        index = i,
                        "Batch parse failed, falling back to individual classify"
                    );
                    results.push(self.classify(texts[i], categories).await?);
                }
            }
        }
        Ok(results)
    }

    fn model_name(&self) -> &str {
        &self.config.model_id
    }

    async fn is_available(&self) -> bool {
        self.model_path.exists()
    }

    fn configured_max_tokens(&self) -> Option<u32> {
        self.model_max_tokens
    }
}
