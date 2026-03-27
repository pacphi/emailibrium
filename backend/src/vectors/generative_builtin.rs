//! Built-in local LLM via llama-cpp-2 (ADR-021 addendum).
//!
//! Tier 0.5 generative model that loads a GGUF file directly into the backend
//! process using the `llama-cpp-2` crate (Rust bindings for llama.cpp).
//!
//! Gated behind the `builtin-llm` Cargo feature so that builds without native
//! C++ compilation remain fast.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::config::BuiltInLlmConfig;
use super::error::VectorError;
use super::generative::GenerativeModel;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;

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

/// Built-in generative model using llama.cpp via the `llama-cpp-2` crate.
pub struct BuiltInGenerativeModel {
    config: BuiltInLlmConfig,
    model_path: PathBuf,
    inner: Arc<Mutex<Option<LoadedModel>>>,
}

impl BuiltInGenerativeModel {
    /// Create a new built-in model. This resolves (and potentially downloads) the
    /// GGUF model file but does **not** load it into memory yet — that happens
    /// lazily on first inference request.
    pub fn new(config: &BuiltInLlmConfig) -> Result<Self, VectorError> {
        let model_path = resolve_model_path(config)?;
        Ok(Self {
            config: config.clone(),
            model_path,
            inner: Arc::new(Mutex::new(None)),
        })
    }

    /// Ensure the model is loaded, returning the lock guard.
    async fn ensure_loaded(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<LoadedModel>>, VectorError> {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            let path = self.model_path.clone();
            let gpu_layers = self.config.gpu_layers;
            let ctx_size = self.config.context_size;

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
        }
        Ok(guard)
    }

    /// Convert a prompt from `[System]`/`[User]`/`[Assistant]`/`[Email Context]`
    /// tagged format into Qwen-style ChatML.
    ///
    /// Uses tag-based splitting (not `\n\n`) so that multi-line content like
    /// email bodies is kept intact within its ChatML block.
    fn to_chatml(raw_prompt: &str) -> String {
        let mut result = String::new();

        // Split on tag boundaries: lines starting with [System], [User], etc.
        let tag_pattern = [
            "\n[System]\n",
            "\n[User]\n",
            "\n[Assistant]\n",
            "\n[Email Context]\n",
        ];

        // Prepend \n so the first tag matches too.
        let normalized = format!("\n{raw_prompt}");

        // Find all tag positions.
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
            // No tags found — treat entire prompt as user message.
            result.push_str("<|im_start|>user\n");
            result.push_str(raw_prompt.trim());
            result.push_str("<|im_end|>\n");
        } else {
            for (i, &(tag, pos)) in segments.iter().enumerate() {
                let content_start = pos + tag.len();
                let content_end = if i + 1 < segments.len() {
                    segments[i + 1].1
                } else {
                    normalized.len()
                };
                let content = normalized[content_start..content_end].trim();
                if content.is_empty() {
                    continue;
                }

                let role = if tag.contains("[System]") || tag.contains("[Email Context]") {
                    "system"
                } else if tag.contains("[Assistant]") {
                    "assistant"
                } else {
                    "user"
                };

                result.push_str(&format!("<|im_start|>{role}\n"));
                result.push_str(content);
                result.push_str("\n<|im_end|>\n");
            }
        }

        // Signal the model to generate an assistant response.
        result.push_str("<|im_start|>assistant\n");
        result
    }

    /// Run generation on the loaded model. Must be called from a blocking context.
    fn generate_sync(
        model: &LlamaModel,
        backend: &LlamaBackend,
        prompt: &str,
        max_tokens: u32,
        ctx_size: u32,
    ) -> Result<String, VectorError> {
        let chatml_prompt = Self::to_chatml(prompt);
        // Dump ChatML for debugging.
        let _ = std::fs::write("/tmp/emailibrium_last_chatml.txt", &chatml_prompt);

        let ctx_params =
            LlamaContextParams::default().with_n_ctx(std::num::NonZeroU32::new(ctx_size));

        let mut ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| VectorError::EmbeddingFailed(format!("Context creation failed: {e}")))?;

        // Tokenize the prompt (no BOS — ChatML handles boundaries)
        let tokens = model
            .str_to_token(&chatml_prompt, llama_cpp_2::model::AddBos::Never)
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
        let mut decoder = encoding_rs::UTF_8.new_decoder();
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
                .token_to_piece(new_token, &mut decoder, true, None)
                .unwrap_or_default();

            // Stop if the model starts a new turn
            if piece.contains("<|im_start|>") || piece.contains("<|im_end|>") {
                break;
            }

            // Repetition detection: if the same token appears 4+ times in the
            // last 8 tokens, the model is stuck in a loop.
            let token_id = new_token.0;
            recent_tokens.push(token_id);
            if recent_tokens.len() > 8 {
                recent_tokens.remove(0);
            }
            if recent_tokens.len() >= 8 {
                let last = recent_tokens.last().unwrap();
                let repeat_count = recent_tokens.iter().filter(|t| *t == last).count();
                if repeat_count >= 4 {
                    debug!("Stopping generation: repetition detected");
                    break;
                }
            }

            // Also detect repeated phrases in the output text
            if output.len() > 200 {
                let tail = &output[output.len().saturating_sub(100)..];
                let check_region = &output[..output.len().saturating_sub(100)];
                if check_region.contains(tail) {
                    debug!("Stopping generation: repeated phrase detected");
                    break;
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

        Ok(output.trim().to_string())
    }
}

#[async_trait]
impl GenerativeModel for BuiltInGenerativeModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, VectorError> {
        // Ensure the model is loaded, then drop the guard before spawning
        // the blocking task (which re-acquires the lock).
        let _ = self.ensure_loaded().await?;

        let prompt = prompt.to_string();
        let ctx_size = self.config.context_size;

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
            )
        })
        .await
        .map_err(|e| VectorError::EmbeddingFailed(format!("Inference task failed: {e}")))?
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, VectorError> {
        let cats_display = categories.join(", ");
        // Use the [System]/[User] block format so to_chatml converts it properly.
        let prompt = format!(
            "[System]\nYou are an email classifier. Respond with ONLY the category name, nothing else.\n\n\
             [User]\nClassify this email into one of: {cats_display}\n\nEmail: {text}"
        );

        let response = self.generate(&prompt, 50).await?;
        let trimmed = response.trim().trim_matches('"');

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

    fn model_name(&self) -> &str {
        &self.config.model_id
    }

    async fn is_available(&self) -> bool {
        self.model_path.exists()
    }
}
