//! Structured logging bootstrap.
//!
//! Initializes a `tracing` subscriber that writes human-readable, leveled logs to
//! **stderr** (never stdout, which is the LSP JSON-RPC bus). Bootstrapping is tolerant
//! of an already-installed global subscriber via `try_init`, so repeated calls — e.g.
//! across tests — log a debug note instead of panicking.

use tracing_subscriber::EnvFilter;

/// Installs the global tracing subscriber if none is set yet.
///
/// Idempotent and panic-free: a second call (or a test harness that already installed
/// a subscriber) is a no-op. The level filter defaults to `info` and honors `RUST_LOG`.
///
/// # Example
///
/// ```
/// use gradle_analyzer::app::logging::init_tracing;
///
/// init_tracing();
/// init_tracing(); // safe to call again; no panic.
/// ```
pub fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let result = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true)
        .try_init();

    if result.is_err() {
        tracing::debug!("tracing subscriber already installed; skipping re-init");
    }
}
