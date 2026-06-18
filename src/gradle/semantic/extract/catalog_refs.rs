//! Emits version-catalog entry facts and links libraries to the versions they reference.
//!
//! Given a parsed [`VersionCatalog`] for a `*.versions.toml` document, this pass records one
//! fact per entry: [`FactPayload::CatalogVersion`], [`FactPayload::CatalogLibrary`],
//! [`FactPayload::CatalogBundle`], and [`FactPayload::CatalogPlugin`]. A library or plugin
//! that references a `[versions]` alias gets its `parent_id` set to that version fact, so the
//! graph records the ownership a `version.ref` expresses. (Accessor RESOLUTION in build
//! scripts lives in [`super::deps`]; this pass only models the catalog file's own entries.)

use std::collections::BTreeMap;

use crate::gradle::semantic::facts::{FactPayload, FactStatus};
use crate::gradle::semantic::id::SemanticId;

use super::super::catalog::VersionCatalog;
use super::Emitter;

/// Records every entry of `catalog` as facts on `emitter`, linking version refs as parents.
pub(crate) fn extract_catalog(emitter: &mut Emitter, catalog: &VersionCatalog) {
    let version_ids = emit_versions(emitter, catalog);
    emit_libraries(emitter, catalog, &version_ids);
    emit_bundles(emitter, catalog);
    emit_plugins(emitter, catalog, &version_ids);
}

/// Emits `[versions]` facts, returning a map from version alias to its fact id (for parents).
fn emit_versions(emitter: &mut Emitter, catalog: &VersionCatalog) -> BTreeMap<String, SemanticId> {
    let mut ids = BTreeMap::new();
    for (alias, version) in catalog.versions() {
        let id = emitter.push(
            alias,
            None,
            ZERO_SPAN,
            FactStatus::Complete,
            FactPayload::CatalogVersion {
                alias: alias.clone(),
                version: version.clone(),
            },
        );
        ids.insert(alias.clone(), id);
    }
    ids
}

/// Emits `[libraries]` facts, parenting each to its referenced version fact when present.
fn emit_libraries(
    emitter: &mut Emitter,
    catalog: &VersionCatalog,
    version_ids: &BTreeMap<String, SemanticId>,
) {
    for (alias, coordinate) in catalog.libraries() {
        let parent = catalog
            .library_version_ref(alias)
            .and_then(|ref_alias| version_ids.get(ref_alias).cloned());
        emitter.push(
            alias,
            parent,
            ZERO_SPAN,
            FactStatus::Complete,
            FactPayload::CatalogLibrary {
                alias: alias.clone(),
                coordinate: coordinate.clone(),
            },
        );
    }
}

/// Emits `[bundles]` facts (alias + member aliases).
fn emit_bundles(emitter: &mut Emitter, catalog: &VersionCatalog) {
    for (alias, members) in catalog.bundles() {
        emitter.push(
            alias,
            None,
            ZERO_SPAN,
            FactStatus::Complete,
            FactPayload::CatalogBundle {
                alias: alias.clone(),
                members: members.clone(),
            },
        );
    }
}

/// Emits `[plugins]` facts, parenting each to its referenced version fact when present.
fn emit_plugins(
    emitter: &mut Emitter,
    catalog: &VersionCatalog,
    version_ids: &BTreeMap<String, SemanticId>,
) {
    for (alias, entry) in catalog.plugins() {
        let parent = catalog
            .plugin_version_ref(alias)
            .and_then(|ref_alias| version_ids.get(ref_alias).cloned());
        emitter.push(
            alias,
            parent,
            ZERO_SPAN,
            FactStatus::Complete,
            FactPayload::CatalogPlugin {
                alias: alias.clone(),
                id: entry.id.clone(),
                version: entry.version.clone(),
            },
        );
    }
}

/// Catalog entries are TOML key/values, not byte spans in a script; a zero span marks that.
const ZERO_SPAN: crate::gradle::syntax::TextSpan = crate::gradle::syntax::TextSpan::new(0, 0);
