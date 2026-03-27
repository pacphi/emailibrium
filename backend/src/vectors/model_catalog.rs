//! Model catalog for hardware-aware model selection (ADR-021).
//!
//! Provides metadata about available GGUF models and recommends the best
//! model based on available system RAM.

use serde::Serialize;

/// Metadata for a downloadable GGUF model.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Config key used in `config.yaml` (e.g., "qwen2.5-0.5b-q4km").
    pub id: String,
    /// Human-friendly display name.
    pub name: String,
    /// Parameter count (e.g., "0.5B", "3B").
    pub params: String,
    /// Approximate disk size of the GGUF file in MB.
    pub disk_mb: u32,
    /// Approximate RAM required when loaded in MB.
    pub ram_mb: u32,
    /// Context window size in tokens.
    pub context_size: u32,
    /// Quality tier: "basic", "good", "excellent".
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

/// Full model catalog with hardware-aware recommendations.
pub fn get_model_catalog(cache_dir: &str) -> Vec<ModelInfo> {
    let sys = get_system_info();
    let available = sys.available_for_model_mb;

    let hf_cache = dirs::home_dir()
        .map(|h| h.join(".cache/huggingface/hub"))
        .unwrap_or_default();

    let models = vec![
        (
            "qwen3-1.7b-q4km",
            "Qwen 3 1.7B",
            "1.7B",
            1100,
            1500,
            32768,
            "fair",
            "unsloth/Qwen3-1.7B-GGUF",
            "Qwen3-1.7B-Q4_K_M.gguf",
        ),
        (
            "qwen2.5-1.5b-q4km",
            "Qwen 2.5 1.5B Instruct",
            "1.5B",
            1100,
            1200,
            8192,
            "good",
            "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
            "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        ),
        (
            "qwen2.5-3b-q4km",
            "Qwen 2.5 3B Instruct",
            "3B",
            2000,
            2500,
            8192,
            "good",
            "Qwen/Qwen2.5-3B-Instruct-GGUF",
            "qwen2.5-3b-instruct-q4_k_m.gguf",
        ),
        (
            "llama3.2-3b-q4km",
            "Llama 3.2 3B Instruct",
            "3B",
            2000,
            2500,
            8192,
            "good",
            "bartowski/Llama-3.2-3B-Instruct-GGUF",
            "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        ),
        (
            "qwen2.5-7b-q4km",
            "Qwen 2.5 7B Instruct",
            "7B",
            4700,
            5000,
            32768,
            "excellent",
            "Qwen/Qwen2.5-7B-Instruct-GGUF",
            "qwen2.5-7b-instruct-q4_k_m.gguf",
        ),
    ];

    models
        .into_iter()
        .map(
            |(id, name, params, disk_mb, ram_mb, ctx, quality, repo, file)| {
                let fits = (ram_mb as u64) <= available;
                // Check HF cache for this model
                let cache_key = repo.replace('/', "--");
                let cached = hf_cache.join(format!("models--{cache_key}")).exists()
                    || std::path::Path::new(cache_dir).join(file).exists();

                ModelInfo {
                    id: id.to_string(),
                    name: name.to_string(),
                    params: params.to_string(),
                    disk_mb,
                    ram_mb,
                    context_size: ctx,
                    quality: quality.to_string(),
                    recommended: fits,
                    cached,
                    repo_id: repo.to_string(),
                    filename: file.to_string(),
                }
            },
        )
        .collect()
}

/// Find the best model for the available hardware.
/// Returns the largest model that fits, preferring quality.
#[allow(dead_code)]
pub fn recommend_model(cache_dir: &str) -> ModelInfo {
    let catalog = get_model_catalog(cache_dir);
    let recommended: Vec<_> = catalog.into_iter().filter(|m| m.recommended).collect();
    recommended
        .into_iter()
        .next_back() // largest that fits (catalog is sorted by size)
        .unwrap_or_else(|| {
            // Fallback to smallest
            ModelInfo {
                id: "qwen3-1.7b-q4km".to_string(),
                name: "Qwen 3 1.7B".to_string(),
                params: "1.7B".to_string(),
                disk_mb: 1100,
                ram_mb: 1500,
                context_size: 32768,
                quality: "fair".to_string(),
                recommended: true,
                cached: false,
                repo_id: "unsloth/Qwen3-1.7B-GGUF".to_string(),
                filename: "Qwen3-1.7B-Q4_K_M.gguf".to_string(),
            }
        })
}
