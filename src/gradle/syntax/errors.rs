//! The typed syntax-error side table.
//!
//! Diagnostics live OUTSIDE the green/red tree (rust-analyzer style): the tree stays a
//! pure, lossless description of the bytes, while every problem the lexer or parser
//! notices is recorded here as a [`SyntaxError`] carrying a [`SyntaxErrorKind`] and the
//! exact [`TextSpan`] it occurred at. Each kind maps to a [`MessageKey`] so the Task 9
//! diagnostics layer renders localized text — the substrate never formats user English.

use crate::i18n::MessageKey;

use super::span::TextSpan;

/// A closed classification of the problems this substrate can report.
///
/// The set is intentionally generic (no Gradle/Groovy/Kotlin specifics). Frontends reuse
/// these kinds; richer semantic diagnostics are layered on top in later tasks.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::SyntaxErrorKind;
/// use gradle_analyzer::i18n::MessageKey;
///
/// assert_eq!(
///     SyntaxErrorKind::UnclosedBlock.message_key(),
///     MessageKey::SyntaxUnclosedBlock,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SyntaxErrorKind {
    /// An assignment was expected to carry `=` but did not.
    MissingEquals,
    /// An identifier closely resembles an expected keyword.
    KeywordTypo,
    /// A block was opened but never closed before end of input.
    UnclosedBlock,
    /// A block opened and closed but its body did not parse.
    MalformedBlock,
    /// A string literal ran to end of line or input without a closing quote.
    UnterminatedString,
    /// A token appeared where the grammar did not expect one.
    UnexpectedToken,
}

impl SyntaxErrorKind {
    /// Returns the [`MessageKey`] used to render this error for a user.
    ///
    /// Keeping the mapping here is what lets the parser stay free of inline English: it
    /// records a kind, and the diagnostics layer localizes via this key.
    pub const fn message_key(self) -> MessageKey {
        match self {
            SyntaxErrorKind::MissingEquals => MessageKey::SyntaxMissingEquals,
            SyntaxErrorKind::KeywordTypo => MessageKey::SyntaxKeywordTypo,
            SyntaxErrorKind::UnclosedBlock => MessageKey::SyntaxUnclosedBlock,
            SyntaxErrorKind::MalformedBlock => MessageKey::SyntaxMalformedBlock,
            SyntaxErrorKind::UnterminatedString => MessageKey::SyntaxUnterminatedString,
            SyntaxErrorKind::UnexpectedToken => MessageKey::SyntaxUnexpectedToken,
        }
    }
}

/// One recorded syntax problem: a kind plus the span it covers.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{SyntaxError, SyntaxErrorKind, TextSpan};
///
/// let err = SyntaxError::new(SyntaxErrorKind::UnterminatedString, TextSpan::new(8, 4));
/// assert_eq!(err.span.start, 8);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxError {
    /// The classification of the problem.
    pub kind: SyntaxErrorKind,
    /// The byte range the problem covers.
    pub span: TextSpan,
}

impl SyntaxError {
    /// Builds a syntax error from a kind and span.
    pub const fn new(kind: SyntaxErrorKind, span: TextSpan) -> Self {
        Self { kind, span }
    }

    /// Returns the [`MessageKey`] for this error's kind.
    pub const fn message_key(self) -> MessageKey {
        self.kind.message_key()
    }
}

/// An append-only collection of [`SyntaxError`]s, kept beside the tree.
///
/// Errors are stored in the order they are discovered (roughly source order during a
/// single resilient parse), which is convenient for diagnostics that want stable output.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{SyntaxErrorKind, SyntaxErrors, TextSpan};
///
/// let mut errors = SyntaxErrors::new();
/// errors.push(SyntaxErrorKind::UnclosedBlock, TextSpan::new(3, 0));
/// assert_eq!(errors.len(), 1);
/// assert!(!errors.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyntaxErrors {
    items: Vec<SyntaxError>,
}

impl SyntaxErrors {
    /// Builds an empty error table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a new error from a kind and span.
    pub fn push(&mut self, kind: SyntaxErrorKind, span: TextSpan) {
        self.items.push(SyntaxError::new(kind, span));
    }

    /// Records an already-built [`SyntaxError`].
    pub fn push_error(&mut self, error: SyntaxError) {
        self.items.push(error);
    }

    /// Appends every error from `other`, draining it.
    pub fn extend(&mut self, other: &mut SyntaxErrors) {
        self.items.append(&mut other.items);
    }

    /// Returns the recorded errors in discovery order.
    pub fn as_slice(&self) -> &[SyntaxError] {
        &self.items
    }

    /// Returns the number of recorded errors.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if no errors were recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Consumes the table into its backing vector.
    pub fn into_vec(self) -> Vec<SyntaxError> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_maps_to_a_syntax_message_key() {
        let kinds = [
            SyntaxErrorKind::MissingEquals,
            SyntaxErrorKind::KeywordTypo,
            SyntaxErrorKind::UnclosedBlock,
            SyntaxErrorKind::MalformedBlock,
            SyntaxErrorKind::UnterminatedString,
            SyntaxErrorKind::UnexpectedToken,
        ];
        for kind in kinds {
            let name = kind.message_key().canonical_name();
            assert!(name.starts_with("syntax."), "{name} should be a syntax key");
        }
    }

    #[test]
    fn push_and_query_preserve_order() {
        let mut errors = SyntaxErrors::new();
        assert!(errors.is_empty());
        errors.push(SyntaxErrorKind::MissingEquals, TextSpan::new(1, 1));
        errors.push(SyntaxErrorKind::UnclosedBlock, TextSpan::new(5, 0));
        assert_eq!(errors.len(), 2);
        assert_eq!(errors.as_slice()[0].kind, SyntaxErrorKind::MissingEquals);
        assert_eq!(errors.as_slice()[1].kind, SyntaxErrorKind::UnclosedBlock);
    }

    #[test]
    fn extend_drains_other() {
        let mut a = SyntaxErrors::new();
        a.push(SyntaxErrorKind::MissingEquals, TextSpan::new(0, 1));
        let mut b = SyntaxErrors::new();
        b.push(SyntaxErrorKind::UnexpectedToken, TextSpan::new(2, 1));
        a.extend(&mut b);
        assert_eq!(a.len(), 2);
        assert!(b.is_empty());
    }
}
