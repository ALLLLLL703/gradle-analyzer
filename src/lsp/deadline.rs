//! The bounded-timeout primitive: [`with_deadline`] and its [`Deadline`] outcome.
//!
//! A model-dependent request (one that may wait on the JVM sidecar tier, Tasks 10+)
//! must NEVER stall the `tower-lsp` event loop. This module is the single place that
//! enforces that: it races a future against a config-backed deadline using
//! [`tokio::time::timeout`] and yields a deterministic [`Deadline::Pending`] when the
//! deadline elapses, so the handler returns an empty/pending result instead of blocking.
//!
//! Static-tier handlers (diagnostics, document symbols) are answered from in-memory
//! snapshots and MUST bypass this helper entirely — they never wait on the model.

use std::future::Future;
use std::time::Duration;

/// The outcome of awaiting a future under a deadline.
///
/// `Ready(T)` carries the value the future produced within the deadline; `Pending`
/// means the deadline elapsed first and the caller should return its empty/pending
/// result. Modeling this as an enum (rather than `Option`) keeps the "deadline
/// exceeded" branch explicit and self-documenting at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deadline<T> {
    /// The future completed within the deadline.
    Ready(T),
    /// The deadline elapsed before the future completed.
    Pending,
}

impl<T> Deadline<T> {
    /// Returns `true` if the future completed within the deadline.
    pub fn is_ready(&self) -> bool {
        matches!(self, Deadline::Ready(_))
    }

    /// Returns the value if ready, or `None` if the deadline elapsed.
    pub fn into_option(self) -> Option<T> {
        match self {
            Deadline::Ready(value) => Some(value),
            Deadline::Pending => None,
        }
    }
}

/// Awaits `fut` but gives up after `deadline_ms` milliseconds.
///
/// On completion within the budget the value is wrapped in [`Deadline::Ready`]; on
/// timeout a `deadline exceeded` trace is emitted and [`Deadline::Pending`] is returned.
/// Because it never awaits past the deadline, a slow or hung model future cannot wedge
/// the event loop — control always returns to the loop within `deadline_ms`.
///
/// The `deadline_ms` value is supplied by the caller, which must read it from
/// [`crate::config::ConfigManager`] (e.g. `sidecar.model_request_deadline_ms`) rather
/// than hardcoding it.
///
/// # Example
///
/// ```
/// use gradle_analyzer::lsp::deadline::{with_deadline, Deadline};
///
/// # tokio_test_block_on(async {
/// // A future that completes immediately is Ready well within any positive deadline.
/// let outcome = with_deadline(async { 7 }, 1_000).await;
/// assert_eq!(outcome, Deadline::Ready(7));
/// # });
/// # fn tokio_test_block_on<F: std::future::Future>(f: F) -> F::Output {
/// #     tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(f)
/// # }
/// ```
pub async fn with_deadline<F, T>(fut: F, deadline_ms: u64) -> Deadline<T>
where
    F: Future<Output = T>,
{
    match tokio::time::timeout(Duration::from_millis(deadline_ms), fut).await {
        Ok(value) => Deadline::Ready(value),
        Err(_elapsed) => {
            tracing::warn!(deadline_ms, "model request deadline exceeded; returning pending");
            Deadline::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fast_future_is_ready_within_deadline() {
        let outcome = with_deadline(async { 42_u32 }, 1_000).await;
        assert_eq!(outcome, Deadline::Ready(42));
        assert!(outcome.is_ready());
    }

    #[tokio::test(start_paused = true)]
    async fn slow_future_yields_pending_at_deadline_without_wallclock_wait() {
        // A future that never resolves; under paused virtual time the deadline fires
        // deterministically at ~0 wall-clock once there is nothing else to drive.
        let never = std::future::pending::<u32>();
        let outcome = with_deadline(never, 25).await;
        assert_eq!(outcome, Deadline::Pending);
        assert_eq!(outcome.into_option(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn event_loop_stays_live_after_a_deadline() {
        // First call times out; a SUBSEQUENT immediate call still answers, proving the
        // helper returned control to the loop rather than wedging it.
        let timed_out = with_deadline(std::future::pending::<u32>(), 10).await;
        assert_eq!(timed_out, Deadline::Pending);

        let answered = with_deadline(async { 1_u32 }, 10).await;
        assert_eq!(answered, Deadline::Ready(1));
    }
}
