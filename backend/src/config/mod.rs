//! Configuration management module for Emailibrium (R-08).
//!
//! Provides hot-reload configuration using file-mtime polling and an
//! `Arc<RwLock<Arc<T>>>` swap pattern for zero-downtime config updates.

pub mod hot_reload;
