//! Best-effort semantic refinement of the syntax-built outline.
//!
//! The [`super::builder`] produces a complete outline from syntax ALONE; this pass only
//! ENRICHES it using the Task 7 [`SemanticGraph`]. It never adds or removes symbols, so a
//! missing or partial graph degrades to a no-op rather than a worse outline. Currently it
//! attaches a resolved version-catalog coordinate to a dependency whose argument was a
//! `libs.*` accessor: the syntax pass labels the accessor verbatim, and this pass appends
//! the catalog coordinate it resolved to (`libs.guava → com.google.guava:guava:33.0.0-jre`).

use crate::gradle::semantic::{
    CatalogResolution, DependencyCoordinate, FactPayload, SemanticGraph,
};

use super::node::{OutlineKind, SymbolNode};

/// Refines `symbols` in place using resolved catalog accessors from `graph`.
pub fn refine(symbols: &mut [SymbolNode], graph: &SemanticGraph) {
    let resolutions = collect_accessor_resolutions(graph);
    if resolutions.is_empty() {
        return;
    }
    apply(symbols, &resolutions);
}

/// A resolved `(accessor, coordinate)` pair, e.g. `("libs.guava", "com.google.guava:guava:..")`.
type Resolution = (String, String);

/// Collects every resolved catalog accessor across all documents in the graph.
fn collect_accessor_resolutions(graph: &SemanticGraph) -> Vec<Resolution> {
    graph
        .all_facts()
        .filter_map(|fact| match &fact.payload {
            FactPayload::Dependency {
                coordinate:
                    DependencyCoordinate::CatalogAccessor {
                        accessor,
                        resolution: CatalogResolution::Resolved { coordinate, .. },
                    },
                ..
            } => Some((accessor.clone(), coordinate.clone())),
            _ => None,
        })
        .collect()
}

/// Walks the outline, appending resolved coordinates to matching dependency details.
fn apply(symbols: &mut [SymbolNode], resolutions: &[Resolution]) {
    for symbol in symbols.iter_mut() {
        if symbol.kind == OutlineKind::Dependency
            && let Some(detail) = &symbol.detail
            && let Some((_, coordinate)) =
                resolutions.iter().find(|(accessor, _)| accessor == detail)
        {
            symbol.detail = Some(format!("{detail} → {coordinate}"));
        }
        apply(&mut symbol.children, resolutions);
    }
}
