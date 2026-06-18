//! Local goto-definition and workspace-local references over the semantic graph.
//!
//! This is Task 12: `textDocument/definition` and `textDocument/references` for the
//! **confidently-resolvable LOCAL** constructs the static [`SemanticGraph`] already models —
//! task references ↔ declarations, version-catalog `libs.*` accessors, and `project(":path")`
//! references ↔ settings `include`s. A cursor position is mapped to the navigable
//! [`Occurrence`] whose source span contains it (via [`locate`]); the resulting [`Symbol`] is
//! then resolved to definition target(s) against the graph ([`definition`]) or to every
//! reference site in the document ([`references`]).
//!
//! # Design: graph is the definition source-of-truth; the scanner is a tiny adapter
//!
//! `dependsOn(...)`/`finalizedBy(...)` are NOT modeled as semantic facts, and fact source
//! spans are whole-call spans (catalog facts even carry a zero span) — too coarse to use as a
//! precise reference range or an accessor cursor-hit test. So a tolerant red-tree occurrence
//! scanner ([`locate::collect_occurrences`]) supplies precise token spans + a classified
//! [`Symbol`], while the [`SemanticGraph`] remains the single source of truth for where a
//! symbol is *defined*. No second symbol index is introduced.
//!
//! # Confidence: empty over a guess
//!
//! Only confidently-local positions resolve. Unsupported token shapes, `OPAQUE`/`ERROR_NODE`
//! regions, and ambiguous positions yield an EMPTY result (never a best guess). Malformed
//! input degrades to empty and never panics.
//!
//! # LSP-type-free
//!
//! The public entries take and return only crate types ([`NavDocument`], [`SemanticGraph`],
//! byte offsets, [`NavTarget`]); the conversion to `tower_lsp` `Location`s happens at the
//! server boundary in [`lsp`].
//!
//! # Task-15 seam (external source-jar goto)
//!
//! A resolved target is a [`NavTarget`], an enum whose only variant today is
//! [`NavTarget::Local`]. Task 15 adds an `External { .. }` variant (goto into a decompiled /
//! source-jar location for a plugin-contributed type) WITHOUT touching the locate/resolve
//! split: the scanner and resolvers keep producing local targets, and only `definition`
//! gains an external branch.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::navigation::{goto_definition, NavDocument};
//! use gradle_analyzer::gradle::parser::parse_groovy;
//! use gradle_analyzer::gradle::semantic::{analyze_documents, DocumentId, SemanticInput};
//! use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
//!
//! let text = "task build {}\ntask check { dependsOn 'build' }\n";
//! let graph = analyze_documents(&[SemanticInput::script(
//!     "build.gradle",
//!     text,
//!     GradleFileKind::RootBuildScript(DslLanguage::Groovy),
//! )]);
//! let parse = parse_groovy(text);
//! let doc = NavDocument::new(DocumentId::new("build.gradle"), DslLanguage::Groovy);
//!
//! // Cursor inside the `'build'` reference resolves to the `task build` declaration.
//! let offset = text.find("'build'").unwrap() + 2;
//! let targets = goto_definition(&doc, &parse, &graph, offset);
//! assert!(!targets.is_empty());
//! ```

pub mod definition;
pub mod locate;
pub mod lsp;
pub mod references;

use crate::gradle::syntax::Parse;
use crate::gradle::semantic::{DocumentId, SemanticGraph};
use crate::gradle::syntax::{SyntaxNode, TextSpan};
use crate::gradle::workspace::DslLanguage;

pub use locate::{Occurrence, OccurrenceRole, Symbol};

/// The identity and DSL of the document a navigation request targets.
///
/// Carries the workspace-relative [`DocumentId`] (so a [`NavTarget`] can name the document a
/// definition lives in) and the [`DslLanguage`] (so the scanner picks the right node kinds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavDocument {
    id: DocumentId,
    language: DslLanguage,
}

impl NavDocument {
    /// Builds a navigation document from its id and DSL.
    pub fn new(id: DocumentId, language: DslLanguage) -> NavDocument {
        NavDocument { id, language }
    }

    /// Returns the document's workspace-relative id.
    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    /// Returns the document's DSL.
    pub fn language(&self) -> DslLanguage {
        self.language
    }
}

/// A resolved navigation destination.
///
/// Today the only variant is [`NavTarget::Local`] — a span in a known workspace document.
/// The enum shape is the documented Task-15 seam: an `External` variant (a plugin type's
/// source-jar / decompiled location) is added later without changing how positions are
/// located or how local symbols resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavTarget {
    /// A location inside a workspace document, addressed by its id and a source byte span.
    Local {
        /// The document the target lives in.
        document: DocumentId,
        /// The source byte span of the target (a declaration range or reference site).
        span: TextSpan,
    },
}

impl NavTarget {
    /// Builds a local target in `document` at `span`.
    pub fn local(document: DocumentId, span: TextSpan) -> NavTarget {
        NavTarget::Local { document, span }
    }
}

/// Resolves the definition target(s) for the symbol at byte `offset`, if any.
///
/// Returns EMPTY when `offset` is not on a navigable occurrence, the occurrence's symbol has
/// no matching definition in `graph`, or the position is opaque/ambiguous. Multiple targets
/// are returned when a symbol legitimately has several definition sites (e.g. a task declared
/// and reconfigured); the caller (an editor) handles a multi-location response.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::navigation::{goto_definition, NavDocument};
/// use gradle_analyzer::gradle::parser::parse_kotlin;
/// use gradle_analyzer::gradle::semantic::{analyze_documents, DocumentId, SemanticInput};
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
///
/// let text = "tasks.register(\"build\") {}\ntasks.named(\"build\") {}\n";
/// let graph = analyze_documents(&[SemanticInput::script(
///     "build.gradle.kts",
///     text,
///     GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
/// )]);
/// let parse = parse_kotlin(text);
/// let doc = NavDocument::new(DocumentId::new("build.gradle.kts"), DslLanguage::Kotlin);
///
/// let offset = text.find("named(\"build\")").unwrap() + 7; // inside the reference string
/// assert!(!goto_definition(&doc, &parse, &graph, offset).is_empty());
/// ```
pub fn goto_definition(
    doc: &NavDocument,
    parse: &Parse,
    graph: &SemanticGraph,
    offset: usize,
) -> Vec<NavTarget> {
    let span = tracing::trace_span!("navigation.definition", doc = doc.id().as_str(), offset);
    let _enter = span.enter();

    let root = SyntaxNode::new_root(parse.green.clone());
    let occurrences = locate::collect_occurrences(&root, doc.language());
    let Some(occurrence) = locate::locate_at(&occurrences, offset) else {
        tracing::trace!("no navigable occurrence at offset");
        return Vec::new();
    };
    let targets = definition::resolve_definition(doc, &occurrence.symbol, graph);
    tracing::trace!(targets = targets.len(), symbol = ?occurrence.symbol, "definition resolved");
    targets
}

/// Resolves every reference site for the symbol at byte `offset` in this document.
///
/// Works from either a reference or a declaration position: it locates the occurrence under
/// the cursor, then returns every occurrence in the document that shares its [`Symbol`]
/// (including the declaration, so the result is useful as an editor "find all references").
/// Returns EMPTY when `offset` is not on a navigable occurrence.
///
/// Scope: the current document only (the snapshot the request is issued against). A
/// cross-document reference sweep is intentionally out of scope for Task 12.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::navigation::{find_references, NavDocument};
/// use gradle_analyzer::gradle::parser::parse_groovy;
/// use gradle_analyzer::gradle::semantic::{analyze_documents, DocumentId, SemanticInput};
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
///
/// let text = "task build {}\ntask check { dependsOn 'build' }\n";
/// let graph = analyze_documents(&[SemanticInput::script(
///     "build.gradle",
///     text,
///     GradleFileKind::RootBuildScript(DslLanguage::Groovy),
/// )]);
/// let parse = parse_groovy(text);
/// let doc = NavDocument::new(DocumentId::new("build.gradle"), DslLanguage::Groovy);
///
/// // From the `task build` declaration, find-references includes the `dependsOn 'build'` site.
/// let offset = text.find("build").unwrap() + 1;
/// assert!(find_references(&doc, &parse, &graph, offset).len() >= 2);
/// ```
pub fn find_references(
    doc: &NavDocument,
    parse: &Parse,
    _graph: &SemanticGraph,
    offset: usize,
) -> Vec<NavTarget> {
    let span = tracing::trace_span!("navigation.references", doc = doc.id().as_str(), offset);
    let _enter = span.enter();

    let root = SyntaxNode::new_root(parse.green.clone());
    let occurrences = locate::collect_occurrences(&root, doc.language());
    let Some(occurrence) = locate::locate_at(&occurrences, offset) else {
        tracing::trace!("no navigable occurrence at offset");
        return Vec::new();
    };
    let targets = references::collect_references(doc, &occurrence.symbol, &occurrences);
    tracing::trace!(sites = targets.len(), symbol = ?occurrence.symbol, "references collected");
    targets
}

#[cfg(test)]
mod tests;
