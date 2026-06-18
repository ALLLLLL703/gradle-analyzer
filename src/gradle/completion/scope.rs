//! Layer 3a: [`VisibleScope`] — the locally-visible symbols pulled from the graph.
//!
//! The candidate builders need three families of workspace-derived names: task names,
//! project paths, and version-catalog accessors (plus buildSrc-contributed symbols). This
//! module gathers them ONCE from the [`SemanticGraph`] into deduplicated, deterministically
//! ordered lists so [`super::candidates`] stays a pure function of the scope + static tables.
//!
//! Catalog accessors are reconstructed from the catalog facts the graph already extracted
//! (`CatalogLibrary`/`CatalogBundle`/`CatalogPlugin`), so the engine reuses the resolved
//! catalog model rather than re-parsing TOML.

use crate::gradle::semantic::{FactPayload, SemanticGraph};

/// A version-catalog accessor a build script can type, with its resolved target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogAccessor {
    /// The full accessor path (e.g. `libs.guava`, `libs.bundles.networking`).
    pub accessor: String,
    /// The resolved coordinate / member list / plugin id (for the detail line).
    pub target: String,
}

/// The locally-visible symbols gathered from the semantic graph for completion.
///
/// Each list is deduplicated and sorted for deterministic output. Built once per
/// completion request via [`VisibleScope::gather`].
#[derive(Debug, Clone, Default)]
pub struct VisibleScope {
    /// Task names (declared tasks + buildSrc task symbols).
    pub task_names: Vec<String>,
    /// Project paths (`:app`, `:core`) from includes / project refs.
    pub project_paths: Vec<String>,
    /// Version-catalog accessors reconstructed from catalog facts.
    pub catalog_accessors: Vec<CatalogAccessor>,
    /// BuildSrc-contributed plugin ids (static visibility).
    pub buildsrc_plugins: Vec<String>,
}

impl VisibleScope {
    /// Gathers visible task names, project paths, and catalog accessors from `graph`.
    ///
    /// Iterates every fact across every document so a catalog opened as a separate file and
    /// tasks/includes declared anywhere in the workspace are all visible. Output lists are
    /// deduplicated and sorted.
    pub fn gather(graph: &SemanticGraph) -> VisibleScope {
        let mut scope = VisibleScope::default();
        for fact in graph.all_facts() {
            match &fact.payload {
                FactPayload::Task { name, .. } => scope.task_names.push(name.clone()),
                FactPayload::ProjectInclude(path) | FactPayload::ProjectPath(path) => {
                    scope.project_paths.push(path.clone())
                }
                FactPayload::CatalogLibrary { alias, coordinate } => {
                    scope.catalog_accessors.push(CatalogAccessor {
                        accessor: format!("libs.{alias}"),
                        target: coordinate.clone(),
                    });
                }
                FactPayload::CatalogBundle { alias, members } => {
                    scope.catalog_accessors.push(CatalogAccessor {
                        accessor: format!("libs.bundles.{alias}"),
                        target: members.join(", "),
                    });
                }
                FactPayload::CatalogPlugin { alias, id, .. } => {
                    scope.catalog_accessors.push(CatalogAccessor {
                        accessor: format!("libs.plugins.{alias}"),
                        target: id.clone(),
                    });
                }
                FactPayload::BuildSrcSymbol { name, symbol } => match symbol {
                    crate::gradle::semantic::BuildSrcSymbolKind::Task => {
                        scope.task_names.push(name.clone())
                    }
                    crate::gradle::semantic::BuildSrcSymbolKind::Plugin => {
                        scope.buildsrc_plugins.push(name.clone())
                    }
                },
                _ => {}
            }
        }
        scope.dedup_sort();
        scope
    }

    /// Deduplicates and sorts every list for deterministic candidate output.
    fn dedup_sort(&mut self) {
        dedup_sort(&mut self.task_names);
        dedup_sort(&mut self.project_paths);
        dedup_sort(&mut self.buildsrc_plugins);
        self.catalog_accessors.sort_by(|a, b| a.accessor.cmp(&b.accessor));
        self.catalog_accessors.dedup_by(|a, b| a.accessor == b.accessor);
    }
}

/// Sorts and removes adjacent duplicates from a string list.
fn dedup_sort(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}
