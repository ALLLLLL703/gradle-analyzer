//! Static semantic graph over both DSL frontends' parse trees.
//!
//! This is Task 7: a set of offline extraction passes that walk the Kotlin (`parse_kotlin`)
//! and Groovy (`parse_groovy`) red trees — plus version-catalog TOML — into a
//! [`SemanticGraph`] of per-document [`SemanticDocument`]s. Each fact carries stable,
//! deterministic identity ([`SemanticFactMetadata`] with a [`SemanticId`]), an ownership link,
//! a source span, and a [`FactStatus`]. The release-1 nucleus is extracted: project
//! includes/paths/`rootProject.name`, plugins, repositories, dependencies (string **and**
//! `libs.*` accessor, resolved against the catalog), tasks, imports, version-catalog entries,
//! and buildSrc-contributed local symbols.
//!
//! It depends on NOTHING beyond the existing red trees and [`TrackedDocument`] — no sidecar,
//! no second parser/workspace model — and never resolves dynamic plugin-contributed members
//! (that is Task 16). Partial/malformed input (opaque regions, parse errors) degrades to
//! `Partial` facts and never panics; `OPAQUE`/`ERROR_NODE` subtrees are skipped by design.
//!
//! # SemanticId scheme (for Tasks 9-13/16)
//!
//! An id is `"<workspace-relative-doc>::<kind>:<key>"`; identical segments under one document
//! get deterministic `#2`/`#3` suffixes. Same input always yields the same ids.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::semantic::{analyze, SemanticFactKind};
//! use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
//! use tower_lsp::lsp_types::Url;
//! use std::path::Path;
//!
//! let root = Path::new("/proj");
//! let uri = Url::from_file_path("/proj/build.gradle").unwrap();
//! let doc = TrackedDocument::new(
//!     uri,
//!     1,
//!     "plugins { id 'java' }\ndependencies { implementation 'g:a:1' }",
//!     GradleFileKind::RootBuildScript(DslLanguage::Groovy),
//! );
//!
//! let graph = analyze(root, &[doc]);
//! let document = graph.documents().next().unwrap();
//! assert_eq!(document.id().as_str(), "build.gradle");
//! assert!(document.facts_of_kind(SemanticFactKind::Plugin).count() >= 1);
//! assert!(document.facts_of_kind(SemanticFactKind::Dependency).count() >= 1);
//! ```

pub mod catalog;
pub mod extract;
pub mod facts;
pub mod graph;
pub mod id;
pub mod message;
pub mod view;

use std::rc::Rc;
use std::sync::Arc;

use tracing::trace;

use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::syntax::SyntaxNode;
use crate::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};

pub use catalog::{CatalogPluginEntry, VersionCatalog};
pub use facts::{
    BuildSrcSymbolKind, CatalogResolution, DependencyCoordinate, FactPayload, FactStatus,
    SemanticFact, SemanticFactKind, SemanticFactMetadata,
};
pub use graph::{SemanticDocument, SemanticGraph};
pub use id::{DocumentId, IdAllocator, SemanticId};
pub use message::{describe_catalog_parse_error, describe_resolution};

use catalog::VersionCatalog as Catalog;
use extract::catalog_refs;

/// A single document to analyze: its id, role, and text.
///
/// This is the DSL-agnostic unit [`analyze_documents`] consumes. Build it from a
/// [`TrackedDocument`] (via [`SemanticInput::from_tracked`]) or directly from text (via
/// [`SemanticInput::script`]) for focused tests.
#[derive(Debug, Clone)]
pub struct SemanticInput {
    /// The workspace-relative document id.
    pub id: DocumentId,
    /// The file's classified role (drives DSL routing and buildSrc handling).
    pub kind: GradleFileKind,
    /// The document text.
    pub text: Arc<str>,
}

impl SemanticInput {
    /// Builds an input directly from a relative id, text, and kind (for tests/demos).
    pub fn script(id: &str, text: impl Into<Arc<str>>, kind: GradleFileKind) -> SemanticInput {
        SemanticInput {
            id: DocumentId::new(id),
            kind,
            text: text.into(),
        }
    }

    /// Builds an input from a [`TrackedDocument`], deriving its id relative to `root`.
    pub fn from_tracked(root: &std::path::Path, doc: &TrackedDocument) -> SemanticInput {
        let id = doc
            .uri()
            .to_file_path()
            .map(|path| DocumentId::from_relative_path(root, &path))
            .unwrap_or_else(|_| DocumentId::new(doc.uri().as_str()));
        SemanticInput {
            id,
            kind: doc.kind(),
            text: doc.text_arc(),
        }
    }
}

/// Analyzes a set of tracked documents under `root` into a [`SemanticGraph`].
///
/// This is the high-level entry: it derives each document's workspace-relative id, parses
/// every version catalog first (so `libs.*` accessors resolve), then extracts facts from
/// every script. See the module example for usage.
pub fn analyze(root: &std::path::Path, documents: &[TrackedDocument]) -> SemanticGraph {
    let inputs: Vec<SemanticInput> = documents
        .iter()
        .map(|doc| SemanticInput::from_tracked(root, doc))
        .collect();
    analyze_documents(&inputs)
}

/// Analyzes pre-identified [`SemanticInput`]s into a [`SemanticGraph`].
///
/// The catalog pass runs first across all `*.versions.toml` inputs and is merged into one
/// catalog, which every script then resolves accessors against. Analysis is deterministic:
/// identical inputs always yield identical ids and fact order.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput, SemanticFactKind};
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
///
/// let inputs = vec![
///     SemanticInput::script(
///         "gradle/libs.versions.toml",
///         "[libraries]\nguava = \"com.google.guava:guava:33.0.0-jre\"",
///         GradleFileKind::VersionCatalog,
///     ),
///     SemanticInput::script(
///         "build.gradle.kts",
///         "dependencies { implementation(libs.guava) }",
///         GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
///     ),
/// ];
/// let graph = analyze_documents(&inputs);
/// assert_eq!(graph.documents().count(), 2);
/// ```
pub fn analyze_documents(inputs: &[SemanticInput]) -> SemanticGraph {
    let span = tracing::info_span!("semantic.analyze", documents = inputs.len());
    let _enter = span.enter();

    let combined = combined_catalog(inputs);
    let mut graph = SemanticGraph::new();

    for input in inputs {
        let document = analyze_input(input, &combined);
        graph.insert(document);
    }

    trace!(facts = graph.all_facts().count(), "analysis complete");
    graph
}

/// Parses and merges every version-catalog input into the single resolution catalog.
fn combined_catalog(inputs: &[SemanticInput]) -> Catalog {
    let mut combined = Catalog::default();
    for input in inputs {
        if input.kind == GradleFileKind::VersionCatalog {
            let parsed = Catalog::parse(&input.text);
            combined.merge(&parsed);
        }
    }
    combined
}

/// Analyzes one input into a [`SemanticDocument`], routing catalogs vs scripts.
fn analyze_input(input: &SemanticInput, combined: &Catalog) -> SemanticDocument {
    if input.kind == GradleFileKind::VersionCatalog {
        return analyze_catalog(input, combined);
    }
    match graph::script_language(input.kind) {
        Some(lang) => analyze_script(input, lang, combined),
        None => SemanticDocument::new(input.id.clone(), Vec::new()),
    }
}

/// Extracts a version-catalog document's own entries into facts.
fn analyze_catalog(input: &SemanticInput, combined: &Catalog) -> SemanticDocument {
    let span = tracing::trace_span!("semantic.catalog", doc = input.id.as_str());
    let _enter = span.enter();

    let parsed = Catalog::parse(&input.text);
    let mut emitter = extract::Emitter::for_document(input.id.clone(), combined);
    catalog_refs::extract_catalog(&mut emitter, &parsed);
    SemanticDocument::new(input.id.clone(), emitter.into_facts())
        .with_catalog_parse_error(parsed.had_parse_error())
}

/// Parses and extracts a build/settings/buildSrc script into facts.
fn analyze_script(input: &SemanticInput, lang: DslLanguage, combined: &Catalog) -> SemanticDocument {
    let root = parse_root(&input.text, lang);
    let is_build_src = matches!(input.kind, GradleFileKind::BuildSrcScript(_));
    let facts = extract::extract_document(input.id.clone(), &root, lang, combined, is_build_src);
    SemanticDocument::new(input.id.clone(), facts)
}

/// Parses `text` with the frontend for `lang` and returns the red-tree root.
fn parse_root(text: &str, lang: DslLanguage) -> Rc<SyntaxNode> {
    let parse = match lang {
        DslLanguage::Kotlin => parse_kotlin(text),
        DslLanguage::Groovy => parse_groovy(text),
    };
    SyntaxNode::new_root(parse.green)
}

#[cfg(test)]
mod tests;
