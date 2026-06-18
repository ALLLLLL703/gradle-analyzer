//! Document outline (`textDocument/documentSymbol`) from syntax, refined by semantics.
//!
//! Produces a syntax-first hierarchical outline for Gradle build scripts (Kotlin and Groovy)
//! and version catalogs. The pipeline is split for testability and a clean protocol boundary:
//!
//! - [`builder`] walks the red syntax tree into an LSP-type-free [`node::SymbolNode`] tree;
//! - [`refine`] enriches it best-effort from the Task 7 [`SemanticGraph`] (never required);
//! - [`catalog_outline`] handles `libs.versions.toml` via a tiny line scanner;
//! - [`convert`] maps the internal tree to `tower_lsp` `DocumentSymbol`s at the boundary,
//!   using the reusable [`crate::util::line_index::LineIndex`] for byte → line/character.
//!
//! The walk is tolerant by construction: a partially-invalid file still yields a USEFUL
//! PARTIAL outline (early symbols intact; later sections may nest under an unclosed parent),
//! never empty/noisy and never panicking. The public entry [`document_symbols`] is
//! LSP-type-free so later features (code actions, navigation) can consume the same tree;
//! [`outline_lsp`] is the thin server-boundary convenience that builds everything and returns
//! protocol types.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::parser::parse_kotlin;
//! use gradle_analyzer::gradle::semantic::SemanticGraph;
//! use gradle_analyzer::gradle::symbols::document_symbols;
//! use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
//! use tower_lsp::lsp_types::Url;
//!
//! let source = "plugins {\n    id(\"java\")\n}\ndependencies {\n    implementation(\"g:a:v\")\n}\n";
//! let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
//! let doc = TrackedDocument::new(uri, 1, source, GradleFileKind::RootBuildScript(DslLanguage::Kotlin));
//! let parse = parse_kotlin(source);
//! let graph = SemanticGraph::new();
//!
//! let symbols = document_symbols(&doc, &parse, &graph);
//! // Top level: a `plugins` block and a `dependencies` block.
//! assert_eq!(symbols.len(), 2);
//! assert_eq!(symbols[0].name, "plugins");
//! assert_eq!(symbols[1].name, "dependencies");
//! // The dependency's configuration is the name; the coordinate is the detail.
//! assert_eq!(symbols[1].children[0].name, "implementation");
//! assert_eq!(symbols[1].children[0].detail.as_deref(), Some("g:a:v"));
//! ```

pub mod builder;
pub mod catalog_outline;
pub mod convert;
pub mod kinds;
pub mod naming;
pub mod node;
pub mod refine;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::DocumentSymbol;

use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{SemanticGraph, SemanticInput, analyze_documents};
use crate::gradle::syntax::{Parse, SyntaxNode};
use crate::gradle::workspace::{GradleFileKind, TrackedDocument};
use crate::util::line_index::LineIndex;

use kinds::DslKinds;
pub use node::{OutlineKind, SymbolNode};

/// Builds the LSP-type-free outline for `doc` from its `parse` and an optional `graph`.
///
/// Dispatches on the document's [`GradleFileKind`]: a version catalog is outlined by its TOML
/// sections; a DSL script is walked from the red tree and then refined by `graph` (a no-op if
/// the graph has no matching facts); an unrecognized file yields an empty outline. `graph` is
/// advisory — outline structure comes from syntax alone, so an empty graph still produces a
/// full outline. Never panics, even on malformed input.
pub fn document_symbols(
    doc: &TrackedDocument,
    parse: &Parse,
    graph: &SemanticGraph,
) -> Vec<SymbolNode> {
    let span = tracing::info_span!("document_symbols", uri = %doc.uri(), kind = ?doc.kind());
    let _guard = span.enter();

    let symbols = match doc.kind() {
        GradleFileKind::VersionCatalog => catalog_outline::build(doc.text()),
        kind => match kind.dsl() {
            Some(dsl) => {
                let root = SyntaxNode::new_root(parse.green.clone());
                let dsl_kinds = DslKinds::for_language(dsl);
                let mut symbols = builder::build(&root, &dsl_kinds);
                refine::refine(&mut symbols, graph);
                symbols
            }
            None => Vec::new(),
        },
    };
    tracing::debug!(symbol_count = symbols.len(), "outline built");
    symbols
}

/// Server-boundary convenience: builds the parse, a single-document graph, and the line
/// index, then returns protocol-ready `DocumentSymbol`s for `doc`.
///
/// Keeps the LSP handler a one-line delegation. The graph is built from `doc` alone, so
/// catalog-accessor refinement only fires when a catalog is in scope; the syntax outline is
/// always complete regardless.
pub fn outline_lsp(doc: &TrackedDocument) -> Vec<DocumentSymbol> {
    let parse = parse_for(doc);
    let graph = single_document_graph(doc);
    let symbols = document_symbols(doc, &parse, &graph);
    let index = LineIndex::new(doc.text_arc());
    convert::to_document_symbols(&symbols, &index)
}

/// Parses `doc` with the frontend matching its DSL (Groovy is the harmless default for files
/// without a DSL, since `document_symbols` ignores the parse for those).
fn parse_for(doc: &TrackedDocument) -> Parse {
    match doc.kind().dsl() {
        Some(crate::gradle::workspace::DslLanguage::Kotlin) => parse_kotlin(doc.text()),
        _ => parse_groovy(doc.text()),
    }
}

/// Builds a semantic graph from `doc` alone (its parent directory as the synthetic root).
fn single_document_graph(doc: &TrackedDocument) -> SemanticGraph {
    let root = doc
        .uri()
        .to_file_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("/"));
    analyze_documents(&[SemanticInput::from_tracked(&root, doc)])
}
