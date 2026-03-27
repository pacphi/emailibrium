//! Model catalog for hardware-aware model selection (ADR-021).
//!
//! Sources all model metadata from `config/models-llm.yaml` — no hardcoded
//! model list. The YAML catalog is the single source of truth.

use serde::Serialize;

/// Metadata for a downloadable GGUF model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Config key (e.g., "qwen3-1.7b-q4km").
    pub id: String,
    /// Human-friendly display name.
    pub name: String,
    /// Parameter count (e.g., "1.7B", "8B").
    pub params: String,
    /// Approximate disk size of the GGUF file in MB.
    pub disk_mb: u32,
    /// Approximate RAM required when loaded in MB.
    pub ram_mb: u32,
    /// Context window size in tokens.
    pub context_size: u32,
    /// Quality tier: "fair", "good", "excellent".
    pub quality: String,
    /// Whether this model is recommended for the user's hardware.
    pub recommended: bool,
    /// Whether the model file is already cached locally.
    pub cached: bool,
    /// Hugging Face repo ID.
    pub repo_id: String,
    /// GGUF filename within the repo.
    pub filename: String,
}

/// System hardware info for model recommendations.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    /// Total physical RAM in MB.
    pub total_ram_mb: u64,
    /// Estimated available RAM in MB (total minus ~4GB OS overhead).
    pub available_for_model_mb: u64,
    /// GPU type detected (e.g., "Apple Metal", "CUDA", "CPU only").
    pub gpu_type: String,
}

/// Get available system RAM in bytes.
fn get_total_ram_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(8 * 1024 * 1024 * 1024) // default 8GB
    }
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| {
                        l.split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse::<u64>().ok())
                    })
            })
            .map(|kb| kb * 1024)
            .unwrap_or(8 * 1024 * 1024 * 1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        8 * 1024 * 1024 * 1024 // default 8GB
    }
}

/// Detect GPU type.
fn detect_gpu() -> String {
    #[cfg(target_os = "macos")]
    {
        "Apple Metal".to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // TODO: detect CUDA/Vulkan
        "CPU only".to_string()
    }
}

/// Get system hardware info.
pub fn get_system_info() -> SystemInfo {
    let total_bytes = get_total_ram_bytes();
    let total_mb = total_bytes / (1024 * 1024);
    // Reserve ~4GB for OS + app + embedding model
    let available = total_mb.saturating_sub(4096);

    SystemInfo {
        total_ram_mb: total_mb,
        available_for_model_mb: available,
        gpu_type: detect_gpu(),
    }
}

/// Check whether a model is cached, either in the HF cache or the app cache dir.
pub fn is_model_cached(repo_id: &str, filename: &str, cache_dir: &str) -> bool {
    let hf_cache = dirs::home_dir()
        .map(|h| h.join(".cache/huggingface/hub"))
        .unwrap_or_default();
    let cache_key = repo_id.replace('/', "--");
    hf_cache.join(format!("models--{cache_key}")).exists()
        || std::path::Path::new(cache_dir).join(filename).exists()
}

/// Full model catalog sourced from `config/models-llm.yaml` with
/// hardware-aware recommendations and cache status.
pub fn get_model_catalog(cache_dir: &str) -> Vec<ModelInfo> {
    let sys = get_system_info();
    let available = sys.available_for_model_mb;

    let yaml = match super::yaml_config::load_yaml_config("../config") {
        Ok(y) => y,
        Err(e) => {
            tracing::warn!("Failed to load config/models-llm.yaml: {e}");
            return Vec::new();
        }
    };

    let Some(builtin) = yaml.llm_catalog.providers.get("builtin") else {
        tracing::warn!("No 'builtin' provider in config/models-llm.yaml");
        return Vec::new();
    };

    builtin
        .models
        .iter()
        .filter_map(|entry| {
            let repo_id = entry.repo_id.as_deref().unwrap_or_default();
            let filename = entry.filename.as_deref().unwrap_or_default();
            if repo_id.is_empty() || filename.is_empty() {
                return None;
            }

            let disk_mb = entry.disk_mb.unwrap_or(0);
            let min_ram_mb = entry.min_ram_mb.unwrap_or(0);
            let fits = (min_ram_mb as u64) <= available;
            let cached = is_model_cached(repo_id, filename, cache_dir);

            Some(ModelInfo {
                id: entry.id.clone(),
                name: entry.name.clone(),
                params: entry.params.clone().unwrap_or_default(),
                disk_mb,
                ram_mb: min_ram_mb,
                context_size: entry.context_size,
                quality: entry.quality.clone().unwrap_or_else(|| "good".to_string()),
                recommended: fits,
                cached,
                repo_id: repo_id.to_string(),
                filename: filename.to_string(),
            })
        })
        .collect()
}

/// Find the best model for the available hardware.
/// Returns the largest model that fits, preferring quality.
#[allow(dead_code)]
pub fn recommend_model(cache_dir: &str) -> Option<ModelInfo> {
    let catalog = get_model_catalog(cache_dir);
    let recommended: Vec<_> = catalog.into_iter().filter(|m| m.recommended).collect();
    // Largest that fits (catalog preserves YAML order, which is sorted by size)
    recommended.into_iter().next_back()
}
