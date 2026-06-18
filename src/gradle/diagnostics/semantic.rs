//! Semantic diagnostics derived from facts and the DSL-agnostic statement view:
//! duplicate registered-task declarations and statically-certain unresolved local
//! `dependsOn` task references.
//!
//! Duplicate detection groups `Task { registered: true }` facts by name and flags the
//! second and later occurrences. Unresolved-reference detection walks the file's statement
//! view (recursing into call blocks) for `dependsOn("name")` calls and flags a string target
//! that is neither a locally declared task nor a well-known Gradle lifecycle task — so a
//! valid reference and a built-in target both stay silent. Both passes are conservative:
//! only string-literal targets are considered, and `OPAQUE`/`ERROR_NODE` regions never reach
//! the view, so a `dependsOn` inside a comment/string is never analyzed.

use std::collections::HashSet;

use crate::gradle::semantic::{FactPayload, SemanticDocument, SemanticFactKind};
use crate::gradle::semantic::view::{child_statements, ArgExpr, CallExpr, Statement};
use crate::gradle::syntax::SyntaxNode;
use crate::gradle::workspace::DslLanguage;
use crate::i18n::MessageKey;

use super::model::{Diagnostic, DiagnosticKind, Severity};

/// Gradle lifecycle tasks that are always present, so a `dependsOn` on them is never a
/// statically-certain error even without a local declaration.
const BUILTIN_TASKS: &[&str] = &[
    "build",
    "assemble",
    "check",
    "test",
    "clean",
    "jar",
    "classes",
    "compileJava",
    "processResources",
    "javadoc",
    "publishToMavenLocal",
];

/// Collects duplicate-declaration and unresolved-local-task-reference diagnostics.
pub(super) fn collect(
    root: &SyntaxNode,
    language: DslLanguage,
    semantics: &SemanticDocument,
) -> Vec<Diagnostic> {
    let mut diagnostics = duplicate_tasks(semantics);
    diagnostics.extend(unresolved_depends_on(root, language, semantics));
    diagnostics
}

/// Flags the second and later declarations of any registered task name.
fn duplicate_tasks(semantics: &SemanticDocument) -> Vec<Diagnostic> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut diagnostics = Vec::new();
    for fact in semantics.facts_of_kind(SemanticFactKind::Task) {
        let FactPayload::Task { name, registered } = &fact.payload else {
            continue;
        };
        if !registered {
            continue;
        }
        if !seen.insert(name.as_str()) {
            diagnostics.push(Diagnostic::new(
                fact.metadata.source,
                Severity::Warning,
                MessageKey::DiagnosticDuplicateDeclaration,
                vec![name.clone()],
                DiagnosticKind::DuplicateDeclaration,
            ));
        }
    }
    diagnostics
}

/// Flags `dependsOn("x")` whose target is neither a declared local task nor a built-in.
fn unresolved_depends_on(
    root: &SyntaxNode,
    language: DslLanguage,
    semantics: &SemanticDocument,
) -> Vec<Diagnostic> {
    let declared = declared_task_names(semantics);
    let mut diagnostics = Vec::new();
    collect_depends_on(root, language, &declared, &mut diagnostics);
    diagnostics
}

/// Returns the set of task names declared or configured anywhere in this file.
fn declared_task_names(semantics: &SemanticDocument) -> HashSet<String> {
    semantics
        .facts_of_kind(SemanticFactKind::Task)
        .filter_map(|fact| match &fact.payload {
            FactPayload::Task { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// Walks `node`'s statement view, recursing into call blocks, emitting unresolved-ref diags.
fn collect_depends_on(
    node: &SyntaxNode,
    language: DslLanguage,
    declared: &HashSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    for statement in child_statements(node, language) {
        let Statement::Call(call) = statement else {
            continue;
        };
        if call.head == "dependsOn"
            && let Some(diag) = depends_on_diagnostic(&call, declared)
        {
            out.push(diag);
        }
        if let Some(block) = &call.block {
            collect_depends_on(block, language, declared, out);
        }
    }
}

/// Builds a diagnostic for a single `dependsOn` call if its string target is unresolved.
fn depends_on_diagnostic(call: &CallExpr, declared: &HashSet<String>) -> Option<Diagnostic> {
    let target = string_target(call)?;
    if declared.contains(target) || BUILTIN_TASKS.contains(&target) {
        return None;
    }
    Some(Diagnostic::new(
        call.span,
        Severity::Warning,
        MessageKey::DiagnosticUnresolvedTaskRef,
        vec![target.to_string()],
        DiagnosticKind::UnresolvedTaskRef,
    ))
}

/// Returns the sole string-literal target of a `dependsOn` call, if it has exactly that
/// shape; a non-string target (a `tasks.named(..)`/identifier) is not statically certain.
fn string_target(call: &CallExpr) -> Option<&str> {
    match call.args.as_slice() {
        [ArgExpr::Str(name)] => Some(name.as_str()),
        _ => None,
    }
}
