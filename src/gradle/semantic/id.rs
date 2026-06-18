//! Stable, deterministic semantic identifiers.
//!
//! Every extracted fact is addressed by a [`SemanticId`] built from two parts: the
//! workspace-relative [`DocumentId`] of the file it came from, and a per-fact path segment
//! `"<kind>:<key>"`. Identical segments under one document are disambiguated by a
//! first-seen `#2`/`#3` suffix via [`IdAllocator`], so the same input always yields the same
//! ids (the stale-state guarantee Tasks 9-13/16 rely on for cross-analysis stability).
//!
//! The scheme is intentionally content-free and path-like (no hashes, no addresses): an id
//! is greppable and human-readable, e.g. `app/build.gradle.kts::dependency:implementation/libs.guava`.

use std::collections::HashMap;
use std::path::Path;

/// A workspace-relative document identity used as the root of every [`SemanticId`].
///
/// Stored as a forward-slash path so ids are stable across operating systems and easy to
/// compare in golden tests. A document outside the workspace root falls back to its file
/// name, keeping the id non-empty and deterministic.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::DocumentId;
/// use std::path::Path;
///
/// let id = DocumentId::from_relative_path(Path::new("/proj"), Path::new("/proj/app/build.gradle.kts"));
/// assert_eq!(id.as_str(), "app/build.gradle.kts");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(String);

impl DocumentId {
    /// Builds a document id directly from an already-relative slash path.
    pub fn new(relative: impl Into<String>) -> DocumentId {
        DocumentId(relative.into())
    }

    /// Builds a document id from `path` made relative to the workspace `root`.
    ///
    /// Backslashes are normalized to forward slashes. If `path` is not under `root`, the
    /// file name is used so the id is still non-empty and deterministic.
    pub fn from_relative_path(root: &Path, path: &Path) -> DocumentId {
        let relative = path
            .strip_prefix(root)
            .ok()
            .map(Path::to_path_buf)
            .or_else(|| path.file_name().map(Into::into))
            .unwrap_or_else(|| path.to_path_buf());
        DocumentId(relative.to_string_lossy().replace('\\', "/"))
    }

    /// Returns the underlying relative slash path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A fully-qualified, stable identity for one extracted fact.
///
/// Formats as `"<document>::<kind>:<key>"` (plus a possible `#n` duplicate suffix in the
/// key part). The string form is the canonical identity Tasks 9-13/16 store and compare.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{DocumentId, SemanticId};
///
/// let doc = DocumentId::new("build.gradle.kts");
/// let id = SemanticId::new(&doc, "plugin", "java");
/// assert_eq!(id.as_str(), "build.gradle.kts::plugin:java");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SemanticId(String);

impl SemanticId {
    /// Builds an id from a document, a fact-kind tag, and a key (no duplicate suffix).
    pub fn new(document: &DocumentId, kind: &str, key: &str) -> SemanticId {
        SemanticId(format!("{}::{}:{}", document.as_str(), kind, key))
    }

    /// Wraps an already-formatted id string (used by the allocator after suffixing).
    pub(crate) fn from_raw(raw: String) -> SemanticId {
        SemanticId(raw)
    }

    /// Returns the canonical id string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SemanticId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Hands out [`SemanticId`]s, appending deterministic `#2`/`#3` suffixes to duplicates.
///
/// The first time a `"<kind>:<key>"` segment is seen for a document it is used verbatim;
/// each later identical segment gets the next `#n` (n starting at 2) in first-seen order.
/// Because allocation order is driven purely by tree-walk order over immutable input, the
/// same source always produces the same ids.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{DocumentId, IdAllocator};
///
/// let doc = DocumentId::new("build.gradle");
/// let mut alloc = IdAllocator::new(doc);
/// assert_eq!(alloc.allocate("repository", "mavenCentral").as_str(), "build.gradle::repository:mavenCentral");
/// assert_eq!(alloc.allocate("repository", "mavenCentral").as_str(), "build.gradle::repository:mavenCentral#2");
/// assert_eq!(alloc.allocate("repository", "mavenCentral").as_str(), "build.gradle::repository:mavenCentral#3");
/// ```
#[derive(Debug)]
pub struct IdAllocator {
    document: DocumentId,
    seen: HashMap<String, u32>,
}

impl IdAllocator {
    /// Creates an allocator scoped to one document.
    pub fn new(document: DocumentId) -> IdAllocator {
        IdAllocator {
            document,
            seen: HashMap::new(),
        }
    }

    /// Returns the document this allocator is scoped to.
    pub fn document(&self) -> &DocumentId {
        &self.document
    }

    /// Allocates the next id for a `kind`/`key` segment, suffixing repeats with `#n`.
    pub fn allocate(&mut self, kind: &str, key: &str) -> SemanticId {
        let base = format!("{}::{}:{}", self.document.as_str(), kind, key);
        let count = self.seen.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            SemanticId::from_raw(base)
        } else {
            SemanticId::from_raw(format!("{base}#{count}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn document_id_is_workspace_relative_with_forward_slashes() {
        let root = PathBuf::from("/proj");
        let id = DocumentId::from_relative_path(&root, &root.join("app/build.gradle.kts"));
        assert_eq!(id.as_str(), "app/build.gradle.kts");
    }

    #[test]
    fn document_id_outside_root_falls_back_to_file_name() {
        let id = DocumentId::from_relative_path(Path::new("/proj"), Path::new("/other/settings.gradle"));
        assert_eq!(id.as_str(), "settings.gradle");
    }

    #[test]
    fn duplicate_segments_get_deterministic_suffixes() {
        let mut alloc = IdAllocator::new(DocumentId::new("build.gradle"));
        let a = alloc.allocate("dependency", "implementation/x");
        let b = alloc.allocate("dependency", "implementation/x");
        let c = alloc.allocate("dependency", "implementation/x");
        assert_eq!(a.as_str(), "build.gradle::dependency:implementation/x");
        assert_eq!(b.as_str(), "build.gradle::dependency:implementation/x#2");
        assert_eq!(c.as_str(), "build.gradle::dependency:implementation/x#3");
    }

    #[test]
    fn distinct_keys_never_collide() {
        let mut alloc = IdAllocator::new(DocumentId::new("build.gradle"));
        let a = alloc.allocate("plugin", "java");
        let b = alloc.allocate("plugin", "application");
        assert_eq!(a.as_str(), "build.gradle::plugin:java");
        assert_eq!(b.as_str(), "build.gradle::plugin:application");
    }

    #[test]
    fn allocation_is_stable_across_identical_runs() {
        let run = || {
            let mut alloc = IdAllocator::new(DocumentId::new("d"));
            vec![
                alloc.allocate("repository", "mavenCentral").as_str().to_string(),
                alloc.allocate("repository", "mavenCentral").as_str().to_string(),
                alloc.allocate("plugin", "java").as_str().to_string(),
            ]
        };
        assert_eq!(run(), run());
    }
}
