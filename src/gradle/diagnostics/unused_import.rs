//! The conservative unused-import warning.
//!
//! For each `import` fact, the final dotted segment (`Bar` of `a.b.Bar`) is the imported
//! symbol. The pass scans every `IDENT` token in the file that lies OUTSIDE any import
//! statement and outside `OPAQUE`/`ERROR_NODE` subtrees; if the symbol never appears among
//! them, the import is flagged. Only `IDENT` code tokens count as references, so a symbol
//! that occurs only inside a string or comment does not rescue the import.
//!
//! Conservatism is deliberate — the pass NEVER flags what it cannot statically prove unused:
//! a wildcard import (`a.b.*`, whose `*` the fact path drops) is skipped, and a symbol used
//! anywhere as a real code identifier (e.g. a `tasks.register<Test>` type argument) keeps its
//! import live.

use std::collections::HashSet;

use crate::gradle::semantic::{FactPayload, SemanticDocument, SemanticFactKind};
use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, TextSpan};
use crate::i18n::MessageKey;

use super::model::{Diagnostic, DiagnosticKind, Severity};

/// Flags each import whose final segment is never referenced as a code identifier.
pub(super) fn collect(root: &SyntaxNode, source: &str, semantics: &SemanticDocument) -> Vec<Diagnostic> {
    let import_spans: Vec<TextSpan> = semantics
        .facts_of_kind(SemanticFactKind::Import)
        .map(|fact| fact.metadata.source)
        .collect();
    if import_spans.is_empty() {
        return Vec::new();
    }

    let references = reference_identifiers(root, &import_spans);

    semantics
        .facts_of_kind(SemanticFactKind::Import)
        .filter_map(|fact| {
            let FactPayload::Import(path) = &fact.payload else {
                return None;
            };
            let symbol = final_segment(path)?;
            if is_wildcard(source, fact.metadata.source) || references.contains(symbol) {
                return None;
            }
            Some(Diagnostic::new(
                fact.metadata.source,
                Severity::Warning,
                MessageKey::DiagnosticUnusedImport,
                vec![path.clone()],
                DiagnosticKind::UnusedImport,
            ))
        })
        .collect()
}

/// Returns the trailing dotted segment of `path`, or `None` if it is empty/`*`.
fn final_segment(path: &str) -> Option<&str> {
    let segment = path.rsplit('.').next().unwrap_or(path);
    if segment.is_empty() || segment == "*" {
        None
    } else {
        Some(segment)
    }
}

/// Returns `true` if the import's source text ends in a `.*` wildcard.
fn is_wildcard(source: &str, span: TextSpan) -> bool {
    span.text(source).contains('*')
}

/// Collects every `IDENT` token text outside an import span and outside opaque subtrees.
fn reference_identifiers(root: &SyntaxNode, import_spans: &[TextSpan]) -> HashSet<String> {
    let mut idents = HashSet::new();
    walk(root, import_spans, &mut idents);
    idents
}

/// Recursively gathers reference identifiers, pruning `OPAQUE`/`ERROR_NODE` subtrees.
fn walk(node: &SyntaxNode, import_spans: &[TextSpan], idents: &mut HashSet<String>) {
    for child in node.children() {
        match child {
            SyntaxElement::Node(inner) => {
                if is_opaque(inner.kind()) {
                    continue;
                }
                walk(inner, import_spans, idents);
            }
            SyntaxElement::Token(token) => {
                if token.kind() == SyntaxKind::IDENT && !within_any(token.span(), import_spans) {
                    idents.insert(token.text().to_string());
                }
            }
        }
    }
}

/// Returns `true` for subtree roots whose contents are not statically meaningful code.
fn is_opaque(kind: SyntaxKind) -> bool {
    kind == SyntaxKind::OPAQUE || kind == SyntaxKind::ERROR_NODE
}

/// Returns `true` if `span` starts within any of the import spans.
fn within_any(span: TextSpan, import_spans: &[TextSpan]) -> bool {
    import_spans
        .iter()
        .any(|import| span.start >= import.start && span.start < import.end())
}
