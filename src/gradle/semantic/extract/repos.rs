//! Repository extraction: `repositories { }` declarations.
//!
//! A repository is the call's head name (`mavenCentral`, `google`, `gradlePluginPortal`,
//! `mavenLocal`, or a custom `maven { }`). For a `maven { url = "..." }` / `maven { url ... }`
//! block, the URL is recovered from a `url` assignment or a `url(...)`/`setUrl(...)` call
//! inside the block when present; otherwise the fact is still emitted without a URL.

use crate::gradle::semantic::facts::{FactPayload, FactStatus};
use crate::gradle::workspace::DslLanguage;

use super::super::view::{self, ArgExpr, Statement};
use super::super::view::CallExpr;
use super::Emitter;

/// Extracts one repository declaration found inside a `repositories` block.
pub(super) fn extract_repository(emitter: &mut Emitter, call: &CallExpr) {
    let name = call.head.clone();
    if name.is_empty() {
        return;
    }
    let url = call.block.as_ref().and_then(repository_url);
    let key = name.clone();
    emitter.push(
        &key,
        None,
        call.span,
        FactStatus::Complete,
        FactPayload::Repository { name, url },
    );
}

/// Finds a `url` value inside a `maven { }` block (a `url = "..."` assign or `url("...")` call).
fn repository_url(block: &std::rc::Rc<crate::gradle::syntax::SyntaxNode>) -> Option<String> {
    for lang in [DslLanguage::Kotlin, DslLanguage::Groovy] {
        for statement in view::child_statements(block, lang) {
            match statement {
                Statement::Assignment(assign) if assign.target == "url" => {
                    if let Some(ArgExpr::Str(url)) = assign.value {
                        return Some(url);
                    }
                }
                Statement::Call(inner) if matches!(inner.head.as_str(), "url" | "setUrl") => {
                    if let Some(url) = inner.first_string() {
                        return Some(url.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}
