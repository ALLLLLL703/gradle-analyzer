//! The semantic fact taxonomy: what an extraction pass records about a build script.
//!
//! A [`SemanticFact`] is one statically-known thing about a Gradle workspace — an applied
//! plugin, a declared repository, a dependency, a registered task, an `include`, an import,
//! a version-catalog entry, or a buildSrc-contributed symbol. Every fact carries
//! [`SemanticFactMetadata`] (its stable id, optional parent id, and source span) plus a
//! [`FactStatus`] flagging whether extraction recovered the whole construct or only part of
//! it (the degradation path for malformed/partial input).
//!
//! The payload is a [`FactPayload`] enum so a consumer matches once and gets kind-specific
//! data. Dependencies additionally carry a [`DependencyCoordinate`] distinguishing string
//! notation, a version-catalog accessor (with its [`CatalogResolution`]), and project refs.

use crate::gradle::syntax::TextSpan;

use super::id::SemanticId;

/// Whether a fact was extracted completely or degraded from malformed/partial input.
///
/// Partial facts are emitted (never dropped) so downstream features still see *something*
/// at a source location — e.g. a plugin block whose id string is missing still yields a
/// `Partial` plugin fact rather than nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactStatus {
    /// Every modeled field was recovered from the source.
    Complete,
    /// The construct was recognized but some field was missing/malformed.
    Partial,
}

/// The high-level classification of a [`SemanticFact`], independent of its payload data.
///
/// Mirrors the id-segment tags used by [`SemanticId`] so a kind round-trips to its id
/// prefix. Tasks 9-13/16 switch on this to decide which facts a feature consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticFactKind {
    /// An `include(":app")` / `include ':app'` project membership.
    ProjectInclude,
    /// A `project(":core")` reference (e.g. in a dependency).
    ProjectPath,
    /// A `rootProject.name = "..."` assignment.
    RootProjectName,
    /// An applied/declared plugin.
    Plugin,
    /// A declared repository.
    Repository,
    /// A dependency declaration.
    Dependency,
    /// A task registration/configuration.
    Task,
    /// An `import` statement.
    Import,
    /// A `[versions]` catalog entry.
    CatalogVersion,
    /// A `[libraries]` catalog entry.
    CatalogLibrary,
    /// A `[bundles]` catalog entry.
    CatalogBundle,
    /// A `[plugins]` catalog entry.
    CatalogPlugin,
    /// A buildSrc/convention-contributed local symbol (task or plugin name; static only).
    BuildSrcSymbol,
}

impl SemanticFactKind {
    /// Returns the lowercase id-segment tag for this kind (the `<kind>` in a [`SemanticId`]).
    pub const fn segment_tag(self) -> &'static str {
        match self {
            SemanticFactKind::ProjectInclude
            | SemanticFactKind::ProjectPath
            | SemanticFactKind::RootProjectName => "project",
            SemanticFactKind::Plugin => "plugin",
            SemanticFactKind::Repository => "repository",
            SemanticFactKind::Dependency => "dependency",
            SemanticFactKind::Task => "task",
            SemanticFactKind::Import => "import",
            SemanticFactKind::CatalogVersion
            | SemanticFactKind::CatalogLibrary
            | SemanticFactKind::CatalogBundle
            | SemanticFactKind::CatalogPlugin => "catalog",
            SemanticFactKind::BuildSrcSymbol => "buildsrc",
        }
    }
}

/// Whether a buildSrc-contributed symbol names a task or a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildSrcSymbolKind {
    /// A task name declared in a buildSrc script.
    Task,
    /// A plugin id declared in a buildSrc / precompiled-script plugin.
    Plugin,
}

/// How a version-catalog accessor (`libs.*`) resolved against the parsed catalog.
///
/// `Resolved` carries the catalog entry key and its coordinate string; `Unresolved` is the
/// recorded-not-panicked outcome for an accessor with no matching entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogResolution {
    /// The accessor matched a catalog entry; carries the entry alias + resolved coordinate.
    Resolved {
        /// The catalog entry alias the accessor resolved to (e.g. `guava`).
        alias: String,
        /// The coordinate the entry resolves to (e.g. `com.google.guava:guava:33.0.0-jre`).
        coordinate: String,
    },
    /// No catalog entry matched the accessor (recorded, never a panic).
    Unresolved,
}

impl CatalogResolution {
    /// Returns `true` if the accessor resolved to a catalog entry.
    pub fn is_resolved(&self) -> bool {
        matches!(self, CatalogResolution::Resolved { .. })
    }
}

/// What notation a dependency declaration used, and its resolved meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyCoordinate {
    /// String notation, e.g. `implementation("g:a:v")` / `implementation 'g:a:v'`.
    StringNotation(String),
    /// A version-catalog accessor, e.g. `libs.guava`, with its resolution outcome.
    CatalogAccessor {
        /// The accessor path as written (e.g. `libs.guava`, `libs.bundles.networking`).
        accessor: String,
        /// How the accessor resolved against the parsed catalog.
        resolution: CatalogResolution,
    },
    /// A project reference, e.g. `project(":core")`.
    ProjectRef(String),
    /// A dependency whose coordinate could not be modeled (partial/unknown shape).
    Unknown,
}

/// The kind-specific data of a [`SemanticFact`].
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{FactPayload, SemanticFactKind};
///
/// let payload = FactPayload::Plugin { id: "java".to_string(), version: None, apply: true };
/// assert_eq!(payload.kind(), SemanticFactKind::Plugin);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactPayload {
    /// An `import org.foo.Bar` statement; carries the dotted path.
    Import(String),
    /// An applied/declared plugin (`id "java"`, `kotlin("jvm")`, `apply plugin: "x"`).
    Plugin {
        /// The plugin id (e.g. `java`, `org.jetbrains.kotlin.jvm`).
        id: String,
        /// An explicit `version "x"`, if present.
        version: Option<String>,
        /// Whether the plugin is applied (`apply false` flips this to `false`).
        apply: bool,
    },
    /// A declared repository (`mavenCentral()`, `google()`, `maven { url ... }`).
    Repository {
        /// The repository name/method (e.g. `mavenCentral`, `maven`).
        name: String,
        /// An explicit URL for a custom `maven { url ... }` repo, if recovered.
        url: Option<String>,
    },
    /// A dependency declaration: a configuration plus a coordinate.
    Dependency {
        /// The configuration (e.g. `implementation`, `testImplementation`, `api`).
        configuration: String,
        /// The coordinate notation and its resolved meaning.
        coordinate: DependencyCoordinate,
    },
    /// A task registration/configuration (`task foo {}`, `tasks.register("x")`).
    Task {
        /// The task name.
        name: String,
        /// `true` for `register`/`task` declarations, `false` for `named` configuration.
        registered: bool,
    },
    /// An `include(":app")` project membership; carries the project path.
    ProjectInclude(String),
    /// A `project(":core")` reference; carries the project path.
    ProjectPath(String),
    /// A `rootProject.name = "..."` assignment; carries the name.
    RootProjectName(String),
    /// A `[versions]` catalog entry: alias + version string.
    CatalogVersion {
        /// The version alias (e.g. `kotlin`).
        alias: String,
        /// The version value (e.g. `1.9.22`).
        version: String,
    },
    /// A `[libraries]` catalog entry: alias + resolved coordinate.
    CatalogLibrary {
        /// The library alias (e.g. `guava`).
        alias: String,
        /// The resolved coordinate (e.g. `com.google.guava:guava:33.0.0-jre`).
        coordinate: String,
    },
    /// A `[bundles]` catalog entry: alias + member aliases.
    CatalogBundle {
        /// The bundle alias (e.g. `networking`).
        alias: String,
        /// The library aliases the bundle groups.
        members: Vec<String>,
    },
    /// A `[plugins]` catalog entry: alias + plugin id (+ optional version).
    CatalogPlugin {
        /// The plugin alias.
        alias: String,
        /// The plugin id (e.g. `org.jetbrains.kotlin.jvm`).
        id: String,
        /// The resolved version, if any.
        version: Option<String>,
    },
    /// A buildSrc/convention-contributed symbol name (static visibility only).
    BuildSrcSymbol {
        /// The contributed name (task name or plugin id).
        name: String,
        /// Whether the name is a task or a plugin.
        symbol: BuildSrcSymbolKind,
    },
}

impl FactPayload {
    /// Returns the [`SemanticFactKind`] this payload represents.
    pub fn kind(&self) -> SemanticFactKind {
        match self {
            FactPayload::Import(_) => SemanticFactKind::Import,
            FactPayload::Plugin { .. } => SemanticFactKind::Plugin,
            FactPayload::Repository { .. } => SemanticFactKind::Repository,
            FactPayload::Dependency { .. } => SemanticFactKind::Dependency,
            FactPayload::Task { .. } => SemanticFactKind::Task,
            FactPayload::ProjectInclude(_) => SemanticFactKind::ProjectInclude,
            FactPayload::ProjectPath(_) => SemanticFactKind::ProjectPath,
            FactPayload::RootProjectName(_) => SemanticFactKind::RootProjectName,
            FactPayload::CatalogVersion { .. } => SemanticFactKind::CatalogVersion,
            FactPayload::CatalogLibrary { .. } => SemanticFactKind::CatalogLibrary,
            FactPayload::CatalogBundle { .. } => SemanticFactKind::CatalogBundle,
            FactPayload::CatalogPlugin { .. } => SemanticFactKind::CatalogPlugin,
            FactPayload::BuildSrcSymbol { .. } => SemanticFactKind::BuildSrcSymbol,
        }
    }
}

/// The stable identity + ownership + provenance of one [`SemanticFact`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFactMetadata {
    /// The fact's stable, deterministic id.
    pub id: SemanticId,
    /// The owning fact's id, if this fact is logically nested under another.
    pub parent_id: Option<SemanticId>,
    /// The source byte span this fact was extracted from.
    pub source: TextSpan,
}

/// One statically-known fact about a Gradle workspace.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{
///     DocumentId, FactPayload, SemanticFact, SemanticFactMetadata, SemanticId, FactStatus,
/// };
/// use gradle_analyzer::gradle::syntax::TextSpan;
///
/// let doc = DocumentId::new("build.gradle");
/// let fact = SemanticFact {
///     metadata: SemanticFactMetadata {
///         id: SemanticId::new(&doc, "plugin", "java"),
///         parent_id: None,
///         source: TextSpan::new(0, 8),
///     },
///     status: FactStatus::Complete,
///     payload: FactPayload::Plugin { id: "java".into(), version: None, apply: true },
/// };
/// assert_eq!(fact.payload.kind().segment_tag(), "plugin");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFact {
    /// Identity, ownership, and provenance.
    pub metadata: SemanticFactMetadata,
    /// Whether extraction recovered the whole construct.
    pub status: FactStatus,
    /// The kind-specific data.
    pub payload: FactPayload,
}

impl SemanticFact {
    /// Returns this fact's stable id.
    pub fn id(&self) -> &SemanticId {
        &self.metadata.id
    }

    /// Returns this fact's classification.
    pub fn kind(&self) -> SemanticFactKind {
        self.payload.kind()
    }
}
