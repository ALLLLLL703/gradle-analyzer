//! The LSP-type-free internal diagnostic model.
//!
//! [`compute_diagnostics`](super::compute_diagnostics) produces [`Diagnostic`]s in this
//! shape rather than `lsp_types::Diagnostic`, so the analysis layer stays free of protocol
//! types and the server boundary owns the single span→`Range` + key→text conversion. Task
//! 13 (code actions) and Task 16 (refinement) match on [`DiagnosticKind`] to attach fixes
//! or suppress without re-deriving anything.

use crate::gradle::syntax::TextSpan;
use crate::i18n::MessageKey;

/// How serious a diagnostic is, mapped to an `lsp_types::DiagnosticSeverity` at the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// A genuine error (malformed syntax that breaks parsing).
    Error,
    /// A likely mistake that does not break parsing (duplicate, unused, unresolved ref).
    Warning,
    /// A gentle suggestion.
    Hint,
    /// Neutral information.
    Information,
}

/// The family a [`Diagnostic`] belongs to, so later tasks dispatch on a stable tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// A typed parser syntax error (carries its originating [`SyntaxErrorKind`] meaning).
    ///
    /// [`SyntaxErrorKind`]: crate::gradle::syntax::SyntaxErrorKind
    Syntax,
    /// A uniquely-named declaration appeared more than once.
    DuplicateDeclaration,
    /// A `dependsOn` named a task with no local declaration (statically certain).
    UnresolvedTaskRef,
    /// An `import` is never referenced elsewhere in the file.
    UnusedImport,
}

/// One static finding: where it is, how serious, what message to render, and its family.
///
/// The message is addressed by [`MessageKey`] plus positional `args` (never raw English),
/// so the server renders it through the [`Translator`](crate::i18n::Translator) and the
/// surface stays localizable.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::diagnostics::{Diagnostic, DiagnosticKind, Severity};
/// use gradle_analyzer::gradle::syntax::TextSpan;
/// use gradle_analyzer::i18n::MessageKey;
///
/// let diag = Diagnostic::new(
///     TextSpan::new(0, 6),
///     Severity::Warning,
///     MessageKey::DiagnosticUnusedImport,
///     vec!["org.example.Foo".to_string()],
///     DiagnosticKind::UnusedImport,
/// );
/// assert_eq!(diag.kind, DiagnosticKind::UnusedImport);
/// assert_eq!(diag.args, ["org.example.Foo"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The byte span the diagnostic covers in the source.
    pub span: TextSpan,
    /// How serious the finding is.
    pub severity: Severity,
    /// The message-catalog key to render.
    pub message_key: MessageKey,
    /// Positional arguments substituted into the message template.
    pub args: Vec<String>,
    /// The family this diagnostic belongs to.
    pub kind: DiagnosticKind,
}

impl Diagnostic {
    /// Builds a diagnostic from its parts.
    pub fn new(
        span: TextSpan,
        severity: Severity,
        message_key: MessageKey,
        args: Vec<String>,
        kind: DiagnosticKind,
    ) -> Diagnostic {
        Diagnostic {
            span,
            severity,
            message_key,
            args,
            kind,
        }
    }
}
