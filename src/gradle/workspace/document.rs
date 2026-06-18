//! [`TrackedDocument`]: an immutable snapshot of one open document.
//!
//! Each document the editor opens is held as an immutable snapshot — its text behind an
//! [`Arc<str>`], plus a version and a classified [`GradleFileKind`]. Mutation is modeled
//! by REPLACING the whole snapshot (see [`crate::gradle::workspace::WorkspaceDocumentStore`]),
//! so a previously captured snapshot keeps observing its old text and version even after
//! the document changes. This is the stale-state guarantee analyses rely on.

use std::sync::Arc;

use tower_lsp::lsp_types::Url;

use crate::gradle::workspace::kind::GradleFileKind;

/// An immutable snapshot of a tracked document at one version.
///
/// Cloning is cheap (an [`Arc`] bump on the text plus small `Copy`/clone fields) and a
/// clone is fully detached from later mutations: the store swaps in a NEW
/// `TrackedDocument` on change rather than mutating in place, so an old clone is never
/// affected. The type is deliberately parser- and sidecar-agnostic — it carries text,
/// identity, version, and role, nothing analysis-derived.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
/// use tower_lsp::lsp_types::Url;
///
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
/// let doc = TrackedDocument::new(
///     uri,
///     1,
///     "plugins {}",
///     GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
/// );
/// assert_eq!(doc.version(), 1);
/// assert_eq!(doc.text(), "plugins {}");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedDocument {
    uri: Url,
    version: i32,
    text: Arc<str>,
    kind: GradleFileKind,
}

impl TrackedDocument {
    /// Creates a snapshot from `uri`, `version`, `text`, and a classified `kind`.
    pub fn new(
        uri: Url,
        version: i32,
        text: impl Into<Arc<str>>,
        kind: GradleFileKind,
    ) -> TrackedDocument {
        TrackedDocument {
            uri,
            version,
            text: text.into(),
            kind,
        }
    }

    /// Returns the document URI.
    pub fn uri(&self) -> &Url {
        &self.uri
    }

    /// Returns the document version (the editor-assigned monotonically increasing id).
    pub fn version(&self) -> i32 {
        self.version
    }

    /// Returns the snapshot text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns a cheap [`Arc<str>`] handle to the snapshot text.
    ///
    /// Holding this handle keeps THIS snapshot's text alive and unchanged regardless of
    /// later edits, which is exactly what a background analysis wants to capture.
    pub fn text_arc(&self) -> Arc<str> {
        Arc::clone(&self.text)
    }

    /// Returns the classified workspace role of this document.
    pub fn kind(&self) -> GradleFileKind {
        self.kind
    }

    /// Returns a new snapshot with replaced `text` at `version`, keeping uri and kind.
    ///
    /// This is the full-text-sync primitive: the store calls it to produce the next
    /// snapshot, leaving `self` (and any clone of it) untouched.
    pub fn with_change(&self, version: i32, text: impl Into<Arc<str>>) -> TrackedDocument {
        TrackedDocument {
            uri: self.uri.clone(),
            version,
            text: text.into(),
            kind: self.kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradle::workspace::DslLanguage;

    fn uri() -> Url {
        Url::from_file_path("/proj/app/build.gradle.kts").unwrap()
    }

    fn doc(version: i32, text: &str) -> TrackedDocument {
        TrackedDocument::new(
            uri(),
            version,
            text,
            GradleFileKind::SubprojectBuildScript(DslLanguage::Kotlin),
        )
    }

    #[test]
    fn exposes_uri_version_text_kind() {
        let d = doc(3, "dependencies {}");
        assert_eq!(d.uri(), &uri());
        assert_eq!(d.version(), 3);
        assert_eq!(d.text(), "dependencies {}");
        assert_eq!(
            d.kind(),
            GradleFileKind::SubprojectBuildScript(DslLanguage::Kotlin)
        );
    }

    #[test]
    fn with_change_produces_new_snapshot_and_leaves_old_unchanged() {
        let first = doc(1, "old");
        let captured = first.clone();
        let second = first.with_change(2, "new");

        assert_eq!(second.version(), 2);
        assert_eq!(second.text(), "new");
        // The captured/old snapshot is unaffected by producing a newer one.
        assert_eq!(captured.version(), 1);
        assert_eq!(captured.text(), "old");
    }

    #[test]
    fn text_arc_handle_outlives_a_later_change() {
        let first = doc(1, "stable");
        let held = first.text_arc();
        let _next = first.with_change(2, "changed");
        // The Arc captured from the first snapshot still reads the first text.
        assert_eq!(&*held, "stable");
    }
}
