//! Maps the parser's typed [`SyntaxError`] side table to [`Diagnostic`]s.
//!
//! Each [`SyntaxError`] becomes one diagnostic 1:1, reusing the substrate's
//! `SyntaxErrorKind -> MessageKey` mapping so no English is duplicated. Severity is assigned
//! per kind (structural breakage is an error; a typo or stray token is a warning). The
//! `KeywordTypo` and `UnexpectedToken` templates take the offending token text as `{0}`,
//! sliced from `source` by the error span.
//!
//! Suppression inside comments/strings/`OPAQUE` regions is free here: the tolerant parser
//! never records a typed error in an opaque region, so this pass has nothing to filter.

use crate::gradle::syntax::{Parse, SyntaxErrorKind};

use super::model::{Diagnostic, DiagnosticKind, Severity};

/// Builds one diagnostic per recorded syntax error.
pub(super) fn collect(parse: &Parse, source: &str) -> Vec<Diagnostic> {
    parse
        .errors
        .as_slice()
        .iter()
        .map(|error| {
            let args = template_args(error.kind, error.span.text(source));
            Diagnostic::new(
                error.span,
                severity_of(error.kind),
                error.message_key(),
                args,
                DiagnosticKind::Syntax,
            )
        })
        .collect()
}

/// Returns the severity for a syntax-error kind: breakage is an error, a near-miss a warning.
fn severity_of(kind: SyntaxErrorKind) -> Severity {
    match kind {
        SyntaxErrorKind::MissingEquals
        | SyntaxErrorKind::UnclosedBlock
        | SyntaxErrorKind::MalformedBlock
        | SyntaxErrorKind::UnterminatedString => Severity::Error,
        SyntaxErrorKind::KeywordTypo | SyntaxErrorKind::UnexpectedToken => Severity::Warning,
    }
}

/// Supplies the `{0}` token text for the kinds whose template references it; empty otherwise.
fn template_args(kind: SyntaxErrorKind, token_text: &str) -> Vec<String> {
    match kind {
        SyntaxErrorKind::KeywordTypo | SyntaxErrorKind::UnexpectedToken => {
            vec![token_text.trim().to_string()]
        }
        _ => Vec::new(),
    }
}
