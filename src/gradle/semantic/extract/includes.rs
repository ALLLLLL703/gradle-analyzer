//! Project-graph extraction: `include`, `project(...)`, and `rootProject.name`.
//!
//! `include ':app'` / `include(":app", ":core")` records one [`FactPayload::ProjectInclude`]
//! per project path argument. A `project(":core")` call (e.g. as a settings statement) records
//! a [`FactPayload::ProjectPath`]. The `rootProject.name = "..."` assignment records the
//! workspace name. These are the settings-script project-graph nucleus consumed by navigation.

use crate::gradle::semantic::facts::{FactPayload, FactStatus};

use super::super::view::{ArgExpr, AssignExpr, CallExpr};
use super::Emitter;

/// Recognizes a project-graph call (`include`, `project`); returns `true` if it emitted facts.
pub(super) fn try_extract(emitter: &mut Emitter, call: &CallExpr) -> bool {
    match call.head.as_str() {
        "include" => {
            extract_include(emitter, call);
            true
        }
        "project" => {
            extract_project(emitter, call);
            true
        }
        _ => false,
    }
}

/// Records one project-include fact per string path argument of an `include` call.
fn extract_include(emitter: &mut Emitter, call: &CallExpr) {
    let paths: Vec<String> = call
        .args
        .iter()
        .filter_map(|arg| match arg {
            ArgExpr::Str(path) => Some(path.clone()),
            _ => None,
        })
        .collect();

    if paths.is_empty() {
        emitter.push(
            "<unknown>",
            None,
            call.span,
            FactStatus::Partial,
            FactPayload::ProjectInclude(String::new()),
        );
        return;
    }
    for path in paths {
        emitter.push(
            &path.clone(),
            None,
            call.span,
            FactStatus::Complete,
            FactPayload::ProjectInclude(path),
        );
    }
}

/// Records a project-path fact from a `project(":core")` reference.
fn extract_project(emitter: &mut Emitter, call: &CallExpr) {
    let path = call.first_string().unwrap_or_default().to_string();
    let status = if path.is_empty() {
        FactStatus::Partial
    } else {
        FactStatus::Complete
    };
    let key = if path.is_empty() { "<unknown>".to_string() } else { path.clone() };
    emitter.push(
        &key,
        None,
        call.span,
        status,
        FactPayload::ProjectPath(path),
    );
}

/// Records `rootProject.name = "..."` from a settings assignment (no-op for other targets).
pub(super) fn extract_assignment(emitter: &mut Emitter, assign: &AssignExpr) {
    if assign.target != "rootProject.name" {
        return;
    }
    let name = match &assign.value {
        Some(ArgExpr::Str(name)) => name.clone(),
        _ => String::new(),
    };
    let status = if name.is_empty() {
        FactStatus::Partial
    } else {
        FactStatus::Complete
    };
    emitter.push(
        "rootProject.name",
        None,
        assign.span,
        status,
        FactPayload::RootProjectName(name),
    );
}
