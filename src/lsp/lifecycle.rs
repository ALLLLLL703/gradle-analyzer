//! The [`DocumentLifecycle`]: the shared open/change/close document model + generations.
//!
//! Every later feature (diagnostics, completion, symbols, navigation) reads the document
//! text from ONE place: the [`WorkspaceDocumentStore`] this type owns behind an async
//! [`Mutex`]. On top of the store it tracks a per-URI monotonic **generation** counter so
//! the dispatch layer can discard a stale result: when a newer `didChange` supersedes an
//! in-flight request, the request's generation no longer matches the document's current
//! generation and its result is dropped instead of delivered.
//!
//! The lock is `tokio::sync::Mutex` (async) so a handler awaiting it never blocks the
//! runtime worker thread. Mutations are wrapped in `tracing`.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp::lsp_types::Url;

use crate::gradle::workspace::{TrackedDocument, WorkspaceDocumentStore};

/// A snapshot of "what was current" for a URI at the moment a request was issued.
///
/// A feature computation is logically tagged with the token returned when its triggering
/// edit landed. [`DocumentLifecycle::is_current`] later compares the token's generation
/// against the live generation; a mismatch means a newer edit superseded this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationToken {
    uri: Url,
    generation: u64,
}

impl GenerationToken {
    /// Returns the URI this token is scoped to.
    pub fn uri(&self) -> &Url {
        &self.uri
    }

    /// Returns the generation captured when this token was issued.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Owns the document store plus per-URI generations behind one async lock.
///
/// Clone is cheap (an `Arc` bump) and all clones share the same state, so the server can
/// hand a handle to spawned tasks. `open`/`change` bump the generation and return a
/// [`GenerationToken`]; `close` removes both the snapshot and the generation entry.
///
/// # Example
///
/// ```
/// use gradle_analyzer::lsp::lifecycle::DocumentLifecycle;
/// use tower_lsp::lsp_types::Url;
///
/// # tokio_test_block_on(async {
/// let lifecycle = DocumentLifecycle::new();
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
///
/// let opened = lifecycle.open(uri.clone(), 1, "plugins {}").await;
/// assert!(lifecycle.is_current(&opened).await);
///
/// // A change bumps the generation; the old token is no longer current.
/// let changed = lifecycle.change(&uri, 2, "plugins { java }").await;
/// assert!(!lifecycle.is_current(&opened).await);
/// assert!(lifecycle.is_current(&changed.unwrap()).await);
/// # });
/// # fn tokio_test_block_on<F: std::future::Future>(f: F) -> F::Output {
/// #     tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
/// # }
/// ```
#[derive(Clone, Default)]
pub struct DocumentLifecycle {
    inner: Arc<Mutex<LifecycleState>>,
}

#[derive(Default)]
struct LifecycleState {
    store: WorkspaceDocumentStore,
    generations: HashMap<Url, u64>,
}

impl DocumentLifecycle {
    /// Creates an empty lifecycle.
    pub fn new() -> DocumentLifecycle {
        DocumentLifecycle {
            inner: Arc::new(Mutex::new(LifecycleState::default())),
        }
    }

    /// Opens `uri` at `version` with `text`, bumping its generation.
    ///
    /// Returns a [`GenerationToken`] capturing the generation now current for `uri`.
    pub async fn open(
        &self,
        uri: Url,
        version: i32,
        text: impl Into<String>,
    ) -> GenerationToken {
        let mut state = self.inner.lock().await;
        state.store.open(uri.clone(), version, text);
        let generation = bump_generation(&mut state.generations, &uri);
        tracing::info!(uri = %uri, version, generation, "document opened (lifecycle)");
        GenerationToken { uri, generation }
    }

    /// Applies a full-text change to `uri`, bumping its generation.
    ///
    /// Returns a fresh [`GenerationToken`], or `None` if the document is not open (a
    /// change before open is a deterministic no-op that does not invent a generation).
    pub async fn change(
        &self,
        uri: &Url,
        version: i32,
        text: impl Into<String>,
    ) -> Option<GenerationToken> {
        let mut state = self.inner.lock().await;
        state.store.change(uri, version, text)?;
        let generation = bump_generation(&mut state.generations, uri);
        tracing::info!(uri = %uri, version, generation, "document changed (lifecycle)");
        Some(GenerationToken {
            uri: uri.clone(),
            generation,
        })
    }

    /// Closes `uri`, removing its snapshot and generation entry.
    ///
    /// Returns `true` if a document was actually removed.
    pub async fn close(&self, uri: &Url) -> bool {
        let mut state = self.inner.lock().await;
        let removed = state.store.close(uri).is_some();
        if removed {
            state.generations.remove(uri);
            tracing::info!(uri = %uri, "document closed (lifecycle)");
        }
        removed
    }

    /// Returns a detached snapshot of `uri`'s current text, if open.
    pub async fn snapshot(&self, uri: &Url) -> Option<TrackedDocument> {
        let state = self.inner.lock().await;
        state.store.get(uri)
    }

    /// Returns the live generation for `uri`, or `None` if it is not open.
    pub async fn current_generation(&self, uri: &Url) -> Option<u64> {
        let state = self.inner.lock().await;
        state.generations.get(uri).copied()
    }

    /// Builds a [`GenerationToken`] tagging `uri` at `generation`.
    ///
    /// Used by read-only request handlers (e.g. `documentSymbol`) that did not themselves
    /// issue an edit but still want their result dropped if a later change supersedes the
    /// generation they read. Pairs with [`current_generation`](Self::current_generation).
    pub fn token_for(&self, uri: Url, generation: u64) -> GenerationToken {
        GenerationToken { uri, generation }
    }

    /// Returns `true` if `token` still matches the live generation for its URI.
    ///
    /// A `false` result means a newer edit superseded the request the token tags (or the
    /// document was closed), so any result computed under that token must be discarded.
    pub async fn is_current(&self, token: &GenerationToken) -> bool {
        let state = self.inner.lock().await;
        state.generations.get(&token.uri).copied() == Some(token.generation)
    }

    /// Returns the number of currently open documents.
    pub async fn open_count(&self) -> usize {
        let state = self.inner.lock().await;
        state.store.len()
    }
}

/// Increments and returns the generation for `uri`, starting at 1 on first open.
fn bump_generation(generations: &mut HashMap<Url, u64>, uri: &Url) -> u64 {
    let entry = generations.entry(uri.clone()).or_insert(0);
    *entry += 1;
    *entry
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri() -> Url {
        Url::from_file_path("/proj/app/build.gradle.kts").unwrap()
    }

    #[tokio::test]
    async fn open_change_close_mutates_shared_store() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();

        lifecycle.open(u.clone(), 1, "plugins {}").await;
        assert_eq!(lifecycle.open_count().await, 1);
        assert_eq!(lifecycle.snapshot(&u).await.unwrap().text(), "plugins {}");

        lifecycle.change(&u, 2, "plugins { java }").await;
        let snap = lifecycle.snapshot(&u).await.unwrap();
        assert_eq!(snap.version(), 2);
        assert_eq!(snap.text(), "plugins { java }");

        assert!(lifecycle.close(&u).await);
        assert!(lifecycle.snapshot(&u).await.is_none());
        assert_eq!(lifecycle.open_count().await, 0);
    }

    #[tokio::test]
    async fn change_bumps_generation_and_supersedes_old_token() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();

        let opened = lifecycle.open(u.clone(), 1, "a").await;
        assert!(lifecycle.is_current(&opened).await);
        assert_eq!(opened.generation(), 1);

        let changed = lifecycle.change(&u, 2, "b").await.unwrap();
        assert_eq!(changed.generation(), 2);
        // The newer token is current; the original token is now stale.
        assert!(lifecycle.is_current(&changed).await);
        assert!(!lifecycle.is_current(&opened).await);
    }

    #[tokio::test]
    async fn change_before_open_is_a_noop_token() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();
        assert!(lifecycle.change(&u, 5, "ignored").await.is_none());
        assert!(lifecycle.current_generation(&u).await.is_none());
    }

    #[tokio::test]
    async fn close_clears_generation_so_token_is_not_current() {
        let lifecycle = DocumentLifecycle::new();
        let u = uri();
        let token = lifecycle.open(u.clone(), 1, "x").await;
        assert!(lifecycle.close(&u).await);
        assert!(!lifecycle.is_current(&token).await);
        assert!(lifecycle.current_generation(&u).await.is_none());
    }
}
