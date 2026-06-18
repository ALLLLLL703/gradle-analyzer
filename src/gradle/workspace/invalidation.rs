//! The invalidation contract: how a change maps to the work it forces.
//!
//! A pure, parser/sidecar-agnostic mapping from a [`ChangeTrigger`] to an
//! [`InvalidationScope`]. Keeping it a free function of plain value types means the
//! "what must I recompute?" policy is unit-testable in isolation and has exactly one
//! home, instead of being re-derived at each call site.

use std::path::Path;

use crate::gradle::workspace::kind::GradleFileKind;

/// What kind of change occurred, as far as invalidation is concerned.
///
/// This is intentionally a SUPERSET of [`GradleFileKind`]: wrapper files
/// (`gradle/wrapper/*`, `gradle.properties`) are not a Gradle script kind yet still
/// drive invalidation, so they get their own trigger. [`ChangeTrigger::for_path`]
/// classifies any workspace path, while [`ChangeTrigger::from_kind`] bridges an
/// already-classified document.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::ChangeTrigger;
/// use std::path::Path;
///
/// let root = Path::new("/proj");
/// let trigger = ChangeTrigger::for_path(Path::new("/proj/gradle.properties"), root);
/// assert_eq!(trigger, ChangeTrigger::WrapperEdit);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeTrigger {
    /// A project build script (`build.gradle*`, not under `buildSrc`) changed.
    BuildScriptEdit,
    /// A `settings.gradle*` script changed (the project graph may have changed).
    SettingsEdit,
    /// A version catalog (`*.versions.toml`) changed (affects every project).
    VersionCatalogEdit,
    /// Something under `buildSrc/` changed (build logic affecting the whole build).
    BuildSrcEdit,
    /// A wrapper/build-environment file changed (`gradle/wrapper/*`, `gradle.properties`).
    WrapperEdit,
    /// A change with no Gradle-relevant invalidation effect.
    Other,
}

impl ChangeTrigger {
    /// Maps an already-classified [`GradleFileKind`] to its trigger.
    ///
    /// Build scripts collapse to [`ChangeTrigger::BuildScriptEdit`] regardless of root
    /// vs subproject, because the invalidation tier is the same for both (a single
    /// build script edit is local); the root-vs-subproject distinction matters to
    /// classification, not to this contract.
    pub fn from_kind(kind: GradleFileKind) -> ChangeTrigger {
        match kind {
            GradleFileKind::RootBuildScript(_) | GradleFileKind::SubprojectBuildScript(_) => {
                ChangeTrigger::BuildScriptEdit
            }
            GradleFileKind::SettingsScript(_) => ChangeTrigger::SettingsEdit,
            GradleFileKind::VersionCatalog => ChangeTrigger::VersionCatalogEdit,
            GradleFileKind::BuildSrcScript(_) => ChangeTrigger::BuildSrcEdit,
            GradleFileKind::Unknown => ChangeTrigger::Other,
        }
    }

    /// Classifies any workspace `path` (relative to `root`) into a trigger.
    ///
    /// Wrapper files are recognized first because they are NOT a [`GradleFileKind`];
    /// everything else defers to [`GradleFileKind::classify`] then [`from_kind`].
    pub fn for_path(path: &Path, root: &Path) -> ChangeTrigger {
        if is_wrapper_file(path, root) {
            return ChangeTrigger::WrapperEdit;
        }
        ChangeTrigger::from_kind(GradleFileKind::classify(path, root))
    }
}

/// The named, plan-level outcome of a change.
///
/// These are the three outcomes the invalidation contract must distinguish so callers
/// (and tests) can assert the exact policy, rather than inferring it from booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidationKind {
    /// Only the edited file needs reparsing; no cross-project semantic work.
    FileOnlyReparse,
    /// Workspace-wide semantic state must be recomputed (graph/catalog changed).
    WorkspaceSemantic,
    /// The JVM sidecar model must be refreshed (build environment changed).
    SidecarRefreshNeeded,
}

/// The resolved scope of work a change forces.
///
/// Composed of a [`InvalidationKind`] plus an explicit `sidecar_refresh` flag, because a
/// `buildSrc` edit is BOTH workspace-semantic AND requires a sidecar refresh — a single
/// enum could not capture the combination without losing the primary outcome. [`kind`]
/// returns the dominant named outcome for assertions; [`needs_sidecar_refresh`] and
/// [`needs_workspace_semantic`] expose the underlying flags.
///
/// [`kind`]: InvalidationScope::kind
/// [`needs_sidecar_refresh`]: InvalidationScope::needs_sidecar_refresh
/// [`needs_workspace_semantic`]: InvalidationScope::needs_workspace_semantic
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::{ChangeTrigger, InvalidationKind, invalidation_for};
///
/// let scope = invalidation_for(ChangeTrigger::BuildSrcEdit);
/// assert_eq!(scope.kind(), InvalidationKind::WorkspaceSemantic);
/// assert!(scope.needs_sidecar_refresh());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidationScope {
    workspace_semantic: bool,
    sidecar_refresh: bool,
}

impl InvalidationScope {
    /// Returns the dominant named outcome.
    ///
    /// Sidecar refresh without a semantic change is reported as
    /// [`InvalidationKind::SidecarRefreshNeeded`]; a semantic change (whether or not it
    /// also refreshes the sidecar) is [`InvalidationKind::WorkspaceSemantic`]; otherwise
    /// the change is [`InvalidationKind::FileOnlyReparse`].
    pub fn kind(self) -> InvalidationKind {
        if self.workspace_semantic {
            InvalidationKind::WorkspaceSemantic
        } else if self.sidecar_refresh {
            InvalidationKind::SidecarRefreshNeeded
        } else {
            InvalidationKind::FileOnlyReparse
        }
    }

    /// Returns `true` if workspace-wide semantic state must be recomputed.
    pub fn needs_workspace_semantic(self) -> bool {
        self.workspace_semantic
    }

    /// Returns `true` if the JVM sidecar model must be refreshed.
    pub fn needs_sidecar_refresh(self) -> bool {
        self.sidecar_refresh
    }
}

/// Maps a [`ChangeTrigger`] to its [`InvalidationScope`] — the pure invalidation contract.
///
/// - build script (subproject/root): file-only reparse, local semantics only;
/// - settings: workspace-semantic (the project graph may have changed);
/// - version catalog: workspace-semantic (a catalog change affects every project);
/// - buildSrc: workspace-semantic AND a sidecar refresh (build logic changed);
/// - wrapper: sidecar refresh only (build environment changed, no source semantics);
/// - other: file-only reparse (the safe minimum).
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::{ChangeTrigger, InvalidationKind, invalidation_for};
///
/// assert_eq!(
///     invalidation_for(ChangeTrigger::SettingsEdit).kind(),
///     InvalidationKind::WorkspaceSemantic
/// );
/// assert_eq!(
///     invalidation_for(ChangeTrigger::WrapperEdit).kind(),
///     InvalidationKind::SidecarRefreshNeeded
/// );
/// ```
pub fn invalidation_for(trigger: ChangeTrigger) -> InvalidationScope {
    match trigger {
        ChangeTrigger::BuildScriptEdit | ChangeTrigger::Other => InvalidationScope {
            workspace_semantic: false,
            sidecar_refresh: false,
        },
        ChangeTrigger::SettingsEdit | ChangeTrigger::VersionCatalogEdit => InvalidationScope {
            workspace_semantic: true,
            sidecar_refresh: false,
        },
        ChangeTrigger::BuildSrcEdit => InvalidationScope {
            workspace_semantic: true,
            sidecar_refresh: true,
        },
        ChangeTrigger::WrapperEdit => InvalidationScope {
            workspace_semantic: false,
            sidecar_refresh: true,
        },
    }
}

/// Returns `true` if `path` is a Gradle wrapper / build-environment file.
///
/// Recognizes `gradle.properties` at any level and any file under a `gradle/wrapper`
/// directory (e.g. `gradle-wrapper.properties`, `gradle-wrapper.jar`).
fn is_wrapper_file(path: &Path, root: &Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()) == Some("gradle.properties") {
        return true;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    let components: Vec<_> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    components
        .windows(2)
        .any(|w| w[0] == "gradle" && w[1] == "wrapper")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradle::workspace::DslLanguage;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/proj")
    }

    #[test]
    fn settings_edit_is_workspace_semantic() {
        let scope = invalidation_for(ChangeTrigger::SettingsEdit);
        assert_eq!(scope.kind(), InvalidationKind::WorkspaceSemantic);
        assert!(scope.needs_workspace_semantic());
        assert!(!scope.needs_sidecar_refresh());
    }

    #[test]
    fn version_catalog_edit_is_workspace_semantic() {
        let scope = invalidation_for(ChangeTrigger::VersionCatalogEdit);
        assert_eq!(scope.kind(), InvalidationKind::WorkspaceSemantic);
        assert!(!scope.needs_sidecar_refresh());
    }

    #[test]
    fn build_src_edit_is_workspace_semantic_plus_sidecar() {
        let scope = invalidation_for(ChangeTrigger::BuildSrcEdit);
        assert_eq!(scope.kind(), InvalidationKind::WorkspaceSemantic);
        assert!(scope.needs_workspace_semantic());
        assert!(scope.needs_sidecar_refresh());
    }

    #[test]
    fn plain_subproject_build_edit_is_file_only_reparse() {
        let scope = invalidation_for(ChangeTrigger::BuildScriptEdit);
        assert_eq!(scope.kind(), InvalidationKind::FileOnlyReparse);
        assert!(!scope.needs_workspace_semantic());
        assert!(!scope.needs_sidecar_refresh());
    }

    #[test]
    fn wrapper_edit_is_sidecar_refresh_only() {
        let scope = invalidation_for(ChangeTrigger::WrapperEdit);
        assert_eq!(scope.kind(), InvalidationKind::SidecarRefreshNeeded);
        assert!(!scope.needs_workspace_semantic());
        assert!(scope.needs_sidecar_refresh());
    }

    #[test]
    fn for_path_recognizes_wrapper_files() {
        assert_eq!(
            ChangeTrigger::for_path(&root().join("gradle.properties"), &root()),
            ChangeTrigger::WrapperEdit
        );
        assert_eq!(
            ChangeTrigger::for_path(
                &root().join("gradle/wrapper/gradle-wrapper.properties"),
                &root()
            ),
            ChangeTrigger::WrapperEdit
        );
    }

    #[test]
    fn for_path_bridges_to_kind_classification() {
        assert_eq!(
            ChangeTrigger::for_path(&root().join("settings.gradle.kts"), &root()),
            ChangeTrigger::SettingsEdit
        );
        assert_eq!(
            ChangeTrigger::for_path(&root().join("app/build.gradle.kts"), &root()),
            ChangeTrigger::BuildScriptEdit
        );
        assert_eq!(
            ChangeTrigger::for_path(&root().join("buildSrc/build.gradle.kts"), &root()),
            ChangeTrigger::BuildSrcEdit
        );
        assert_eq!(
            ChangeTrigger::for_path(&root().join("gradle/libs.versions.toml"), &root()),
            ChangeTrigger::VersionCatalogEdit
        );
    }

    #[test]
    fn from_kind_collapses_root_and_subproject_builds() {
        assert_eq!(
            ChangeTrigger::from_kind(GradleFileKind::RootBuildScript(DslLanguage::Kotlin)),
            ChangeTrigger::BuildScriptEdit
        );
        assert_eq!(
            ChangeTrigger::from_kind(GradleFileKind::SubprojectBuildScript(DslLanguage::Groovy)),
            ChangeTrigger::BuildScriptEdit
        );
        assert_eq!(
            ChangeTrigger::from_kind(GradleFileKind::Unknown),
            ChangeTrigger::Other
        );
    }
}
