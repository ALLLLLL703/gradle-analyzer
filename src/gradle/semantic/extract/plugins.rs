//! Plugin extraction: `plugins { }` / `pluginManagement { }` declarations and `apply plugin:`.
//!
//! A plugin id is recovered from the several shapes Gradle allows: `id("x")` / `id 'x'`
//! (id is the first string arg), `kotlin("jvm")` (mapped to `org.jetbrains.kotlin.jvm`), a
//! bare accessor (`` `kotlin-dsl` ``, `application` — id is the head itself), and the legacy
//! `apply plugin: "x"`. Kotlin `version "x"`/`apply false` infix suffixes fold onto the fact.
//! A declaration with no recoverable id still yields a `Partial` plugin fact.

use crate::gradle::semantic::facts::{FactPayload, FactStatus};

use super::super::view::{ArgExpr, CallExpr};
use super::Emitter;

/// Extracts one plugin declaration found inside a `plugins`/`pluginManagement` block.
pub(super) fn extract_plugin(emitter: &mut Emitter, call: &CallExpr) {
    let id = plugin_id(call);
    let version = suffix_value(call, "version");
    let apply = suffix_value(call, "apply").map(|v| v != "false").unwrap_or(true);
    let status = if id.is_empty() {
        FactStatus::Partial
    } else {
        FactStatus::Complete
    };
    let key = if id.is_empty() { "<unknown>".to_string() } else { id.clone() };
    emitter.push(
        &key,
        None,
        call.span,
        status,
        FactPayload::Plugin { id, version, apply },
    );
}

/// Recognizes the legacy `apply plugin: "x"` form; returns `true` if it emitted a plugin.
pub(super) fn try_extract_apply(emitter: &mut Emitter, call: &CallExpr) -> bool {
    if call.head != "apply" {
        return false;
    }
    let Some(ArgExpr::Str(id)) = call.named("plugin") else {
        return false;
    };
    let id = id.clone();
    let key = id.clone();
    emitter.push(
        &key,
        None,
        call.span,
        FactStatus::Complete,
        FactPayload::Plugin {
            id,
            version: None,
            apply: true,
        },
    );
    true
}

/// Determines the plugin id from a `plugins {}` call's head and first string argument.
fn plugin_id(call: &CallExpr) -> String {
    match call.head.as_str() {
        "id" | "alias" => call.first_string().unwrap_or_default().to_string(),
        "kotlin" => match call.first_string() {
            Some(module) => format!("org.jetbrains.kotlin.{module}"),
            None => "org.jetbrains.kotlin".to_string(),
        },
        // A bare accessor like `application` / `` `kotlin-dsl` `` is the id itself.
        _ if call.args.is_empty() => call.head_raw.clone(),
        _ => call.first_string().unwrap_or_default().to_string(),
    }
}

/// Returns the value of a Kotlin plugin infix suffix keyword (`version`, `apply`).
fn suffix_value(call: &CallExpr, keyword: &str) -> Option<String> {
    call.suffixes
        .iter()
        .find(|s| s.keyword == keyword)
        .and_then(|s| s.value.clone())
}
