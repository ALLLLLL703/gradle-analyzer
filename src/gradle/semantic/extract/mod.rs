//! The extraction driver: walks a normalized statement view into [`SemanticFact`]s.
//!
//! [`extract_document`] lowers a parsed red tree (via [`super::view`]) and walks it with a
//! scope stack, routing each statement to the right per-fact-kind extractor in
//! [`plugins`]/[`repos`]/[`deps`]/[`tasks`]/[`includes`]. The scope stack is what lets one
//! generic walk serve nested blocks: a call inside `repositories { }` is a repository, the
//! same call shape inside `dependencies { }` is a dependency, and container blocks
//! (`pluginManagement`, `dependencyResolutionManagement`, …) are descended generically.
//!
//! All extraction is offline and tolerant: only nucleus nodes are visited, `OPAQUE`/
//! `ERROR_NODE` subtrees never reach here, and a construct missing a modeled piece yields a
//! `Partial` fact rather than a panic.

pub mod buildsrc;
pub mod catalog_refs;
pub mod deps;
pub mod includes;
pub mod plugins;
pub mod repos;
pub mod tasks;

use std::rc::Rc;

use tracing::trace;

use crate::gradle::syntax::{SyntaxNode, TextSpan};
use crate::gradle::workspace::DslLanguage;

use super::catalog::VersionCatalog;
use super::facts::{FactPayload, FactStatus, SemanticFact, SemanticFactMetadata};
use super::id::{DocumentId, IdAllocator, SemanticId};
use super::view::{self, CallExpr, Statement};

/// Accumulates facts for one document, owning the id allocator and a catalog reference.
///
/// Extractors call [`Emitter::push`] to record a fact; the emitter assigns its stable id
/// (with deterministic duplicate suffixing) from the payload's kind tag and a caller key.
pub(crate) struct Emitter<'a> {
    alloc: IdAllocator,
    catalog: &'a VersionCatalog,
    facts: Vec<SemanticFact>,
}

impl<'a> Emitter<'a> {
    /// Creates an emitter for `document` resolving accessors against `catalog`.
    fn new(document: DocumentId, catalog: &'a VersionCatalog) -> Emitter<'a> {
        Emitter {
            alloc: IdAllocator::new(document),
            catalog,
            facts: Vec::new(),
        }
    }

    /// Creates an emitter for `document` (public to the semantic module, e.g. catalog pass).
    pub(crate) fn for_document(document: DocumentId, catalog: &'a VersionCatalog) -> Emitter<'a> {
        Emitter::new(document, catalog)
    }

    /// Consumes the emitter, returning the accumulated facts in extraction order.
    pub(crate) fn into_facts(self) -> Vec<SemanticFact> {
        self.facts
    }

    /// Returns the catalog accessors resolve against.
    pub(crate) fn catalog(&self) -> &VersionCatalog {
        self.catalog
    }

    /// Records a fact, allocating its stable id from `key` and returning that id.
    pub(crate) fn push(
        &mut self,
        key: &str,
        parent_id: Option<SemanticId>,
        source: TextSpan,
        status: FactStatus,
        payload: FactPayload,
    ) -> SemanticId {
        let tag = payload.kind().segment_tag();
        let id = self.alloc.allocate(tag, key);
        self.facts.push(SemanticFact {
            metadata: SemanticFactMetadata {
                id: id.clone(),
                parent_id,
                source,
            },
            status,
            payload,
        });
        id
    }
}

/// Which leaf block a call is currently inside (determines its fact kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Plugins,
    Repositories,
    Dependencies,
    Tasks,
}

impl Leaf {
    /// Maps a scope segment name to its leaf block kind, if it is one.
    fn from_segment(segment: &str) -> Option<Leaf> {
        match segment {
            "plugins" => Some(Leaf::Plugins),
            "repositories" => Some(Leaf::Repositories),
            "dependencies" => Some(Leaf::Dependencies),
            "tasks" => Some(Leaf::Tasks),
            _ => None,
        }
    }
}

/// Extracts every nucleus fact from one parsed build/settings/buildSrc document.
///
/// `root` is the red-tree root for `lang`; `catalog` resolves `libs.*` accessors. When
/// `is_build_src` is set, contributed local task symbols are additionally recorded as
/// [`FactPayload::BuildSrcSymbol`]s (static names only).
pub(crate) fn extract_document(
    document: DocumentId,
    root: &Rc<SyntaxNode>,
    lang: DslLanguage,
    catalog: &VersionCatalog,
    is_build_src: bool,
) -> Vec<SemanticFact> {
    let span = tracing::trace_span!("semantic.extract_document", doc = document.as_str(), ?lang);
    let _enter = span.enter();

    let mut emitter = Emitter::new(document, catalog);
    let mut scope: Vec<String> = Vec::new();
    walk(&mut emitter, root, lang, &mut scope);

    if is_build_src {
        buildsrc::contribute(&mut emitter);
    }

    trace!(facts = emitter.facts.len(), "extraction complete");
    emitter.facts
}

/// Walks the direct child statements of `node`, recursing into nucleus blocks.
fn walk(emitter: &mut Emitter, node: &SyntaxNode, lang: DslLanguage, scope: &mut Vec<String>) {
    for statement in view::child_statements(node, lang) {
        match statement {
            Statement::Import { path, span } => {
                emit_import(emitter, &path, span);
            }
            Statement::Assignment(assign) => {
                includes::extract_assignment(emitter, &assign);
            }
            Statement::Call(call) => walk_call(emitter, call, lang, scope),
        }
    }
}

/// Routes one call: leaf-scope decl, direct-head fact, or generic block recursion.
fn walk_call(emitter: &mut Emitter, call: CallExpr, lang: DslLanguage, scope: &mut Vec<String>) {
    if let Some(leaf) = scope.last().and_then(|s| Leaf::from_segment(s)) {
        dispatch_leaf(emitter, leaf, &call);
        return;
    }
    if try_direct(emitter, &call) {
        return;
    }
    if let Some(block) = call.block.clone() {
        scope.push(call.head.clone());
        walk(emitter, &block, lang, scope);
        scope.pop();
    }
}

/// Dispatches a call inside a leaf block to its per-kind extractor.
fn dispatch_leaf(emitter: &mut Emitter, leaf: Leaf, call: &CallExpr) {
    match leaf {
        Leaf::Plugins => plugins::extract_plugin(emitter, call),
        Leaf::Repositories => repos::extract_repository(emitter, call),
        Leaf::Dependencies => deps::extract_dependency(emitter, call),
        Leaf::Tasks => tasks::extract_in_tasks_block(emitter, call),
    }
}

/// Attempts a direct-head fact (include, task, project, apply-plugin); returns `true` if one was emitted.
fn try_direct(emitter: &mut Emitter, call: &CallExpr) -> bool {
    includes::try_extract(emitter, call)
        || tasks::try_extract_top_level(emitter, call)
        || plugins::try_extract_apply(emitter, call)
}

/// Records an `import` fact.
fn emit_import(emitter: &mut Emitter, path: &str, span: TextSpan) {
    let status = if path.is_empty() {
        FactStatus::Partial
    } else {
        FactStatus::Complete
    };
    emitter.push(path, None, span, status, FactPayload::Import(path.to_string()));
}
