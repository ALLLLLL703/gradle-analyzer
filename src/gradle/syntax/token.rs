//! The open [`SyntaxKind`] tag space and the lossless [`Token`] type.
//!
//! `SyntaxKind` is a `u16` newtype rather than a closed enum so the substrate stays
//! language-agnostic: this module reserves a low band of built-in kinds (trivia, the
//! default lexer's lexical classes, and structural tags like `ERROR`/`OPAQUE`/`ROOT`),
//! while each DSL frontend defines its own kinds at or above [`SyntaxKind::FIRST_CUSTOM`].
//! A [`Token`] is just a `(kind, span)` pair, keeping the lexer output trivially lossless.

use super::span::TextSpan;

/// An open, language-agnostic syntax tag.
///
/// The numeric value below [`SyntaxKind::FIRST_CUSTOM`] is reserved for the built-in kinds
/// declared here; frontends allocate their own kinds at or above that boundary so the two
/// spaces never collide.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::SyntaxKind;
///
/// assert!(SyntaxKind::WHITESPACE.is_trivia());
/// assert!(!SyntaxKind::IDENT.is_trivia());
///
/// let plugins_kw = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM);
/// assert!(plugins_kw.is_custom());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SyntaxKind(pub u16);

impl SyntaxKind {
    /// End-of-input marker produced past the last real token.
    pub const EOF: SyntaxKind = SyntaxKind(0);
    /// A run of whitespace (trivia).
    pub const WHITESPACE: SyntaxKind = SyntaxKind(1);
    /// A `//`-style line comment (trivia).
    pub const LINE_COMMENT: SyntaxKind = SyntaxKind(2);
    /// A `/* */`-style block comment (trivia).
    pub const BLOCK_COMMENT: SyntaxKind = SyntaxKind(3);
    /// An identifier or bare word.
    pub const IDENT: SyntaxKind = SyntaxKind(4);
    /// A numeric literal.
    pub const NUMBER: SyntaxKind = SyntaxKind(5);
    /// A quoted string literal (terminated or not — see the error side table).
    pub const STRING: SyntaxKind = SyntaxKind(6);
    /// A single punctuation / operator byte not otherwise classified.
    pub const PUNCT: SyntaxKind = SyntaxKind(7);
    /// A stray byte the default lexer could not classify.
    pub const ERROR: SyntaxKind = SyntaxKind(8);

    /// A node wrapping unexpected/recovered tokens (structural, set by the parser).
    pub const ERROR_NODE: SyntaxKind = SyntaxKind(9);
    /// A node wrapping a tolerated-but-unrecognized run (the opaque fallback).
    pub const OPAQUE: SyntaxKind = SyntaxKind(10);
    /// The implicit root node wrapping a whole parsed document.
    pub const ROOT: SyntaxKind = SyntaxKind(11);

    /// First tag value a frontend may use for its own kinds.
    ///
    /// Kept well above the built-in band so adding more built-ins later never shifts the
    /// frontends' kinds.
    pub const FIRST_CUSTOM: u16 = 256;

    /// Wraps a raw `u16` as a `SyntaxKind`.
    pub const fn from_raw(raw: u16) -> Self {
        SyntaxKind(raw)
    }

    /// Returns the underlying raw `u16`.
    pub const fn to_raw(self) -> u16 {
        self.0
    }

    /// Returns `true` for whitespace and comment kinds.
    ///
    /// The parser skips trivia when looking ahead, but the builder still emits it so the
    /// tree round-trips to the original source.
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        )
    }

    /// Returns `true` if this is the end-of-input marker.
    pub const fn is_eof(self) -> bool {
        self.0 == SyntaxKind::EOF.0
    }

    /// Returns `true` if this kind belongs to a frontend's custom range.
    pub const fn is_custom(self) -> bool {
        self.0 >= SyntaxKind::FIRST_CUSTOM
    }

    /// Returns a short debug name for the built-in kinds, or `"CUSTOM"` otherwise.
    ///
    /// Used by tree-shape dumps and the demo; frontends render their own names.
    pub const fn builtin_name(self) -> &'static str {
        match self {
            SyntaxKind::EOF => "EOF",
            SyntaxKind::WHITESPACE => "WHITESPACE",
            SyntaxKind::LINE_COMMENT => "LINE_COMMENT",
            SyntaxKind::BLOCK_COMMENT => "BLOCK_COMMENT",
            SyntaxKind::IDENT => "IDENT",
            SyntaxKind::NUMBER => "NUMBER",
            SyntaxKind::STRING => "STRING",
            SyntaxKind::PUNCT => "PUNCT",
            SyntaxKind::ERROR => "ERROR",
            SyntaxKind::ERROR_NODE => "ERROR_NODE",
            SyntaxKind::OPAQUE => "OPAQUE",
            SyntaxKind::ROOT => "ROOT",
            _ => "CUSTOM",
        }
    }
}

/// A single lossless token: a syntax tag plus the byte span it covers.
///
/// The token carries no owned text — the span indexes back into the source — so a token
/// stream is a cheap, faithful description of the input including trivia.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{SyntaxKind, TextSpan, Token};
///
/// let tok = Token::new(SyntaxKind::IDENT, TextSpan::new(0, 3));
/// assert_eq!(tok.text("foo = 1"), "foo");
/// assert!(!tok.is_trivia());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// The token's syntax tag.
    pub kind: SyntaxKind,
    /// The byte range the token covers.
    pub span: TextSpan,
}

impl Token {
    /// Builds a token from a kind and span.
    pub const fn new(kind: SyntaxKind, span: TextSpan) -> Self {
        Self { kind, span }
    }

    /// Returns the source slice this token covers.
    pub fn text(self, source: &str) -> &str {
        self.span.text(source)
    }

    /// Returns `true` if this token is trivia (whitespace/comment).
    pub const fn is_trivia(self) -> bool {
        self.kind.is_trivia()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivia_classification_matches_builtins() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::LINE_COMMENT.is_trivia());
        assert!(SyntaxKind::BLOCK_COMMENT.is_trivia());
        assert!(!SyntaxKind::IDENT.is_trivia());
        assert!(!SyntaxKind::STRING.is_trivia());
    }

    #[test]
    fn custom_range_starts_above_builtins() {
        assert!(!SyntaxKind::ROOT.is_custom());
        assert!(SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM).is_custom());
        assert!(SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 50).is_custom());
    }

    #[test]
    fn raw_roundtrips() {
        let k = SyntaxKind::from_raw(777);
        assert_eq!(k.to_raw(), 777);
        assert_eq!(k.builtin_name(), "CUSTOM");
    }

    #[test]
    fn token_text_indexes_source() {
        let tok = Token::new(SyntaxKind::NUMBER, TextSpan::new(6, 2));
        assert_eq!(tok.text("foo = 42"), "42");
    }

    #[test]
    fn eof_is_detected() {
        assert!(SyntaxKind::EOF.is_eof());
        assert!(!SyntaxKind::IDENT.is_eof());
    }
}
