//! Workspace root detection: locate the directory that anchors a Gradle build.
//!
//! The root governs every later project-graph fact, so it is resolved once per document
//! and modeled as a small [`WorkspaceRoot`] value object. Detection prefers the nearest
//! ancestor holding a `settings.gradle*` (the canonical multi-project anchor) and only
//! falls back to a top-level `build.gradle*` for a settings-less single module. A nested
//! `build.gradle*` never becomes a root when a settings ancestor exists.

use std::path::{Path, PathBuf};

use crate::i18n::{MessageKey, Translator};
use crate::util::fs::nearest_ancestor;

/// The resolved anchor directory of a Gradle workspace.
///
/// A value object wrapping the root path plus HOW it was resolved, so callers can both
/// use the path and surface a localized status explaining the choice. Constructed only
/// through [`detect_workspace_root`].
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::{detect_workspace_root, RootResolution};
/// use std::path::Path;
///
/// // With no real files on disk, detection cannot find a marker and returns None.
/// assert!(detect_workspace_root(Path::new("/nonexistent/app/build.gradle.kts")).is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    path: PathBuf,
    resolution: RootResolution,
}

/// How a [`WorkspaceRoot`] was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootResolution {
    /// Resolved from the nearest ancestor containing a `settings.gradle*`.
    FromSettings,
    /// No settings script found; fell back to a top-level `build.gradle*` directory.
    FromBuildScript,
}

impl WorkspaceRoot {
    /// Returns the resolved root directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns how the root was resolved.
    pub fn resolution(&self) -> RootResolution {
        self.resolution
    }

    /// Renders a localized status line explaining how the root was resolved.
    ///
    /// User-facing text flows through the [`Translator`] so the status stays localizable;
    /// the root path is passed as the single template argument.
    pub fn status_message(&self, translator: &Translator) -> String {
        let key = match self.resolution {
            RootResolution::FromSettings => MessageKey::WorkspaceRootFromSettings,
            RootResolution::FromBuildScript => MessageKey::WorkspaceRootFromBuildScript,
        };
        translator.get_text(key, &[&self.path.to_string_lossy()])
    }
}

/// Detects the workspace root for `file_path`, or `None` if no Gradle marker is found.
///
/// Strategy, in order:
/// 1. the NEAREST ancestor directory containing `settings.gradle`/`settings.gradle.kts`
///    wins (so a subproject's settings ancestor dominates its own `build.gradle*`);
/// 2. otherwise the nearest ancestor containing a `build.gradle`/`build.gradle.kts`
///    (a settings-less single module).
///
/// This performs real filesystem probes (`Path::exists`) on candidate ancestor dirs,
/// and emits a `tracing` record of the chosen strategy and path.
pub fn detect_workspace_root(file_path: &Path) -> Option<WorkspaceRoot> {
    let start_dir = starting_dir(file_path);

    if let Some(path) = nearest_ancestor(start_dir, dir_has_settings) {
        tracing::info!(root = %path.display(), strategy = "settings", "workspace root resolved");
        return Some(WorkspaceRoot {
            path,
            resolution: RootResolution::FromSettings,
        });
    }

    if let Some(path) = nearest_ancestor(start_dir, dir_has_build_script) {
        tracing::info!(root = %path.display(), strategy = "build_script", "workspace root resolved");
        return Some(WorkspaceRoot {
            path,
            resolution: RootResolution::FromBuildScript,
        });
    }

    tracing::warn!(file = %file_path.display(), "no workspace root marker found");
    None
}

/// Returns the directory to start the ancestor scan from for `file_path`.
///
/// A file path scans from its parent; a path with no parent scans from itself.
fn starting_dir(file_path: &Path) -> &Path {
    file_path.parent().unwrap_or(file_path)
}

/// Returns `true` if `dir` directly contains a `settings.gradle*` script.
fn dir_has_settings(dir: &Path) -> bool {
    dir.join("settings.gradle").exists() || dir.join("settings.gradle.kts").exists()
}

/// Returns `true` if `dir` directly contains a `build.gradle*` script.
fn dir_has_build_script(dir: &Path) -> bool {
    dir.join("build.gradle").exists() || dir.join("build.gradle.kts").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Creates a unique temp directory for one test, returned for explicit teardown.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ga-root-{}-{}-{}",
            tag,
            std::process::id(),
            fastish_unique()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A cheap per-call unique suffix so parallel tests never collide on a path.
    fn fastish_unique() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn nested_subproject_build_resolves_to_settings_ancestor_not_itself() {
        let root = temp_dir("nested");
        touch(&root.join("settings.gradle.kts"));
        let sub_build = root.join("app/build.gradle.kts");
        touch(&sub_build);

        let resolved = detect_workspace_root(&sub_build).expect("root found");
        // The settings ancestor (root) wins; the subproject dir does NOT become a root.
        assert_eq!(resolved.path(), root.as_path());
        assert_ne!(resolved.path(), root.join("app").as_path());
        assert_eq!(resolved.resolution(), RootResolution::FromSettings);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn settingsless_single_module_falls_back_to_build_script_dir() {
        let root = temp_dir("single");
        let build = root.join("build.gradle");
        touch(&build);

        let resolved = detect_workspace_root(&build).expect("root found");
        assert_eq!(resolved.path(), root.as_path());
        assert_eq!(resolved.resolution(), RootResolution::FromBuildScript);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn deeper_settings_ancestor_beats_a_closer_build_script() {
        let root = temp_dir("deep");
        touch(&root.join("settings.gradle"));
        // A subproject that ALSO has its own build script must still resolve to the
        // settings root, proving settings detection takes precedence over a nearer build.
        let sub = root.join("services/api");
        touch(&sub.join("build.gradle"));

        let resolved = detect_workspace_root(&sub.join("build.gradle")).expect("root found");
        assert_eq!(resolved.path(), root.as_path());
        assert_eq!(resolved.resolution(), RootResolution::FromSettings);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_marker_anywhere_returns_none() {
        let root = temp_dir("bare");
        let orphan = root.join("notes/README.md");
        touch(&orphan);

        assert!(detect_workspace_root(&orphan).is_none());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn status_message_is_localized_and_includes_path() {
        let root = temp_dir("status");
        touch(&root.join("settings.gradle.kts"));
        let resolved = detect_workspace_root(&root.join("settings.gradle.kts")).unwrap();

        let translator = Translator::new();
        let message = resolved.status_message(&translator);
        assert!(message.contains("settings script"));
        assert!(message.contains(&*root.to_string_lossy()));

        fs::remove_dir_all(&root).unwrap();
    }
}
