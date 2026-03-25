//! Hot-reload configuration using file-mtime polling (R-08).
//!
//! Watches configuration files for changes by polling their modification times
//! at a configurable interval. When a change is detected, subscribers are
//! notified via a `tokio::sync::watch` channel so they can reload.
//!
//! This avoids adding the `notify` crate as a dependency. Polling is simple,
//! portable, and sufficient for configuration files that change infrequently.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{watch, RwLock};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// ConfigWatcher
// ---------------------------------------------------------------------------

/// Watches config files by polling their modification times and notifies
/// subscribers when any file changes.
pub struct ConfigWatcher {
    config_paths: Vec<PathBuf>,
    tx: watch::Sender<()>,
    /// Subscribers clone this receiver to be notified of changes.
    pub rx: watch::Receiver<()>,
}

impl ConfigWatcher {
    /// Create a watcher for the given config file paths.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let (tx, rx) = watch::channel(());
        Self {
            config_paths: paths,
            tx,
            rx,
        }
    }

    /// Start watching in a background task.
    ///
    /// Polls file modification times every `interval`. When any file's mtime
    /// changes, sends a notification on the watch channel.
    pub fn start(self: Arc<Self>, interval: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut last_mtimes: HashMap<PathBuf, SystemTime> = HashMap::new();

            // Seed initial mtimes.
            for path in &self.config_paths {
                if let Ok(meta) = tokio::fs::metadata(path).await {
                    if let Ok(mtime) = meta.modified() {
                        last_mtimes.insert(path.clone(), mtime);
                    }
                }
            }

            info!(
                paths = ?self.config_paths,
                interval_secs = interval.as_secs(),
                "Config watcher started"
            );

            loop {
                tokio::time::sleep(interval).await;

                let mut changed = false;

                for path in &self.config_paths {
                    let current_mtime = match tokio::fs::metadata(path).await {
                        Ok(meta) => meta.modified().ok(),
                        Err(_) => {
                            // File might have been deleted -- ignore.
                            continue;
                        }
                    };

                    if let Some(current) = current_mtime {
                        let previous = last_mtimes.get(path).copied();
                        if previous.is_none() || previous != Some(current) {
                            debug!(path = %path.display(), "Config file changed");
                            last_mtimes.insert(path.clone(), current);
                            changed = true;
                        }
                    }
                }

                if changed {
                    if self.tx.send(()).is_err() {
                        // All receivers dropped -- stop watching.
                        debug!("All config watchers dropped, stopping poll loop");
                        break;
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// ReloadableConfig
// ---------------------------------------------------------------------------

/// Holds a configuration value that can be atomically swapped via `Arc`.
///
/// Uses the "Arc swap" pattern: readers get a cheap `Arc<T>` clone, while
/// the reload path acquires a write lock only briefly to swap the inner Arc.
pub struct ReloadableConfig<T: Clone + Send + Sync + 'static> {
    current: Arc<RwLock<Arc<T>>>,
    loader: Box<dyn Fn() -> Result<T, anyhow::Error> + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> ReloadableConfig<T> {
    /// Create with an initial value and a loader closure that produces new values.
    pub fn new(
        initial: T,
        loader: impl Fn() -> Result<T, anyhow::Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            current: Arc::new(RwLock::new(Arc::new(initial))),
            loader: Box::new(loader),
        }
    }

    /// Get the current configuration. This is a cheap `Arc` clone.
    pub async fn get(&self) -> Arc<T> {
        self.current.read().await.clone()
    }

    /// Reload from the loader. Returns `Ok(true)` if the reload succeeded
    /// (regardless of whether the value actually changed).
    pub async fn reload(&self) -> Result<bool, anyhow::Error> {
        let new_value = (self.loader)()?;
        let new_arc = Arc::new(new_value);
        let mut guard = self.current.write().await;
        *guard = new_arc;
        info!("Configuration reloaded successfully");
        Ok(true)
    }

    /// Start an auto-reload loop that reloads whenever the given watch
    /// receiver is notified.
    pub fn auto_reload(
        self: Arc<Self>,
        mut rx: watch::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if rx.changed().await.is_err() {
                    // Sender dropped -- stop.
                    debug!("Config watcher sender dropped, stopping auto-reload");
                    break;
                }

                match self.reload().await {
                    Ok(_) => {
                        info!("Config auto-reloaded after file change");
                    }
                    Err(e) => {
                        warn!(error = %e, "Config auto-reload failed, keeping previous config");
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_reloadable_config_initial_value() {
        let config = ReloadableConfig::new(42u32, || Ok(99));
        let val = config.get().await;
        assert_eq!(*val, 42);
    }

    #[tokio::test]
    async fn test_reloadable_config_reload() {
        let config = ReloadableConfig::new(1u32, || Ok(2));
        let reloaded = config.reload().await.unwrap();
        assert!(reloaded);
        assert_eq!(*config.get().await, 2);
    }

    #[tokio::test]
    async fn test_reloadable_config_reload_error() {
        let config = ReloadableConfig::new(1u32, || {
            Err(anyhow::anyhow!("load failed"))
        });
        let result = config.reload().await;
        assert!(result.is_err());
        // Original value preserved.
        assert_eq!(*config.get().await, 1);
    }

    #[tokio::test]
    async fn test_reloadable_config_concurrent_reads() {
        let config = Arc::new(ReloadableConfig::new(100u32, || Ok(200)));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let c = config.clone();
            handles.push(tokio::spawn(async move { *c.get().await }));
        }

        for h in handles {
            let val = h.await.unwrap();
            assert!(val == 100 || val == 200);
        }
    }

    #[tokio::test]
    async fn test_reloadable_config_auto_reload() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let config = Arc::new(ReloadableConfig::new(0u32, move || {
            let val = counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(val + 1)
        }));

        let (tx, rx) = watch::channel(());

        let handle = config.clone().auto_reload(rx);

        // Trigger a reload.
        tx.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let val = *config.get().await;
        assert!(val >= 1, "expected reloaded value >= 1, got {val}");

        // Drop sender to stop the loop.
        drop(tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_config_watcher_creation() {
        let paths = vec![PathBuf::from("/tmp/test_config.yaml")];
        let watcher = ConfigWatcher::new(paths.clone());
        assert_eq!(watcher.config_paths.len(), 1);
    }

    #[tokio::test]
    async fn test_config_watcher_detects_change() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");
        tokio::fs::write(&config_path, b"port: 8080").await.unwrap();

        let watcher = Arc::new(ConfigWatcher::new(vec![config_path.clone()]));
        let mut rx = watcher.rx.clone();

        let handle = watcher.start(Duration::from_millis(50));

        // Wait for initial poll cycle.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Modify the file.
        tokio::fs::write(&config_path, b"port: 9090").await.unwrap();

        // Should receive a change notification.
        let result = tokio::time::timeout(Duration::from_secs(2), rx.changed()).await;
        assert!(result.is_ok(), "expected change notification");

        handle.abort();
    }

    #[test]
    fn test_config_watcher_new_empty_paths() {
        let watcher = ConfigWatcher::new(Vec::new());
        assert!(watcher.config_paths.is_empty());
    }
}
