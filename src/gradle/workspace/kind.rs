//! [`GradleFileKind`] classification: what role a path plays in a Gradle workspace.
//!
//! Classifying a file's IDENTITY before parsing keeps role decisions in one pure
//! function instead of scattered `path.ends_with(...)` checks across diagnostics,
//! completion, and the parser. The classifier is side-effect-free (it never touches the
//! filesystem for the classified path) so it is trivially unit-testable on synthetic
//! paths, and it is parser/sidecar-agnostic.

use std::path::Path;

/// Which build DSL a Gradle script is written in.
///
/// Distinguishes Groovy (`*.gradle`) from Kotlin (`*.gradle.kts`) scripts, which later
/// tasks route to different frontends. A version catalog is plain TOML and therefore
/// carries no DSL.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::DslLanguage;
/// use std::path::Path;
///
/// assert_eq!(DslLanguage::of_script(Path::new("build.gradle")), DslLanguage::Groovy);
/// assert_eq!(DslLanguage::of_script(Path::new("build.gradle.kts")), DslLanguage::Kotlin);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DslLanguage {
    /// Groovy DSL (`*.gradle`).
    Groovy,
    /// Kotlin DSL (`*.gradle.kts`).
    Kotlin,
}

impl DslLanguage {
    /// Returns the DSL implied by a script file name, defaulting to Groovy.
    ///
    /// A `.kts` suffix selects Kotlin; anything else (including a bare `.gradle`) is
    /// treated as Groovy, matching Gradle's own naming convention.
    pub fn of_script(path: &Path) -> DslLanguage {
        match path.extension().and_then(|e| e.to_str()) {
            Some("kts") => DslLanguage::Kotlin,
            _ => DslLanguage::Groovy,
        }
    }
}

/// The role a file plays in a Gradle workspace.
///
/// This is a value object: a pure classification of a path, independent of document
/// text, parser output, or sidecar facts. The DSL-bearing variants distinguish Groovy
/// from Kotlin where it matters; [`GradleFileKind::VersionCatalog`] is TOML (no DSL) and
/// [`GradleFileKind::Unknown`] is the explicit fallback for unrelated/temporary files so
/// callers never need an implicit "else" branch.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
/// use std::path::Path;
///
/// let root = Path::new("/proj");
/// let kind = GradleFileKind::classify(Path::new("/proj/app/build.gradle.kts"), root);
/// assert_eq!(kind, GradleFileKind::SubprojectBuildScript(DslLanguage::Kotlin));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GradleFileKind {
    /// The workspace's top-level `build.gradle*` (directly under the root).
    RootBuildScript(DslLanguage),
    /// A `settings.gradle*` script (workspace structure / included builds).
    SettingsScript(DslLanguage),
    /// A nested project's `build.gradle*` (below the root, not under `buildSrc`).
    SubprojectBuildScript(DslLanguage),
    /// A build/settings script under `buildSrc/` (the included build for build logic).
    BuildSrcScript(DslLanguage),
    /// A Gradle version catalog (`gradle/libs.versions.toml` or any `*.versions.toml`).
    VersionCatalog,
    /// A file with no recognized Gradle role.
    Unknown,
}

impl GradleFileKind {
    /// Classifies `path` relative to the resolved workspace `root` (a pure function).
    ///
    /// Precedence (first match wins): version catalog, then a `buildSrc` script, then a
    /// settings script, then a build script split into root vs subproject by whether its
    /// parent directory IS the workspace root. A nested `build.gradle*` therefore becomes
    /// a [`GradleFileKind::SubprojectBuildScript`], never a root. Anything unrecognized is
    /// [`GradleFileKind::Unknown`]. No filesystem access is performed on `path`.
    pub fn classify(path: &Path, root: &Path) -> GradleFileKind {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            return GradleFileKind::Unknown;
        };

        if is_version_catalog(file_name) {
            return GradleFileKind::VersionCatalog;
        }

        let script_role = ScriptName::parse(file_name);

        if is_under_build_src(path, root) {
            return match script_role {
                Some(role) => GradleFileKind::BuildSrcScript(role.dsl),
                None => GradleFileKind::Unknown,
            };
        }

        let Some(role) = script_role else {
            return GradleFileKind::Unknown;
        };

        match role.flavor {
            ScriptFlavor::Settings => GradleFileKind::SettingsScript(role.dsl),
            ScriptFlavor::Build => {
                if parent_is_root(path, root) {
                    GradleFileKind::RootBuildScript(role.dsl)
                } else {
                    GradleFileKind::SubprojectBuildScript(role.dsl)
                }
            }
        }
    }

    /// Returns the DSL of this kind, or `None` for a version catalog or unknown file.
    pub fn dsl(self) -> Option<DslLanguage> {
        match self {
            GradleFileKind::RootBuildScript(d)
            | GradleFileKind::SettingsScript(d)
            | GradleFileKind::SubprojectBuildScript(d)
            | GradleFileKind::BuildSrcScript(d) => Some(d),
            GradleFileKind::VersionCatalog | GradleFileKind::Unknown => None,
        }
    }

    /// Returns `true` if this kind is a Gradle file the analyzer should process.
    ///
    /// Everything except [`GradleFileKind::Unknown`] is in scope; callers use this to
    /// skip unrelated documents an editor may have opened without a separate branch.
    pub fn is_recognized(self) -> bool {
        !matches!(self, GradleFileKind::Unknown)
    }
}

/// Whether a build flavor is a settings script or a project build script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptFlavor {
    Settings,
    Build,
}

/// A parsed Gradle script file name: its flavor plus its DSL.
struct ScriptName {
    flavor: ScriptFlavor,
    dsl: DslLanguage,
}

impl ScriptName {
    /// Parses a Gradle build/settings script file name, or `None` if it is neither.
    fn parse(file_name: &str) -> Option<ScriptName> {
        match file_name {
            "settings.gradle" => Some(ScriptName {
                flavor: ScriptFlavor::Settings,
                dsl: DslLanguage::Groovy,
            }),
            "settings.gradle.kts" => Some(ScriptName {
                flavor: ScriptFlavor::Settings,
                dsl: DslLanguage::Kotlin,
            }),
            "build.gradle" => Some(ScriptName {
                flavor: ScriptFlavor::Build,
                dsl: DslLanguage::Groovy,
            }),
            "build.gradle.kts" => Some(ScriptName {
                flavor: ScriptFlavor::Build,
                dsl: DslLanguage::Kotlin,
            }),
            _ => None,
        }
    }
}

/// Returns `true` for a Gradle version-catalog file name.
///
/// Matches the canonical `libs.versions.toml` plus any `*.versions.toml` so custom
/// catalogs are recognized too.
fn is_version_catalog(file_name: &str) -> bool {
    file_name == "libs.versions.toml" || file_name.ends_with(".versions.toml")
}

/// Returns `true` if `path` lies within a `buildSrc` directory at or below `root`.
///
/// Only components from `root` downward are considered, so a stray `buildSrc` segment
/// in the absolute prefix above the root never triggers a false positive.
fn is_under_build_src(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .any(|c| c.as_os_str() == "buildSrc")
}

/// Returns `true` if `path`'s parent directory IS the workspace root.
fn parent_is_root(path: &Path, root: &Path) -> bool {
    path.parent().map(|p| p == root).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/proj")
    }

    #[test]
    fn root_build_script_groovy_and_kotlin() {
        assert_eq!(
            GradleFileKind::classify(&root().join("build.gradle"), &root()),
            GradleFileKind::RootBuildScript(DslLanguage::Groovy)
        );
        assert_eq!(
            GradleFileKind::classify(&root().join("build.gradle.kts"), &root()),
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin)
        );
    }

    #[test]
    fn settings_script_groovy_and_kotlin() {
        assert_eq!(
            GradleFileKind::classify(&root().join("settings.gradle"), &root()),
            GradleFileKind::SettingsScript(DslLanguage::Groovy)
        );
        assert_eq!(
            GradleFileKind::classify(&root().join("settings.gradle.kts"), &root()),
            GradleFileKind::SettingsScript(DslLanguage::Kotlin)
        );
    }

    #[test]
    fn nested_build_script_is_subproject_not_root() {
        assert_eq!(
            GradleFileKind::classify(&root().join("app/build.gradle.kts"), &root()),
            GradleFileKind::SubprojectBuildScript(DslLanguage::Kotlin)
        );
        assert_eq!(
            GradleFileKind::classify(&root().join("core/lib/build.gradle"), &root()),
            GradleFileKind::SubprojectBuildScript(DslLanguage::Groovy)
        );
    }

    #[test]
    fn version_catalog_canonical_and_custom() {
        assert_eq!(
            GradleFileKind::classify(&root().join("gradle/libs.versions.toml"), &root()),
            GradleFileKind::VersionCatalog
        );
        assert_eq!(
            GradleFileKind::classify(&root().join("gradle/tools.versions.toml"), &root()),
            GradleFileKind::VersionCatalog
        );
    }

    #[test]
    fn build_src_scripts_classified_as_build_src_even_when_deep() {
        assert_eq!(
            GradleFileKind::classify(&root().join("buildSrc/build.gradle.kts"), &root()),
            GradleFileKind::BuildSrcScript(DslLanguage::Kotlin)
        );
        // A buildSrc settings script is still a buildSrc script (buildSrc wins over flavor).
        assert_eq!(
            GradleFileKind::classify(&root().join("buildSrc/settings.gradle"), &root()),
            GradleFileKind::BuildSrcScript(DslLanguage::Groovy)
        );
        // Deeply nested under buildSrc still classifies as buildSrc.
        assert_eq!(
            GradleFileKind::classify(
                &root().join("buildSrc/subplugin/build.gradle.kts"),
                &root()
            ),
            GradleFileKind::BuildSrcScript(DslLanguage::Kotlin)
        );
    }

    #[test]
    fn unknown_for_odd_and_unrelated_names() {
        assert_eq!(
            GradleFileKind::classify(&root().join("README.md"), &root()),
            GradleFileKind::Unknown
        );
        assert_eq!(
            GradleFileKind::classify(&root().join("app/Main.java"), &root()),
            GradleFileKind::Unknown
        );
        // A bare "gradle" file (no recognized suffix/name) is Unknown, not a crash.
        assert_eq!(
            GradleFileKind::classify(&root().join("gradle"), &root()),
            GradleFileKind::Unknown
        );
    }

    #[test]
    fn build_src_prefix_above_root_does_not_false_positive() {
        // "buildSrc" appears ABOVE the root in the absolute prefix; the file itself is a
        // plain root build script and must NOT be misread as a buildSrc script.
        let weird_root = PathBuf::from("/buildSrc/myproject");
        assert_eq!(
            GradleFileKind::classify(&weird_root.join("build.gradle.kts"), &weird_root),
            GradleFileKind::RootBuildScript(DslLanguage::Kotlin)
        );
    }

    #[test]
    fn dsl_and_recognized_accessors() {
        assert_eq!(
            GradleFileKind::SubprojectBuildScript(DslLanguage::Kotlin).dsl(),
            Some(DslLanguage::Kotlin)
        );
        assert_eq!(GradleFileKind::VersionCatalog.dsl(), None);
        assert!(GradleFileKind::VersionCatalog.is_recognized());
        assert!(!GradleFileKind::Unknown.is_recognized());
    }
}
