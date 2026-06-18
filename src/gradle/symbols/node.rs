//! The LSP-type-free outline tree: [`SymbolNode`] and [`OutlineKind`].
//!
//! The builder produces a tree of these nodes from the red syntax tree alone; the protocol
//! boundary ([`super::convert`]) is the only place that maps them to `tower_lsp`
//! `DocumentSymbol`s. Keeping the internal tree free of LSP types lets the walker and its
//! tests run without the protocol crate and lets later tasks (code actions, navigation)
//! consume the same structure.

use crate::gradle::syntax::TextSpan;

/// The category of an outline entry, mapped to an LSP `SymbolKind` at the boundary.
///
/// The categories are Gradle-shaped rather than language-shaped: they name the build-script
/// constructs an editor outline cares about, independent of Kotlin vs Groovy surface syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// A major script section or block whose name is not otherwise specialized
    /// (`buildscript`, `allprojects`, a generic configuration block).
    Section,
    /// A project reference or include (`include(":app")`, `rootProject.name`).
    Project,
    /// A plugin application (`id("java")`, `kotlin("jvm")`, `apply plugin: 'x'`).
    Plugin,
    /// A repository declaration (`mavenCentral()`, `google()`).
    Repository,
    /// A dependency declaration (`implementation("g:a:v")`).
    Dependency,
    /// A task declaration or registration (`tasks.register("x")`, `task foo {}`).
    Task,
    /// A property assignment (`group = "..."`, `version = "..."`).
    Property,
    /// A version-catalog table section (`[libraries]`) or a generic container block.
    Block,
}

/// A single node in the hierarchical document outline.
///
/// `span` covers the whole construct (used for the LSP `range`); `selection` covers the
/// name token (used for the LSP `selection_range`) and is always contained within `span`.
/// Names are SOURCE identifiers (e.g. `dependencies`, `implementation`) — not translatable
/// UI — so they are stored verbatim and never routed through the i18n layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolNode {
    /// The display name (a source identifier or extracted coordinate).
    pub name: String,
    /// Optional secondary text (e.g. a dependency coordinate, a property value).
    pub detail: Option<String>,
    /// The outline category.
    pub kind: OutlineKind,
    /// The byte span of the whole construct.
    pub span: TextSpan,
    /// The byte span of the name token, always within `span`.
    pub selection: TextSpan,
    /// Nested outline entries.
    pub children: Vec<SymbolNode>,
}

impl SymbolNode {
    /// Builds a leaf node (no children) from its parts.
    pub fn leaf(
        name: impl Into<String>,
        detail: Option<String>,
        kind: OutlineKind,
        span: TextSpan,
        selection: TextSpan,
    ) -> SymbolNode {
        SymbolNode {
            name: name.into(),
            detail,
            kind,
            span,
            selection,
            children: Vec::new(),
        }
    }

    /// Builds a container node with `children`.
    pub fn container(
        name: impl Into<String>,
        kind: OutlineKind,
        span: TextSpan,
        selection: TextSpan,
        children: Vec<SymbolNode>,
    ) -> SymbolNode {
        SymbolNode {
            name: name.into(),
            detail: None,
            kind,
            span,
            selection,
            children,
        }
    }
}
