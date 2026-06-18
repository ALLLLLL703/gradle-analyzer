//! Safe, reversible, LOCAL code actions over the static analysis tier.
//!
//! This is Task 13's code-action half: a NARROW whitelist of quick-fixes, each carrying a
//! PROOF OBLIGATION so it is offered ONLY when its exact precondition holds. Every action is
//! reversible and local to one document — no cross-file edits, no semantic rewrites, and no
//! action in an ambiguous or multi-error context.
//!
//! The whitelist (each built in [`builders`]):
//!
//! 1. **Remove a duplicate declaration** — when a [`DiagnosticKind::DuplicateDeclaration`]
//!    diagnostic overlaps the request range; deletes exactly that declaration's line(s).
//! 2. **Insert one missing closing brace** — ONLY when the parser proves a SINGLE end-of-file
//!    `UnclosedBlock`/`MalformedBlock` (`parse.errors.len() == 1`); inserts one `}` at EOF.
//!    A multi-error malformed file fails the precondition and offers nothing.
//! 3. **Remove an unused import** — when a [`DiagnosticKind::UnusedImport`] diagnostic overlaps
//!    the request range; deletes exactly that import line, nothing else.
//! 4. **Modernize a deprecated configuration** — when a `Dependency` fact overlaps the request
//!    range and its configuration is a deprecated, officially 1:1-renamed Gradle configuration
//!    (`compile`→`implementation`, `runtime`→`runtimeOnly`, `testCompile`→`testImplementation`,
//!    `testRuntime`→`testRuntimeOnly`); replaces just that one head token.
//!
//! # LSP-type-free
//!
//! The public entry [`code_actions`] takes and returns only crate types. Each
//! [`CodeActionModel`] carries a [`MessageKey`] + args (so the title is localized at the
//! server boundary, never here) and a list of [`SpanEdit`]s over byte spans. The server
//! converts spans to `Range`s and the model to a `tower_lsp` `CodeAction` with a
//! `WorkspaceEdit`. Malformed input degrades to an EMPTY list and never panics.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::code_actions::code_actions;
//! use gradle_analyzer::gradle::diagnostics::compute_diagnostics;
//! use gradle_analyzer::gradle::parser::parse_kotlin;
//! use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput};
//! use gradle_analyzer::gradle::syntax::TextSpan;
//! use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
//! use tower_lsp::lsp_types::Url;
//!
//! let text = "dependencies {\n    implementation(\"a:b:1.0\")\n"; // single unclosed block
//! let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
//! let kind = GradleFileKind::RootBuildScript(DslLanguage::Kotlin);
//! let doc = TrackedDocument::new(uri, 1, text, kind);
//! let parse = parse_kotlin(text);
//! let input = SemanticInput::script("build.gradle.kts", text, kind);
//! let graph = analyze_documents(std::slice::from_ref(&input));
//! let semantics = graph.document(&input.id).unwrap();
//! let diags = compute_diagnostics(&doc, &parse, semantics);
//!
//! let actions = code_actions(&doc, &parse, &graph, &diags, TextSpan::new(0, text.len()));
//! assert!(actions.iter().any(|a| !a.edits.is_empty())); // the missing-brace fix
//! ```

pub mod builders;

#[cfg(test)]
mod tests;

use crate::gradle::diagnostics::Diagnostic;
use crate::gradle::semantic::{DocumentId, SemanticGraph};
use crate::gradle::syntax::{Parse, TextSpan};
use crate::gradle::workspace::TrackedDocument;

/// The LSP-free category of a code action, mapped to a `CodeActionKind` at the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeActionCategory {
    /// A fix that resolves a reported problem (maps to `CodeActionKind::QUICKFIX`).
    QuickFix,
    /// A neutral rewrite that does not resolve a problem (maps to `REFACTOR_REWRITE`).
    Rewrite,
}

/// A single local text replacement: replace `span` with `new_text`.
///
/// A zero-width `span` is an insertion; an empty `new_text` is a deletion. All edits in one
/// [`CodeActionModel`] target the requesting document and never overlap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanEdit {
    /// The byte range to replace (zero-width = insertion point).
    pub span: TextSpan,
    /// The replacement text (empty = deletion).
    pub new_text: String,
}

impl SpanEdit {
    /// Builds a replacement of `span` with `new_text`.
    pub fn new(span: TextSpan, new_text: impl Into<String>) -> SpanEdit {
        SpanEdit {
            span,
            new_text: new_text.into(),
        }
    }
}

/// One offered code action: a localizable title plus the local edits that apply it.
///
/// `title_key` + `title_args` render the user-facing title through a
/// [`crate::i18n::Translator`] at the server boundary; the model itself stays LSP-type-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeActionModel {
    /// The message key for the action's user-facing title.
    pub title_key: crate::i18n::MessageKey,
    /// Positional arguments substituted into the title template.
    pub title_args: Vec<String>,
    /// The action's category (drives the LSP `CodeActionKind`).
    pub category: CodeActionCategory,
    /// The local edits that apply this action (never cross-file).
    pub edits: Vec<SpanEdit>,
}

/// Computes the whitelisted, reversible local code actions available over `range`.
///
/// `doc` supplies the source text and DSL, `parse` the green tree + typed syntax errors,
/// `graph` the semantic facts (for the modernize-configuration action), and `diagnostics`
/// the already-computed findings for `doc`. Only actions whose exact precondition holds are
/// returned; an unrecognized (no-DSL) document, an ambiguous position, or a multi-error
/// malformed file yields an EMPTY vec. Never panics.
///
/// The result is LSP-type-free; the server boundary converts each [`CodeActionModel`] to a
/// `tower_lsp` `CodeAction`.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::code_actions::code_actions;
/// use gradle_analyzer::gradle::diagnostics::compute_diagnostics;
/// use gradle_analyzer::gradle::parser::parse_groovy;
/// use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput};
/// use gradle_analyzer::gradle::syntax::TextSpan;
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
/// use tower_lsp::lsp_types::Url;
///
/// let text = "import a.b.Unused\ntask build {}\n";
/// let uri = Url::from_file_path("/proj/build.gradle").unwrap();
/// let kind = GradleFileKind::RootBuildScript(DslLanguage::Groovy);
/// let doc = TrackedDocument::new(uri, 1, text, kind);
/// let parse = parse_groovy(text);
/// let input = SemanticInput::script("build.gradle", text, kind);
/// let graph = analyze_documents(std::slice::from_ref(&input));
/// let diags = compute_diagnostics(&doc, &parse, graph.document(&input.id).unwrap());
///
/// // Request over the import line offers exactly the remove-unused-import fix.
/// let actions = code_actions(&doc, &parse, &graph, &diags, TextSpan::new(0, 16));
/// assert!(actions.iter().any(|a| a.edits.iter().any(|e| e.new_text.is_empty())));
/// ```
pub fn code_actions(
    doc: &TrackedDocument,
    parse: &Parse,
    graph: &SemanticGraph,
    diagnostics: &[Diagnostic],
    range: TextSpan,
) -> Vec<CodeActionModel> {
    let span = tracing::trace_span!("code_actions.build", uri = %doc.uri(), range_start = range.start);
    let _enter = span.enter();

    if doc.kind().dsl().is_none() {
        return Vec::new();
    }

    let mut actions = Vec::new();
    builders::duplicate_removal(doc, diagnostics, range, &mut actions);
    builders::insert_missing_brace(doc, parse, &mut actions);
    builders::remove_unused_import(doc, diagnostics, range, &mut actions);
    builders::modernize_configuration(doc, graph, range, &mut actions);

    tracing::trace!(actions = actions.len(), "code actions built");
    actions
}

/// Returns `true` if `a` and `b` touch or overlap (treating zero-width spans as points).
///
/// Used to test a diagnostic/fact span against the request range. A point at exactly the
/// boundary counts as a hit so an editor selection adjacent to a finding still offers its fix.
pub(crate) fn spans_overlap(a: TextSpan, b: TextSpan) -> bool {
    a.start <= b.end() && b.start <= a.end()
}

/// Derives the file-name [`DocumentId`] the workspace graph keys this document under.
///
/// Mirrors the server's `completion::workspace_inputs` keying (by file name), so a fact
/// lookup against the graph finds this document's facts. Falls back to the full URI string.
pub(crate) fn document_id_for(doc: &TrackedDocument) -> DocumentId {
    doc.uri()
        .to_file_path()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .map(DocumentId::new)
        .unwrap_or_else(|| DocumentId::new(doc.uri().as_str()))
}

/// Expands `span` to cover the full source line(s) it touches, including a trailing newline.
///
/// Used by the deletion actions so removing a declaration or import leaves no blank-line
/// residue. Scans back to the previous line start and forward past the next newline; clamps
/// to the text bounds so the result is always a valid byte range.
pub(crate) fn full_line_span(text: &str, span: TextSpan) -> TextSpan {
    let start = text[..span.start.min(text.len())]
        .rfind('\n')
        .map(|nl| nl + 1)
        .unwrap_or(0);
    let after = span.end().min(text.len());
    let end = match text[after..].find('\n') {
        Some(nl) => after + nl + 1,
        None => text.len(),
    };
    TextSpan::from_range(start, end)
}
