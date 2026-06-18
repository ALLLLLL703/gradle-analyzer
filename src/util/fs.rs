//! Filesystem path helpers reused across feature domains.
//!
//! Centralizes generic path-walking utilities so no feature module reimplements an
//! ancestor scan. The helpers are intentionally domain-agnostic: they take a predicate
//! and know nothing about Gradle, configuration, or any specific file convention.

use std::path::{Path, PathBuf};

/// Returns the nearest ancestor directory of `start` (inclusive) for which `accept`
/// returns `true`, or `None` if no ancestor matches.
///
/// The search begins at `start` itself and walks toward the filesystem root, so the
/// CLOSEST matching ancestor wins. `start` should be a directory; callers holding a
/// file path pass its parent. The predicate decides what "matches" means (e.g. a
/// directory that contains a marker file), keeping this helper free of any one domain.
///
/// # Example
///
/// ```
/// use gradle_analyzer::util::fs::nearest_ancestor;
/// use std::path::Path;
///
/// // The nearest ancestor whose final component is named "b".
/// let found = nearest_ancestor(Path::new("/a/b/c"), |dir| {
///     dir.file_name().map(|n| n == "b").unwrap_or(false)
/// });
/// assert_eq!(found.as_deref(), Some(Path::new("/a/b")));
/// ```
pub fn nearest_ancestor<F>(start: &Path, mut accept: F) -> Option<PathBuf>
where
    F: FnMut(&Path) -> bool,
{
    for ancestor in start.ancestors() {
        if accept(ancestor) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_start_itself_when_it_matches() {
        let found = nearest_ancestor(Path::new("/x/y/z"), |dir| dir.ends_with("z"));
        assert_eq!(found, Some(PathBuf::from("/x/y/z")));
    }

    #[test]
    fn walks_upward_and_picks_closest_match() {
        // Both /m and /m/n end with a single component; the CLOSEST (n) must win when
        // the predicate accepts either, proving the scan is nearest-first.
        let found = nearest_ancestor(Path::new("/m/n/o"), |dir| {
            dir.ends_with("n") || dir.ends_with("m")
        });
        assert_eq!(found, Some(PathBuf::from("/m/n")));
    }

    #[test]
    fn returns_none_when_no_ancestor_matches() {
        let found = nearest_ancestor(Path::new("/p/q"), |_| false);
        assert_eq!(found, None);
    }
}
