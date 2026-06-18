//! Dependency extraction: configuration + coordinate, in every release-1 notation.
//!
//! A dependency is the call head (the configuration, e.g. `implementation`, `api`,
//! `testImplementation`) plus a [`DependencyCoordinate`] recovered from its first argument:
//! string notation (`"g:a:v"`), a version-catalog accessor (`libs.guava`, resolved against
//! the catalog), a project reference (`project(":core")`), or the Groovy map form
//! (`group: "g", name: "a", version: "v"`). An accessor with no catalog match is recorded
//! `Unresolved`; an unrecognized shape yields a `Partial` `Unknown` coordinate.

use crate::gradle::semantic::facts::{DependencyCoordinate, FactPayload, FactStatus};

use super::super::view::{ArgExpr, CallExpr};
use super::Emitter;

/// Extracts one dependency declaration found inside a `dependencies` block.
pub(super) fn extract_dependency(emitter: &mut Emitter, call: &CallExpr) {
    let configuration = call.head.clone();
    if configuration.is_empty() {
        return;
    }
    let (coordinate, status) = coordinate_of(emitter, call);
    let key = format!("{configuration}/{}", coordinate_key(&coordinate));
    emitter.push(
        &key,
        None,
        call.span,
        status,
        FactPayload::Dependency {
            configuration,
            coordinate,
        },
    );
}

/// Determines the coordinate notation and completeness from a dependency call's arguments.
fn coordinate_of(emitter: &Emitter, call: &CallExpr) -> (DependencyCoordinate, FactStatus) {
    if let Some(map) = map_notation(call) {
        return (DependencyCoordinate::StringNotation(map), FactStatus::Complete);
    }
    match call.args.first() {
        Some(ArgExpr::Str(coord)) => (
            DependencyCoordinate::StringNotation(coord.clone()),
            FactStatus::Complete,
        ),
        Some(ArgExpr::Path(accessor)) if is_catalog_accessor(accessor) => {
            let resolution = emitter.catalog().resolve_accessor(accessor);
            (
                DependencyCoordinate::CatalogAccessor {
                    accessor: accessor.clone(),
                    resolution,
                },
                FactStatus::Complete,
            )
        }
        Some(ArgExpr::Call(inner)) if inner.head == "project" => {
            let path = inner.first_string().unwrap_or_default().to_string();
            (DependencyCoordinate::ProjectRef(path), FactStatus::Complete)
        }
        _ => (DependencyCoordinate::Unknown, FactStatus::Partial),
    }
}

/// Builds a `g:a:v` string from the Groovy map form (`group:`, `name:`, `version:`).
fn map_notation(call: &CallExpr) -> Option<String> {
    let group = named_str(call, "group")?;
    let name = named_str(call, "name")?;
    match named_str(call, "version") {
        Some(version) => Some(format!("{group}:{name}:{version}")),
        None => Some(format!("{group}:{name}")),
    }
}

/// Returns the string value of a named argument, if present and a string.
fn named_str(call: &CallExpr, key: &str) -> Option<String> {
    match call.named(key) {
        Some(ArgExpr::Str(value)) => Some(value.clone()),
        _ => None,
    }
}

/// Returns `true` if a path accessor addresses the version catalog (`libs...`).
fn is_catalog_accessor(path: &str) -> bool {
    path == "libs" || path.starts_with("libs.")
}

/// Produces a stable id-key fragment for a coordinate (used in the [`crate::gradle::semantic::SemanticId`]).
fn coordinate_key(coordinate: &DependencyCoordinate) -> String {
    match coordinate {
        DependencyCoordinate::StringNotation(coord) => coord.clone(),
        DependencyCoordinate::CatalogAccessor { accessor, .. } => accessor.clone(),
        DependencyCoordinate::ProjectRef(path) => format!("project({path})"),
        DependencyCoordinate::Unknown => "<unknown>".to_string(),
    }
}
