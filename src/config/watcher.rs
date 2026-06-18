//! A thin, debounced filesystem watcher that triggers config hot-reload.
//!
//! The watcher's only job is to notice changes to the configuration files and, after a
//! configurable quiet window, ask the [`ConfigManager`] to reload. All merge,
//! validation, and atomic-swap logic lives in the manager and loader (which are
//! unit-tested directly), so this module stays intentionally small.

use std::path::PathBuf;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, error, warn};

use crate::config::error::ConfigError;
use crate::config::manager::ConfigManager;

/// A running config watcher. Dropping it stops watching.
///
/// The watcher keeps the underlying OS watch handle alive and owns the debounce task.
/// Construct it with [`ConfigWatcher::spawn`].
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    _debounce_task: tokio::task::JoinHandle<()>,
}

impl ConfigWatcher {
    /// Begins watching `paths`, debouncing changes by `debounce` before reloading.
    ///
    /// Filesystem and notify errors are logged (never panic); a failed reload leaves
    /// the live snapshot untouched. Requires a Tokio runtime context for the debounce
    /// task. Returns the OS watcher error only if the initial watch registration fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use gradle_analyzer::config::{ConfigManager, GradleAnalyzerConfig};
    /// # use gradle_analyzer::config::watcher::ConfigWatcher;
    /// # use std::path::PathBuf;
    /// # use std::time::Duration;
    /// # async fn demo() -> notify::Result<()> {
    /// let manager = ConfigManager::new(GradleAnalyzerConfig::default());
    /// let _watcher = ConfigWatcher::spawn(
    ///     manager,
    ///     vec![PathBuf::from("gradle-analyzer.toml")],
    ///     Duration::from_millis(250),
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn spawn(
        manager: ConfigManager,
        paths: Vec<PathBuf>,
        debounce: Duration,
    ) -> notify::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            match res {
                Ok(_event) => {
                    let _ = tx.send(());
                }
                Err(err) => warn!(error = %err, "config watch event error"),
            }
        })?;

        for path in &paths {
            if let Some(parent) = watch_target(path) {
                if let Err(err) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
                    warn!(path = %parent.display(), error = %err, "failed to watch config path");
                }
            }
        }

        let debounce_task = tokio::spawn(debounce_loop(manager, rx, debounce));

        Ok(Self {
            _watcher: watcher,
            _debounce_task: debounce_task,
        })
    }
}

/// Chooses what to hand to `notify::watch`: the file's parent dir if it exists,
/// otherwise the path itself. Watching the parent survives editor save-replace cycles.
fn watch_target(path: &PathBuf) -> Option<PathBuf> {
    match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Some(PathBuf::from(".")),
        Some(parent) if parent.exists() => Some(parent.to_path_buf()),
        _ => Some(path.clone()),
    }
}

/// Collapses bursts of change events into a single reload after a quiet window.
async fn debounce_loop(
    manager: ConfigManager,
    mut rx: mpsc::UnboundedReceiver<()>,
    debounce: Duration,
) {
    loop {
        // Block until the first event of a new burst.
        if rx.recv().await.is_none() {
            return;
        }

        // Drain further events until the channel stays quiet for `debounce`.
        let mut deadline = Instant::now() + debounce;
        loop {
            let sleep = tokio::time::sleep_until(deadline);
            tokio::select! {
                maybe = rx.recv() => match maybe {
                    Some(()) => deadline = Instant::now() + debounce,
                    None => return,
                },
                _ = sleep => break,
            }
        }

        match manager.reload() {
            Ok(()) => debug!("config reloaded after debounce window"),
            Err(err) => report_reload_error(&err),
        }
    }
}

fn report_reload_error(err: &ConfigError) {
    error!(error = %err, "config reload failed; keeping previous snapshot");
}
