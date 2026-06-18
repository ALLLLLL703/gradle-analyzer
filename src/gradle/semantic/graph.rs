//! The aggregate result types: [`SemanticDocument`] and [`SemanticGraph`].
//!
//! A [`SemanticDocument`] holds the facts extracted from one file plus its catalog parse
//! status; a [`SemanticGraph`] is the workspace-wide collection keyed by [`DocumentId`],
//! preserving insertion order so iteration and demos are deterministic. These are pure data
//! aggregates with read accessors and lookup-by-id helpers — Tasks 9-13/16 consume them.

use crate::gradle::workspace::DslLanguage;

use super::facts::{SemanticFact, SemanticFactKind};
use super::id::{DocumentId, SemanticId};

/// All facts extracted from one document, plus its identity and catalog status.
#[derive(Debug, Clone)]
pub struct SemanticDocument {
    id: DocumentId,
    facts: Vec<SemanticFact>,
    had_catalog_parse_error: bool,
}

impl SemanticDocument {
    /// Builds a document record from its id and facts (no catalog error).
    pub(crate) fn new(id: DocumentId, facts: Vec<SemanticFact>) -> SemanticDocument {
        SemanticDocument {
            id,
            facts,
            had_catalog_parse_error: false,
        }
    }

    /// Marks this document as having failed to parse its version catalog TOML.
    pub(crate) fn with_catalog_parse_error(mut self, had_error: bool) -> SemanticDocument {
        self.had_catalog_parse_error = had_error;
        self
    }

    /// Returns this document's id.
    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    /// Returns every fact extracted from this document, in extraction order.
    pub fn facts(&self) -> &[SemanticFact] {
        &self.facts
    }

    /// Returns `true` if this document's version catalog TOML failed to parse.
    pub fn had_catalog_parse_error(&self) -> bool {
        self.had_catalog_parse_error
    }

    /// Returns the facts of one kind, in extraction order.
    pub fn facts_of_kind(&self, kind: SemanticFactKind) -> impl Iterator<Item = &SemanticFact> {
        self.facts.iter().filter(move |f| f.kind() == kind)
    }

    /// Looks up a fact by its stable id.
    pub fn fact(&self, id: &SemanticId) -> Option<&SemanticFact> {
        self.facts.iter().find(|f| f.id() == id)
    }
}

/// The workspace-wide semantic graph: every analyzed document's facts.
///
/// Documents are stored in insertion order (the order they were analyzed) so iteration is
/// deterministic across runs of identical input. Facts within a document are likewise in
/// extraction order, making the whole structure golden-test friendly.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput};
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind};
///
/// let inputs = vec![SemanticInput::script(
///     "build.gradle",
///     "plugins { id 'java' }",
///     GradleFileKind::RootBuildScript(DslLanguage::Groovy),
/// )];
/// let graph = analyze_documents(&inputs);
/// assert_eq!(graph.documents().count(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct SemanticGraph {
    documents: Vec<SemanticDocument>,
}

impl SemanticGraph {
    /// Creates an empty graph.
    pub fn new() -> SemanticGraph {
        SemanticGraph::default()
    }

    /// Appends a document's facts to the graph.
    pub(crate) fn insert(&mut self, document: SemanticDocument) {
        self.documents.push(document);
    }

    /// Iterates the analyzed documents in insertion (analysis) order.
    pub fn documents(&self) -> impl Iterator<Item = &SemanticDocument> {
        self.documents.iter()
    }

    /// Looks up an analyzed document by its id.
    pub fn document(&self, id: &DocumentId) -> Option<&SemanticDocument> {
        self.documents.iter().find(|d| d.id() == id)
    }

    /// Iterates every fact across every document.
    pub fn all_facts(&self) -> impl Iterator<Item = &SemanticFact> {
        self.documents.iter().flat_map(SemanticDocument::facts)
    }

    /// Looks up a fact anywhere in the graph by its stable id.
    pub fn fact(&self, id: &SemanticId) -> Option<&SemanticFact> {
        self.documents.iter().find_map(|d| d.fact(id))
    }
}

/// The DSL a document is written in, if it is a script (a catalog has none).
pub(crate) fn script_language(kind: crate::gradle::workspace::GradleFileKind) -> Option<DslLanguage> {
    kind.dsl()
}
