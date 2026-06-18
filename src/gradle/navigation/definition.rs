//! Resolving a located [`Symbol`] to its definition target(s) against the semantic graph.
//!
//! The [`SemanticGraph`] is the single source of truth for *where a symbol is defined*: this
//! module never re-derives definitions from the tree, it only matches the located [`Symbol`]
//! against the facts the graph already extracted and returns their owning document + source
//! span as a [`NavTarget`]. Returning EMPTY when nothing matches is the confidence guarantee
//! (a `dependsOn("ghost")` with no declared `ghost` task yields no target — never a guess).
//!
//! # Task-15 seam
//!
//! Today every resolved target is a [`NavTarget::Local`] sourced from a graph fact. Task 15
//! adds an `External` branch here (a plugin-contributed type's source-jar location) without
//! changing the scanner or how local symbols match.

use crate::gradle::semantic::{FactPayload, SemanticGraph};

use super::locate::Symbol;
use super::{NavDocument, NavTarget};

/// Resolves `symbol` to every definition target the graph records, if any.
pub fn resolve_definition(
    _doc: &NavDocument,
    symbol: &Symbol,
    graph: &SemanticGraph,
) -> Vec<NavTarget> {
    match symbol {
        Symbol::Task(name) => resolve_task(name, graph),
        Symbol::CatalogAccessor(rest) => resolve_catalog(rest, graph),
        Symbol::Project(path) => resolve_project(path, graph),
    }
}

/// Resolves a task name to its declaration site(s), preferring `register`/`task` declarations.
///
/// When a task only appears as a `named(...)` configuration (no declaration in scope), those
/// configuration sites are returned so the position still navigates somewhere useful.
fn resolve_task(name: &str, graph: &SemanticGraph) -> Vec<NavTarget> {
    let mut declarations = Vec::new();
    let mut configurations = Vec::new();
    for document in graph.documents() {
        for fact in document.facts() {
            if let FactPayload::Task {
                name: task_name,
                registered,
            } = &fact.payload
                && task_name == name
            {
                let target = NavTarget::local(document.id().clone(), fact.metadata.source);
                if *registered {
                    declarations.push(target);
                } else {
                    configurations.push(target);
                }
            }
        }
    }
    if declarations.is_empty() {
        configurations
    } else {
        declarations
    }
}

/// Resolves a `libs.` accessor remainder to its catalog entry fact.
///
/// `rest` is the dotted remainder after `libs.`: a bare/dotted alias for a library, or a
/// `bundles.`/`plugins.`-prefixed alias. Matches the corresponding catalog fact by alias.
fn resolve_catalog(rest: &str, graph: &SemanticGraph) -> Vec<NavTarget> {
    let (wanted_kind, alias) = catalog_target(rest);
    let mut targets = Vec::new();
    for document in graph.documents() {
        for fact in document.facts() {
            if catalog_alias(&fact.payload).is_some_and(|(k, a)| k == wanted_kind && a == alias) {
                targets.push(NavTarget::local(document.id().clone(), fact.metadata.source));
            }
        }
    }
    targets
}

/// Which catalog table an accessor remainder targets, plus the alias to match.
fn catalog_target(rest: &str) -> (CatalogKind, &str) {
    if let Some(alias) = rest.strip_prefix("bundles.") {
        (CatalogKind::Bundle, alias)
    } else if let Some(alias) = rest.strip_prefix("plugins.") {
        (CatalogKind::Plugin, alias)
    } else {
        (CatalogKind::Library, rest)
    }
}

/// The catalog table a fact belongs to (for accessor matching).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogKind {
    Library,
    Bundle,
    Plugin,
}

/// Returns a catalog fact's table + alias, if it is a catalog entry.
fn catalog_alias(payload: &FactPayload) -> Option<(CatalogKind, &str)> {
    match payload {
        FactPayload::CatalogLibrary { alias, .. } => Some((CatalogKind::Library, alias)),
        FactPayload::CatalogBundle { alias, .. } => Some((CatalogKind::Bundle, alias)),
        FactPayload::CatalogPlugin { alias, .. } => Some((CatalogKind::Plugin, alias)),
        _ => None,
    }
}

/// Resolves a `:project:path` reference to the settings `include` that declares it.
fn resolve_project(path: &str, graph: &SemanticGraph) -> Vec<NavTarget> {
    let mut targets = Vec::new();
    for document in graph.documents() {
        for fact in document.facts() {
            if let FactPayload::ProjectInclude(included) = &fact.payload
                && included == path
            {
                targets.push(NavTarget::local(document.id().clone(), fact.metadata.source));
            }
        }
    }
    targets
}
