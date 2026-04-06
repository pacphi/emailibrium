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
    /// Model family (e.g., "qwen3", "llama3"), useful for UI grouping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Chat template format (e.g., "chatml", "llama3").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template: Option<String>,
    /// Whether this model supports retrieval-augmented generation.
    pub rag_capable: bool,
    /// Whether this model supports native tool/function calling.
    pub tool_calling: bool,
    /// RAM threshold (MB) at or above which this model is the default choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_for_ram_mb: Option<u32>,
    /// Human-readable notes about the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Cost per 1 million input tokens (USD), for cloud/API models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_1m_input: Option<f64>,
    /// Cost per 1 million output tokens (USD), for cloud/API models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_per_1m_output: Option<f64>,
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

/// Get total system RAM in bytes.
pub fn get_total_ram_bytes() -> u64 {
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
        // Detect GPU acceleration: try nvidia-smi for CUDA, then vulkaninfo for Vulkan.
        if std::process::Command::new("nvidia-smi")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            "NVIDIA CUDA".to_string()
        } else if std::process::Command::new("vulkaninfo")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            "Vulkan".to_string()
        } else {
            "CPU only".to_string()
        }
    }
}

/// Default OS overhead in MB (used when no config is provided).
const DEFAULT_OS_OVERHEAD_MB: u64 = 4096;

/// Get system hardware info.
///
/// The `os_overhead_mb` parameter comes from `app.yaml → hardware.os_overhead_mb`
/// and specifies how much RAM to reserve for the OS and application before
/// recommending models. Falls back to [`DEFAULT_OS_OVERHEAD_MB`] (4 GB).
pub fn get_system_info() -> SystemInfo {
    get_system_info_with_overhead(DEFAULT_OS_OVERHEAD_MB)
}

/// Get system hardware info with a configurable OS overhead.
pub fn get_system_info_with_overhead(os_overhead_mb: u64) -> SystemInfo {
    let total_bytes = get_total_ram_bytes();
    let total_mb = total_bytes / (1024 * 1024);
    let overhead = if os_overhead_mb > 0 {
        os_overhead_mb
    } else {
        DEFAULT_OS_OVERHEAD_MB
    };
    let available = total_mb.saturating_sub(overhead);

    SystemInfo {
        total_ram_mb: total_mb,
        available_for_model_mb: available,
        gpu_type: detect_gpu(),
    }
}

/// Check whether a model is cached, either in the HF cache or the app cache dir.
///
/// For HF cache, we check that the snapshots directory contains the actual GGUF
/// file (not just that the model directory exists — HF creates the directory at
/// the start of download before the file is complete).
pub fn is_model_cached(repo_id: &str, filename: &str, cache_dir: &str) -> bool {
    // Check app's own cache dir first (simple path check).
    if std::path::Path::new(cache_dir).join(filename).exists() {
        return true;
    }

    // Check HF cache: look for the actual file inside snapshots/<hash>/.
    let hf_cache = dirs::home_dir()
        .map(|h| h.join(".cache/huggingface/hub"))
        .unwrap_or_default();
    let cache_key = repo_id.replace('/', "--");
    let model_dir = hf_cache.join(format!("models--{cache_key}"));
    let snapshots_dir = model_dir.join("snapshots");

    if let Ok(entries) = std::fs::read_dir(&snapshots_dir) {
        for entry in entries.flatten() {
            let gguf_path = entry.path().join(filename);
            if gguf_path.exists() {
                return true;
            }
        }
    }

    false
}

/// Full model catalog sourced from `config/models-llm.yaml` with
/// hardware-aware recommendations and cache status.
///
/// Applies `memory_safety_margin` from `tuning.yaml` when checking whether
/// a model fits into available RAM, and logs a warning when system memory
/// usage exceeds `memory_warning_threshold`.
pub fn get_model_catalog(cache_dir: &str) -> Vec<ModelInfo> {
    let yaml = match super::yaml_config::load_yaml_config("../config") {
        Ok(y) => y,
        Err(e) => {
            tracing::warn!("Failed to load config/models-llm.yaml: {e}");
            return Vec::new();
        }
    };
    get_model_catalog_with_config(cache_dir, &yaml)
}

/// Build the model catalog using an already-loaded YAML config (avoids re-reading from disk).
pub fn get_model_catalog_with_config(
    cache_dir: &str,
    yaml: &super::yaml_config::YamlConfig,
) -> Vec<ModelInfo> {
    let os_overhead = yaml.app.hardware.os_overhead_mb as u64;
    let sys = get_system_info_with_overhead(os_overhead);
    let available = sys.available_for_model_mb;

    // Memory safety margin: multiply model RAM estimate by this factor (e.g. 1.2)
    // so that a model requiring 1000 MB is treated as needing 1200 MB.
    let safety_margin = yaml.tuning.llm.memory_safety_margin;

    // Memory warning: log a warning when used RAM exceeds this fraction of total.
    let warning_threshold = yaml.tuning.llm.memory_warning_threshold;
    let used_ratio = if sys.total_ram_mb > 0 {
        1.0 - (available as f32 / sys.total_ram_mb as f32)
    } else {
        0.0
    };
    if used_ratio > warning_threshold {
        tracing::warn!(
            used_ratio = format!("{:.1}%", used_ratio * 100.0),
            threshold = format!("{:.0}%", warning_threshold * 100.0),
            available_mb = available,
            total_mb = sys.total_ram_mb,
            "System memory usage exceeds warning threshold"
        );
    }

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
            // Apply safety margin: model needs min_ram_mb * safety_margin to "fit".
            let required_with_margin = (min_ram_mb as f32 * safety_margin) as u64;
            let fits = required_with_margin <= available;
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
                family: entry.family.clone(),
                chat_template: entry.chat_template.clone(),
                rag_capable: entry.rag_capable,
                tool_calling: entry.tool_calling,
                default_for_ram_mb: entry.default_for_ram_mb,
                notes: entry.notes.clone(),
                cost_per_1m_input: entry.cost_per_1m_input,
                cost_per_1m_output: entry.cost_per_1m_output,
            })
        })
        .collect()
}

/// Find the best model for the available hardware.
///
/// Uses `default_for_ram_mb` from the YAML catalog: picks the model whose
/// `default_for_ram_mb` threshold is the highest value that still fits in the
/// user's available RAM.  Falls back to the largest model that fits if no
/// model has `default_for_ram_mb` set.
#[allow(dead_code)]
pub fn recommend_model(cache_dir: &str) -> Option<ModelInfo> {
    let catalog = get_model_catalog(cache_dir);
    let recommended: Vec<_> = catalog.into_iter().filter(|m| m.recommended).collect();

    // First try: pick the model with the highest default_for_ram_mb that fits.
    let sys = get_system_info();
    let available = sys.available_for_model_mb;
    let by_ram_default = recommended
        .iter()
        .filter(|m| {
            m.default_for_ram_mb
                .map(|threshold| (threshold as u64) <= available)
                .unwrap_or(false)
        })
        .max_by_key(|m| m.default_for_ram_mb.unwrap_or(0));

    if let Some(model) = by_ram_default {
        return Some(model.clone());
    }

    // Fallback: largest model that fits (catalog preserves YAML order, sorted by size).
    recommended.into_iter().next_back()
}
