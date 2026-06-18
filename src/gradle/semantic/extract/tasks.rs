//! Task extraction: registrations and configurations in every release-1 form.
//!
//! Tasks surface as `task foo { }` (Groovy: head `task`, the name is the first bare arg),
//! `tasks.register("x")` / `tasks.register<T>("x")` and `tasks.named("y")` at top level, and
//! `register("x")` / `named("y")` inside a `tasks { }` container block. `register`/`task`
//! mark a declaration (`registered = true`); `named` marks a configuration of an existing task.

use crate::gradle::semantic::facts::{FactPayload, FactStatus};

use super::super::view::{ArgExpr, CallExpr};
use super::Emitter;

/// Recognizes a top-level task form (`task foo`, `tasks.register`, `tasks.named`).
///
/// Returns `true` if a task fact was emitted, so the driver does not also recurse the block.
pub(super) fn try_extract_top_level(emitter: &mut Emitter, call: &CallExpr) -> bool {
    match call.head.as_str() {
        "task" => emit_named_task(emitter, call, first_name(call), true),
        "tasks.register" => emit_named_task(emitter, call, call.first_string().map(str::to_string), true),
        "tasks.named" => emit_named_task(emitter, call, call.first_string().map(str::to_string), false),
        _ => return false,
    }
    true
}

/// Extracts a `register`/`named` call found inside a `tasks { }` container block.
pub(super) fn extract_in_tasks_block(emitter: &mut Emitter, call: &CallExpr) {
    match call.head.as_str() {
        "register" => emit_named_task(emitter, call, call.first_string().map(str::to_string), true),
        "named" => emit_named_task(emitter, call, call.first_string().map(str::to_string), false),
        _ => {}
    }
}

/// Emits one task fact, marking it `Partial` when the name could not be recovered.
fn emit_named_task(emitter: &mut Emitter, call: &CallExpr, name: Option<String>, registered: bool) {
    let (name, status) = match name {
        Some(name) if !name.is_empty() => (name, FactStatus::Complete),
        _ => ("<unknown>".to_string(), FactStatus::Partial),
    };
    let key = name.clone();
    emitter.push(
        &key,
        None,
        call.span,
        status,
        FactPayload::Task { name, registered },
    );
}

/// Returns the first arg as a task name (Groovy `task hello` puts the name as a bare path).
fn first_name(call: &CallExpr) -> Option<String> {
    call.args.first().and_then(|arg| match arg {
        ArgExpr::Str(name) | ArgExpr::Path(name) => Some(name.clone()),
        _ => None,
    })
}
