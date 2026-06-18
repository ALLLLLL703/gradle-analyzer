//! The [`ConfigManager`]: lock-free shared access to the live config snapshot.
//!
//! The manager holds the current [`GradleAnalyzerConfig`] behind an
//! [`arc_swap::ArcSwap`], so reads ([`ConfigManager::snapshot`]) are cheap and
//! lock-free while a reload atomically swaps in a freshly loaded snapshot. The
//! reload-apply path is exposed and unit-tested directly, independent of any
//! filesystem-watch latency.

use std::sync::Arc;

use arc_swap::ArcSwap;
use tracing::info;

use crate::config::error::ConfigError;
use crate::config::loader::ConfigSources;
use crate::config::model::GradleAnalyzerConfig;

/// Shared, hot-reloadable holder of the current configuration snapshot.
///
/// Clone is cheap (an `Arc` bump) and clones share the same underlying cell, so every
/// holder sees a reload immediately. Reads return an `Arc` snapshot that stays valid
/// even while a concurrent reload swaps in a newer one.
///
/// # Example
///
/// ```
/// use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
///
/// let manager = ConfigManager::new(GradleAnalyzerConfig::default());
/// let before = manager.snapshot().watcher.debounce_ms;
///
/// let mut next = GradleAnalyzerConfig::default();
/// next.watcher.debounce_ms = before + 100;
/// manager.apply(next);
///
/// assert_eq!(manager.snapshot().watcher.debounce_ms, before + 100);
/// ```
#[derive(Clone)]
pub struct ConfigManager {
    current: Arc<ArcSwap<GradleAnalyzerConfig>>,
    sources: Arc<ConfigSources>,
}

impl ConfigManager {
    /// Creates a manager seeded with `initial` and no reload sources.
    pub fn new(initial: GradleAnalyzerConfig) -> Self {
        Self {
            current: Arc::new(ArcSwap::from_pointee(initial)),
            sources: Arc::new(ConfigSources::default()),
        }
    }

    /// Creates a manager by loading `sources`, remembering them for later reloads.
    pub fn from_sources(sources: ConfigSources) -> Result<Self, ConfigError> {
        let initial = sources.load()?;
        Ok(Self {
            current: Arc::new(ArcSwap::from_pointee(initial)),
            sources: Arc::new(sources),
        })
    }

    /// Returns the current snapshot. Cheap and lock-free.
    pub fn snapshot(&self) -> Arc<GradleAnalyzerConfig> {
        self.current.load_full()
    }

    /// Atomically swaps in a fully-built snapshot (the reload-apply primitive).
    ///
    /// Kept public and synchronous so the swap can be unit-tested without filesystem
    /// or watcher timing; the watcher and `reload` both funnel through here.
    pub fn apply(&self, next: GradleAnalyzerConfig) {
        self.current.store(Arc::new(next));
    }

    /// Re-reads the remembered sources and applies the result on success.
    ///
    /// On a load/validation error the live snapshot is left untouched so a bad edit
    /// never poisons a running server; the error is returned for the caller to log.
    pub fn reload(&self) -> Result<(), ConfigError> {
        let next = self.sources.load()?;
        self.apply(next);
        info!("configuration snapshot reloaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_swaps_live_snapshot() {
        let manager = ConfigManager::new(GradleAnalyzerConfig::default());
        let held_before = manager.snapshot();
        assert_eq!(held_before.watcher.debounce_ms, 250);

        let mut next = GradleAnalyzerConfig::default();
        next.watcher.debounce_ms = 999;
        manager.apply(next);

        assert_eq!(manager.snapshot().watcher.debounce_ms, 999);
        // A snapshot captured before the swap stays valid and unchanged.
        assert_eq!(held_before.watcher.debounce_ms, 250);
    }

    #[test]
    fn reload_reads_new_file_not_cached_snapshot() {
        use std::fs;
        use std::io::Write;

        let dir = std::env::temp_dir().join(format!("ga-reload-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gradle-analyzer.toml");

        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"[watcher]\ndebounce_ms = 300\n").unwrap();
        drop(f);

        let manager = ConfigManager::from_sources(ConfigSources::workspace_only(path.clone()))
            .expect("initial load");
        assert_eq!(manager.snapshot().watcher.debounce_ms, 300);

        // Rewrite the file with a new value, then reload.
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"[watcher]\ndebounce_ms = 750\n").unwrap();
        drop(f);

        manager.reload().expect("reload");
        assert_eq!(manager.snapshot().watcher.debounce_ms, 750);
    }

    #[test]
    fn reload_keeps_old_snapshot_on_error() {
        use std::fs;
        use std::io::Write;

        let dir = std::env::temp_dir().join(format!("ga-reload-err-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gradle-analyzer.toml");

        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"[watcher]\ndebounce_ms = 321\n").unwrap();
        drop(f);

        let manager = ConfigManager::from_sources(ConfigSources::workspace_only(path.clone()))
            .expect("initial load");
        assert_eq!(manager.snapshot().watcher.debounce_ms, 321);

        // Corrupt the file; reload must error AND leave the live snapshot intact.
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"not [ valid toml").unwrap();
        drop(f);

        assert!(manager.reload().is_err());
        assert_eq!(manager.snapshot().watcher.debounce_ms, 321);
    }
}
