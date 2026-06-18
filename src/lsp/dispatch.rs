//! The dispatch / cancellation seam: [`run_if_current`].
//!
//! A request handler computes a result against a document snapshot, but by the time the
//! work finishes a newer `didChange` may have superseded it. This module is the single
//! place that enforces the discard rule: it awaits the work, then re-checks the document
//! generation via [`DocumentLifecycle::is_current`] and delivers the result ONLY if the
//! token is still current. A superseded result is dropped (returns `None`) rather than
//! handed back stale.
//!
//! This is deliberately tiny and pure over a [`DocumentLifecycle`] handle so it is
//! unit-testable without a live server, and so every model-tier and static-tier handler
//! routes its supersede check through the same logic.

use std::future::Future;

use crate::lsp::lifecycle::{DocumentLifecycle, GenerationToken};

/// Awaits `work`, then delivers its result only if `token` is still current.
///
/// Re-checks the live generation AFTER the work completes: if a newer edit advanced the
/// document past `token`'s generation (or it was closed), the result is discarded and
/// `None` is returned. Otherwise the freshly computed value is returned as `Some`.
///
/// # Example
///
/// ```
/// use gradle_analyzer::lsp::dispatch::run_if_current;
/// use gradle_analyzer::lsp::lifecycle::DocumentLifecycle;
/// use tower_lsp::lsp_types::Url;
///
/// # tokio_test_block_on(async {
/// let lifecycle = DocumentLifecycle::new();
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
/// let token = lifecycle.open(uri.clone(), 1, "plugins {}").await;
///
/// // No superseding change: the computed result is delivered.
/// let delivered = run_if_current(&lifecycle, &token, async { vec![1, 2, 3] }).await;
/// assert_eq!(delivered, Some(vec![1, 2, 3]));
///
/// // A change supersedes the original token: a result computed under it is dropped.
/// lifecycle.change(&uri, 2, "plugins { java }").await;
/// let dropped = run_if_current(&lifecycle, &token, async { vec![9] }).await;
/// assert_eq!(dropped, None);
/// # });
/// # fn tokio_test_block_on<F: std::future::Future>(f: F) -> F::Output {
/// #     tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
/// # }
/// ```
pub async fn run_if_current<F, T>(
    lifecycle: &DocumentLifecycle,
    token: &GenerationToken,
    work: F,
) -> Option<T>
where
    F: Future<Output = T>,
{
    tracing::debug!(uri = %token.uri(), generation = token.generation(), "dispatch: work issued");
    let result = work.await;
    if lifecycle.is_current(token).await {
        tracing::debug!(uri = %token.uri(), generation = token.generation(), "dispatch: result delivered");
        Some(result)
    } else {
        tracing::debug!(
            uri = %token.uri(),
            generation = token.generation(),
            "dispatch: result discarded (generation superseded)"
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    fn uri() -> Url {
        Url::from_file_path("/proj/app/build.gradle.kts").unwrap()
    }

    #[tokio::test]
    async fn delivers_result_when_token_still_current() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();
        let token = lifecycle.open(u.clone(), 1, "x").await;

        let out = run_if_current(&lifecycle, &token, async { 100_u32 }).await;
        assert_eq!(out, Some(100));
    }

    #[tokio::test]
    async fn discards_result_after_superseding_change() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();
        let stale_token = lifecycle.open(u.clone(), 1, "x").await;

        // A newer edit lands before the (modeled) work would be delivered.
        lifecycle.change(&u, 2, "y").await;

        let out = run_if_current(&lifecycle, &stale_token, async { 100_u32 }).await;
        assert_eq!(out, None, "stale-generation result must NOT be delivered");
    }

    #[tokio::test]
    async fn discards_result_after_close() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();
        let token = lifecycle.open(u.clone(), 1, "x").await;
        lifecycle.close(&u).await;

        let out = run_if_current(&lifecycle, &token, async { 5_u32 }).await;
        assert_eq!(out, None);
    }
}
