//! [`WorkspaceDocumentStore`]: the open/change/close lifecycle of tracked documents.
//!
//! The store owns the set of currently open [`TrackedDocument`] snapshots, keyed by URI,
//! and implements full-text synchronization for v1: a change REPLACES the stored snapshot
//! with a fresh one at a bumped version. It is parser- and sidecar-agnostic — it tracks
//! text and identity only — and resolves each document's [`GradleFileKind`] at open time
//! against the detected workspace root. Mutations are wrapped in `tracing`.

use std::collections::HashMap;
use std::path::PathBuf;

use tower_lsp::lsp_types::Url;

use crate::gradle::workspace::document::TrackedDocument;
use crate::gradle::workspace::kind::GradleFileKind;
use crate::gradle::workspace::root::{WorkspaceRoot, detect_workspace_root};

/// Tracks open documents and their full-text snapshots, keyed by URI.
///
/// `open` classifies and stores a new snapshot, `change` swaps in a replacement at a new
/// version, and `close` drops it. Reads (`get`, `snapshot`) hand back a cheap clone, so a
/// caller can hold a snapshot across later mutations without it changing underneath them.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::WorkspaceDocumentStore;
/// use tower_lsp::lsp_types::Url;
///
/// let mut store = WorkspaceDocumentStore::new();
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
/// // A file path with no real root on disk classifies as Unknown but still tracks.
/// store.open(uri.clone(), 1, "plugins {}");
/// assert_eq!(store.get(&uri).unwrap().version(), 1);
///
/// store.change(&uri, 2, "plugins { java }");
/// assert_eq!(store.get(&uri).unwrap().version(), 2);
///
/// store.close(&uri);
/// assert!(store.get(&uri).is_none());
/// ```
#[derive(Debug, Default)]
pub struct WorkspaceDocumentStore {
    documents: HashMap<Url, TrackedDocument>,
}

impl WorkspaceDocumentStore {
    /// Creates an empty store.
    pub fn new() -> WorkspaceDocumentStore {
        WorkspaceDocumentStore {
            documents: HashMap::new(),
        }
    }

    /// Opens `uri` at `version` with `text`, classifying and storing a snapshot.
    ///
    /// Returns the stored snapshot. Re-opening an already-open URI replaces it (an
    /// editor may resend `didOpen`); the classification is recomputed from the path.
    pub fn open(
        &mut self,
        uri: Url,
        version: i32,
        text: impl Into<String>,
    ) -> TrackedDocument {
        let kind = classify_uri(&uri);
        let document = TrackedDocument::new(uri.clone(), version, text.into(), kind);
        tracing::info!(uri = %uri, version, ?kind, "document opened");
        self.documents.insert(uri, document.clone());
        document
    }

    /// Applies a full-text change to `uri`, replacing its snapshot at `version`.
    ///
    /// Returns the new snapshot, or `None` if the document is not open (a change before
    /// open is a deterministic no-op rather than an error). The previous snapshot — and
    /// any clone a caller is holding — is left untouched.
    pub fn change(
        &mut self,
        uri: &Url,
        version: i32,
        text: impl Into<String>,
    ) -> Option<TrackedDocument> {
        let existing = self.documents.get(uri)?;
        let updated = existing.with_change(version, text.into());
        tracing::info!(uri = %uri, version, "document changed (full-text sync)");
        self.documents.insert(uri.clone(), updated.clone());
        Some(updated)
    }

    /// Closes `uri`, removing its snapshot. Returns the removed snapshot if present.
    pub fn close(&mut self, uri: &Url) -> Option<TrackedDocument> {
        let removed = self.documents.remove(uri);
        if removed.is_some() {
            tracing::info!(uri = %uri, "document closed");
        }
        removed
    }

    /// Returns a clone of the current snapshot for `uri`, if open.
    pub fn get(&self, uri: &Url) -> Option<TrackedDocument> {
        self.documents.get(uri).cloned()
    }

    /// Returns the number of currently open documents.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Returns `true` if no documents are open.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Detects the workspace root for `uri`, if its path yields one.
    ///
    /// A thin convenience over [`detect_workspace_root`] for callers holding a URI; it
    /// performs filesystem probes and so reflects the on-disk layout at call time.
    pub fn detect_root(&self, uri: &Url) -> Option<WorkspaceRoot> {
        let path = uri.to_file_path().ok()?;
        detect_workspace_root(&path)
    }
}

/// Classifies a document URI into a [`GradleFileKind`] using its detected root.
///
/// If the URI is not a file path, or no workspace root can be resolved, the file is
/// classified against its own parent directory as a best-effort root so a standalone
/// build script still gets a sensible kind instead of forcing `Unknown`.
fn classify_uri(uri: &Url) -> GradleFileKind {
    let Ok(path) = uri.to_file_path() else {
        return GradleFileKind::Unknown;
    };
    let root = detect_workspace_root(&path)
        .map(|r| r.path().to_path_buf())
        .or_else(|| path.parent().map(PathBuf::from))
        .unwrap_or_else(|| path.clone());
    GradleFileKind::classify(&path, &root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("ga-store-{}-{}-{}", tag, std::process::id(), unique));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file_uri(path: &std::path::Path) -> Url {
        Url::from_file_path(path).unwrap()
    }

    #[test]
    fn open_change_close_lifecycle_with_version_bump() {
        let root = temp_dir("lifecycle");
        fs::write(root.join("settings.gradle.kts"), b"").unwrap();
        let build = root.join("app/build.gradle.kts");
        fs::create_dir_all(build.parent().unwrap()).unwrap();
        fs::write(&build, b"plugins {}").unwrap();
        let uri = file_uri(&build);

        let mut store = WorkspaceDocumentStore::new();
        let opened = store.open(uri.clone(), 1, "plugins {}");
        assert_eq!(opened.version(), 1);
        assert_eq!(
            opened.kind(),
            GradleFileKind::SubprojectBuildScript(
                crate::gradle::workspace::DslLanguage::Kotlin
            )
        );
        assert_eq!(store.len(), 1);

        let changed = store.change(&uri, 2, "plugins { java }").unwrap();
        assert_eq!(changed.version(), 2);
        assert_eq!(store.get(&uri).unwrap().text(), "plugins { java }");

        let closed = store.close(&uri);
        assert!(closed.is_some());
        assert!(store.get(&uri).is_none());
        assert!(store.is_empty());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn old_snapshot_unchanged_after_change() {
        let uri = Url::from_file_path("/tmp/ga-x/build.gradle").unwrap();
        let mut store = WorkspaceDocumentStore::new();
        let captured = store.open(uri.clone(), 1, "old-text");

        store.change(&uri, 2, "new-text").unwrap();

        // The snapshot captured at open is a detached value: still version 1, old text.
        assert_eq!(captured.version(), 1);
        assert_eq!(captured.text(), "old-text");
        // The store now holds the new snapshot.
        assert_eq!(store.get(&uri).unwrap().version(), 2);
        assert_eq!(store.get(&uri).unwrap().text(), "new-text");
    }

    #[test]
    fn change_before_open_is_a_noop() {
        let uri = Url::from_file_path("/tmp/ga-y/build.gradle").unwrap();
        let mut store = WorkspaceDocumentStore::new();
        assert!(store.change(&uri, 5, "ignored").is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn detect_root_via_store_resolves_settings_dir() {
        let root = temp_dir("detect");
        fs::write(root.join("settings.gradle.kts"), b"").unwrap();
        let build = root.join("app/build.gradle.kts");
        fs::create_dir_all(build.parent().unwrap()).unwrap();
        fs::write(&build, b"").unwrap();
        let uri = file_uri(&build);

        let store = WorkspaceDocumentStore::new();
        let resolved = store.detect_root(&uri).expect("root");
        assert_eq!(resolved.path(), root.as_path());

        fs::remove_dir_all(&root).unwrap();
    }
}
