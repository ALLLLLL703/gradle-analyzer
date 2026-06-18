//! Fact-driven hover: the smallest semantic fact at an offset → a [`HoverModel`].
//!
//! Scans this document's facts for the smallest source span containing the offset and renders
//! a summary from its payload. A dependency in `libs.*` notation reuses the catalog-resolution
//! message keys so a resolved accessor shows its coordinate; an unknown/partial coordinate
//! yields `None` so hover falls through to the block-keyword scan.

use crate::gradle::semantic::{
    CatalogResolution, DependencyCoordinate, FactPayload, SemanticDocument, SemanticFact,
};
use crate::i18n::MessageKey;

use super::HoverModel;

/// Returns the hover for the smallest fact whose source span contains `offset`, if any.
pub(super) fn hover_fact(semantics: &SemanticDocument, offset: usize) -> Option<HoverModel> {
    let fact = smallest_fact_at(semantics, offset)?;
    model_for(fact)
}

/// Finds the fact with the smallest source span containing `offset`.
fn smallest_fact_at(semantics: &SemanticDocument, offset: usize) -> Option<&SemanticFact> {
    semantics
        .facts()
        .iter()
        .filter(|fact| !fact.metadata.source.is_empty() && fact.metadata.source.contains(offset))
        .min_by_key(|fact| fact.metadata.source.len)
}

/// Renders a hover model for a fact, or `None` when the payload carries nothing to show.
fn model_for(fact: &SemanticFact) -> Option<HoverModel> {
    let span = fact.metadata.source;
    match &fact.payload {
        FactPayload::Dependency {
            configuration,
            coordinate,
        } => dependency_model(configuration, coordinate, span),
        FactPayload::Task { name, .. } => Some(HoverModel::new(
            MessageKey::HoverTask,
            vec![name.clone()],
            span,
        )),
        FactPayload::Plugin { id, .. } => Some(HoverModel::new(
            MessageKey::HoverPlugin,
            vec![id.clone()],
            span,
        )),
        _ => None,
    }
}

/// Builds the hover for a dependency, resolving the coordinate notation to display text.
fn dependency_model(
    configuration: &str,
    coordinate: &DependencyCoordinate,
    span: crate::gradle::syntax::TextSpan,
) -> Option<HoverModel> {
    match coordinate {
        DependencyCoordinate::StringNotation(coord) => Some(HoverModel::new(
            MessageKey::HoverDependency,
            vec![configuration.to_string(), coord.clone()],
            span,
        )),
        DependencyCoordinate::ProjectRef(path) => Some(HoverModel::new(
            MessageKey::HoverDependency,
            vec![configuration.to_string(), format!("project({path})")],
            span,
        )),
        DependencyCoordinate::CatalogAccessor {
            accessor,
            resolution,
        } => Some(catalog_model(accessor, resolution, span)),
        DependencyCoordinate::Unknown => None,
    }
}

/// Builds the hover for a `libs.*` accessor, reusing the catalog-resolution message keys.
fn catalog_model(
    accessor: &str,
    resolution: &CatalogResolution,
    span: crate::gradle::syntax::TextSpan,
) -> HoverModel {
    match resolution {
        CatalogResolution::Resolved { coordinate, .. } => HoverModel::new(
            MessageKey::SemanticCatalogResolved,
            vec![accessor.to_string(), coordinate.clone()],
            span,
        ),
        CatalogResolution::Unresolved => HoverModel::new(
            MessageKey::SemanticCatalogUnresolved,
            vec![accessor.to_string()],
            span,
        ),
    }
}
