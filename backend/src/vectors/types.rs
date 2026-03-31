//! Core types for the vector intelligence layer.
//!
//! These types form the value objects of the Email Intelligence bounded context (DDD-001).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a vector document in the store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VectorId(pub Uuid);

impl VectorId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for VectorId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for VectorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An embedding vector with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDocument {
    /// Unique vector ID within the store.
    pub id: VectorId,
    /// Links to the source email in SQLite.
    pub email_id: String,
    /// The embedding vector (e.g., 384-dimensional for MiniLM).
    pub vector: Vec<f32>,
    /// Filterable metadata attributes.
    pub metadata: HashMap<String, String>,
    /// Which collection this belongs to.
    pub collection: VectorCollection,
    /// When the embedding was created.
    pub created_at: DateTime<Utc>,
}

/// The different vector collections for multi-asset search.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VectorCollection {
    /// Primary email text embeddings (subject + from + body).
    EmailText,
    /// OCR text from inline images and image attachments.
    ImageText,
    /// CLIP visual embeddings from images.
    ImageVisual,
    /// Extracted text from PDF/DOCX/XLSX attachments.
    AttachmentText,
}

impl std::fmt::Display for VectorCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VectorCollection::EmailText => write!(f, "email_text"),
            VectorCollection::ImageText => write!(f, "image_text"),
            VectorCollection::ImageVisual => write!(f, "image_visual"),
            VectorCollection::AttachmentText => write!(f, "attachment_text"),
        }
    }
}

/// A search result with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredResult {
    pub document: VectorDocument,
    pub score: f32,
}

/// Search parameters for vector search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchParams {
    /// The query vector.
    pub vector: Vec<f32>,
    /// Maximum number of results.
    pub limit: usize,
    /// Which collection to search.
    pub collection: VectorCollection,
    /// Optional metadata filters (key=value).
    pub filters: Option<HashMap<String, String>>,
    /// Minimum similarity score threshold.
    pub min_score: Option<f32>,
}

/// Email category for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmailCategory {
    Work,
    Personal,
    Finance,
    Shopping,
    Social,
    Newsletter,
    Marketing,
    Notification,
    Alerts,
    Promotions,
    Travel,
    Uncategorized,
}

impl std::fmt::Display for EmailCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmailCategory::Work => write!(f, "Work"),
            EmailCategory::Personal => write!(f, "Personal"),
            EmailCategory::Finance => write!(f, "Finance"),
            EmailCategory::Shopping => write!(f, "Shopping"),
            EmailCategory::Social => write!(f, "Social"),
            EmailCategory::Newsletter => write!(f, "Newsletter"),
            EmailCategory::Marketing => write!(f, "Marketing"),
            EmailCategory::Notification => write!(f, "Notification"),
            EmailCategory::Alerts => write!(f, "Alerts"),
            EmailCategory::Promotions => write!(f, "Promotions"),
            EmailCategory::Travel => write!(f, "Travel"),
            EmailCategory::Uncategorized => write!(f, "Uncategorized"),
        }
    }
}

/// Result of email categorization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryResult {
    pub category: EmailCategory,
    pub confidence: f32,
    pub method: String, // "vector_centroid" or "llm_fallback"
}

/// A category centroid — the average vector representing a category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryCentroid {
    pub category: EmailCategory,
    pub vector: Vec<f32>,
    pub email_count: u64,
    pub last_updated: DateTime<Utc>,
}

/// Health status of the vector service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    pub status: String,
    pub store_healthy: bool,
    pub embedding_available: bool,
    pub store_stats: VectorStats,
}

/// Statistics about the vector store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorStats {
    pub total_vectors: u64,
    pub collections: HashMap<String, u64>,
    pub dimensions: usize,
    pub memory_bytes: u64,
    pub index_type: String,
}

/// Embedding status tracked in SQLite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingStatus {
    Pending,
    Embedded,
    Failed,
    Stale,
}

impl std::fmt::Display for EmbeddingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddingStatus::Pending => write!(f, "pending"),
            EmbeddingStatus::Embedded => write!(f, "embedded"),
            EmbeddingStatus::Failed => write!(f, "failed"),
            EmbeddingStatus::Stale => write!(f, "stale"),
        }
    }
}
