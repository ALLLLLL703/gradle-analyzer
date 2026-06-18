//! Static diagnostics: syntax errors, duplicate declarations, unresolved local task refs,
//! and a conservative unused-import warning, mapped to a localizable, LSP-free model.
//!
//! [`compute_diagnostics`] is the single public entry. It consumes already-computed inputs
//! (a [`TrackedDocument`], its [`Parse`], and its [`SemanticDocument`]) and fans out to four
//! focused families, each in its own submodule:
//!
//! - [`syntax`] — the parser's typed error side table, mapped 1:1 (reusing the substrate's
//!   `SyntaxErrorKind -> MessageKey`).
//! - [`semantic`] — duplicate registered-task names and statically-certain unresolved local
//!   `dependsOn` references.
//! - [`unused_import`] — an `import` whose final segment is never referenced elsewhere.
//!
//! Suppression inside comments/strings/`OPAQUE` regions is structural: the tolerant parser
//! never records a typed error in an opaque region, and the semantic view skips
//! `OPAQUE`/`ERROR_NODE` subtrees, so no family ever sees those regions as code. The result
//! [`Diagnostic`] is LSP-type-free; the server boundary converts spans to ranges and renders
//! message keys. Pure: no IO, no clock, no config read (the enable/disable gate lives at the
//! server). Does NOT diagnose plugin-derived/unknown members (Task 16).

mod model;
mod semantic;
mod syntax;
mod unused_import;

pub use model::{Diagnostic, DiagnosticKind, Severity};

use crate::gradle::semantic::SemanticDocument;
use crate::gradle::syntax::{Parse, SyntaxNode};
use crate::gradle::workspace::TrackedDocument;

/// Computes every static diagnostic for one document.
///
/// `doc` supplies the source text and classified kind (which DSL, or none for a catalog),
/// `parse` supplies the green tree + typed syntax errors, and `semantics` supplies this
/// file's extracted facts (obtain it via `graph.document(&id)`). The returned diagnostics
/// are ordered syntax-first, then semantic, then unused-import; an unrecognized or
/// catalog-only document yields an empty vector.
///
/// Never panics on malformed input: a broken file still yields diagnostics (degrading to
/// whatever the tolerant parser and partial facts expose), never a crash.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::diagnostics::compute_diagnostics;
/// use gradle_analyzer::gradle::parser::parse_kotlin;
/// use gradle_analyzer::gradle::semantic::{analyze_documents, SemanticInput};
/// use gradle_analyzer::gradle::workspace::{DslLanguage, GradleFileKind, TrackedDocument};
/// use tower_lsp::lsp_types::Url;
///
/// let text = "dependencies {\n    implementation(\"a:b:1.0\")\n";
/// let uri = Url::from_file_path("/proj/build.gradle.kts").unwrap();
/// let doc = TrackedDocument::new(
///     uri,
///     1,
///     text,
///     GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
/// );
/// let parse = parse_kotlin(text);
/// let input = SemanticInput::script(
///     "build.gradle.kts",
///     text,
///     GradleFileKind::RootBuildScript(DslLanguage::Kotlin),
/// );
/// let graph = analyze_documents(std::slice::from_ref(&input));
/// let semantics = graph.document(&input.id).unwrap();
///
/// let diags = compute_diagnostics(&doc, &parse, semantics);
/// // The unclosed `dependencies {` block surfaces one syntax diagnostic.
/// assert!(!diags.is_empty());
/// ```
pub fn compute_diagnostics(
    doc: &TrackedDocument,
    parse: &Parse,
    semantics: &SemanticDocument,
) -> Vec<Diagnostic> {
    let span = tracing::debug_span!("diagnostics.compute", uri = %doc.uri());
    let _enter = span.enter();

    let Some(language) = doc.kind().dsl() else {
        return Vec::new();
    };

    let root = SyntaxNode::new_root(parse.green.clone());
    let source = doc.text();

    let mut diagnostics = Vec::new();
    diagnostics.extend(syntax::collect(parse, source));
    diagnostics.extend(semantic::collect(&root, language, semantics));
    diagnostics.extend(unused_import::collect(&root, source, semantics));

    tracing::debug!(count = diagnostics.len(), "diagnostics computed");
    diagnostics
}

#[cfg(test)]
mod tests;
