//! The four whitelisted code-action builders, each gated on an exact precondition.
//!
//! Every builder pushes AT MOST one [`CodeActionModel`] and only when its proof obligation
//! holds, so an ambiguous or unsupported context simply produces nothing. None of these
//! edits crosses a file boundary or rewrites semantics: they delete a line, insert one brace,
//! or rename one deprecated configuration token.

use crate::gradle::diagnostics::{Diagnostic, DiagnosticKind};
use crate::gradle::parser::{groovy as gv, kotlin::kinds as kt};
use crate::gradle::semantic::{FactPayload, SemanticFactKind, SemanticGraph};
use crate::gradle::syntax::{Parse, SyntaxElement, SyntaxErrorKind, SyntaxKind, SyntaxNode, TextSpan};
use crate::gradle::workspace::{DslLanguage, TrackedDocument};
use crate::i18n::MessageKey;

use super::{
    CodeActionCategory, CodeActionModel, SpanEdit, document_id_for, full_line_span, spans_overlap,
};

/// Deprecated Gradle configurations with an official, unambiguous 1:1 replacement.
///
/// Sourced from Gradle's upgrade guide (removed-configuration replacement table). Only
/// configurations whose successor is exact and behavior-preserving at the declaration site
/// are listed, so the rewrite stays a safe local token rename.
const DEPRECATED_CONFIGURATIONS: &[(&str, &str)] = &[
    ("compile", "implementation"),
    ("runtime", "runtimeOnly"),
    ("testCompile", "testImplementation"),
    ("testRuntime", "testRuntimeOnly"),
];

/// Offers removing a duplicate declaration when such a diagnostic overlaps `range`.
pub(super) fn duplicate_removal(
    doc: &TrackedDocument,
    diagnostics: &[Diagnostic],
    range: TextSpan,
    out: &mut Vec<CodeActionModel>,
) {
    let text = doc.text();
    for diag in diagnostics {
        if diag.kind != DiagnosticKind::DuplicateDeclaration || !spans_overlap(diag.span, range) {
            continue;
        }
        let name = diag.args.first().cloned().unwrap_or_default();
        out.push(CodeActionModel {
            title_key: MessageKey::CodeActionRemoveDuplicate,
            title_args: vec![name],
            category: CodeActionCategory::QuickFix,
            edits: vec![SpanEdit::new(full_line_span(text, diag.span), "")],
        });
    }
}

/// Offers inserting one closing brace ONLY when the parser proves a single EOF unclosed block.
///
/// The proof obligation is `parse.errors.len() == 1` and that one error is an
/// `UnclosedBlock`/`MalformedBlock`. A file with more than one syntax error fails the
/// precondition (a multi-error malformed file is ambiguous), so nothing is offered.
pub(super) fn insert_missing_brace(
    doc: &TrackedDocument,
    parse: &Parse,
    out: &mut Vec<CodeActionModel>,
) {
    let errors = parse.errors.as_slice();
    if errors.len() != 1 {
        return;
    }
    let only = errors[0];
    if !matches!(
        only.kind,
        SyntaxErrorKind::UnclosedBlock | SyntaxErrorKind::MalformedBlock
    ) {
        return;
    }
    let eof = doc.text().len();
    out.push(CodeActionModel {
        title_key: MessageKey::CodeActionInsertClosingBrace,
        title_args: Vec::new(),
        category: CodeActionCategory::QuickFix,
        edits: vec![SpanEdit::new(TextSpan::empty_at(eof), "}")],
    });
}

/// Offers removing an unused import when such a diagnostic overlaps `range`.
///
/// Deletes exactly the import's line (nothing else), so the edit is trivially reversible.
pub(super) fn remove_unused_import(
    doc: &TrackedDocument,
    diagnostics: &[Diagnostic],
    range: TextSpan,
    out: &mut Vec<CodeActionModel>,
) {
    let text = doc.text();
    for diag in diagnostics {
        if diag.kind != DiagnosticKind::UnusedImport || !spans_overlap(diag.span, range) {
            continue;
        }
        let path = diag.args.first().cloned().unwrap_or_default();
        out.push(CodeActionModel {
            title_key: MessageKey::CodeActionRemoveUnusedImport,
            title_args: vec![path],
            category: CodeActionCategory::QuickFix,
            edits: vec![SpanEdit::new(full_line_span(text, diag.span), "")],
        });
    }
}

/// Offers renaming a deprecated dependency configuration to its modern 1:1 successor.
///
/// Finds the `Dependency` fact overlapping `range` whose configuration is deprecated, locates
/// that configuration's head token in the source via a tolerant red-tree scan, and replaces
/// just that token. No argument or block body is touched.
pub(super) fn modernize_configuration(
    doc: &TrackedDocument,
    graph: &SemanticGraph,
    range: TextSpan,
    out: &mut Vec<CodeActionModel>,
) {
    let Some(language) = doc.kind().dsl() else {
        return;
    };
    let document = document_id_for(doc);
    let Some(semantics) = graph.document(&document) else {
        return;
    };

    for fact in semantics.facts_of_kind(SemanticFactKind::Dependency) {
        if !spans_overlap(fact.metadata.source, range) {
            continue;
        }
        let FactPayload::Dependency { configuration, .. } = &fact.payload else {
            continue;
        };
        let Some(modern) = modern_replacement(configuration) else {
            continue;
        };
        let Some(head_span) = locate_config_head(parse_root(doc), language, fact.metadata.source, configuration)
        else {
            continue;
        };
        out.push(CodeActionModel {
            title_key: MessageKey::CodeActionModernizeConfiguration,
            title_args: vec![configuration.clone(), modern.to_string()],
            category: CodeActionCategory::Rewrite,
            edits: vec![SpanEdit::new(head_span, modern)],
        });
    }
}

/// Returns the modern successor for a deprecated configuration name, if one exists.
fn modern_replacement(configuration: &str) -> Option<&'static str> {
    DEPRECATED_CONFIGURATIONS
        .iter()
        .find(|(old, _)| *old == configuration)
        .map(|(_, new)| *new)
}

/// Parses `doc` for its DSL and returns the red-tree root used by the head-token scan.
fn parse_root(doc: &TrackedDocument) -> SyntaxNode {
    let text = doc.text();
    let parse = match doc.kind().dsl() {
        Some(DslLanguage::Kotlin) => crate::gradle::parser::parse_kotlin(text),
        _ => crate::gradle::parser::parse_groovy(text),
    };
    SyntaxNode::new_root(parse.green).as_ref().clone()
}

/// Finds the precise span of the `configuration` head IDENT inside the dependency call.
///
/// Scans the call node whose span matches `call_span` for the first IDENT token equal to
/// `configuration`, returning that token's span. Returns `None` if the call or token is not
/// found (e.g. an opaque region), so the action is simply not offered.
fn locate_config_head(
    root: SyntaxNode,
    language: DslLanguage,
    call_span: TextSpan,
    configuration: &str,
) -> Option<TextSpan> {
    let call_kind = match language {
        DslLanguage::Kotlin => kt::CALL,
        DslLanguage::Groovy => gv::CALL,
    };
    find_head_in(&root, call_kind, call_span, configuration)
}

/// Recursively searches for the matching call's first head IDENT token.
fn find_head_in(
    node: &SyntaxNode,
    call_kind: SyntaxKind,
    call_span: TextSpan,
    configuration: &str,
) -> Option<TextSpan> {
    for child in node.child_nodes() {
        if child.kind() == call_kind
            && child.span() == call_span
            && let Some(span) = first_ident(&child, configuration)
        {
            return Some(span);
        }
        if let Some(found) = find_head_in(&child, call_kind, call_span, configuration) {
            return Some(found);
        }
    }
    None
}

/// Returns the span of the first descendant IDENT token whose text equals `name`.
fn first_ident(node: &SyntaxNode, name: &str) -> Option<TextSpan> {
    for child in node.children() {
        match child {
            SyntaxElement::Token(token)
                if token.kind() == SyntaxKind::IDENT && token.text() == name =>
            {
                return Some(token.span());
            }
            SyntaxElement::Node(inner) => {
                if let Some(span) = first_ident(inner, name) {
                    return Some(span);
                }
            }
            _ => {}
        }
    }
    None
}
