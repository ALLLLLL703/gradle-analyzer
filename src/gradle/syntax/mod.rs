//! Shared tolerant syntax substrate (lexer, green/red tree, spans, typed errors, recovery).
//!
//! This is the GENERIC, language-agnostic foundation both DSL frontends (Kotlin in Task 5,
//! Groovy in Task 6) build on. It is modeled on rust-analyzer's architecture but implements
//! its OWN minimal green/red trees (no `rowan`, no `tree-sitter`, no parser generator):
//!
//! - [`TextSpan`] — byte-based source ranges, the common currency for tokens, trees, errors.
//! - [`tokenize`]/[`Lexed`]/[`Token`]/[`SyntaxKind`] — a lossless, trivia-preserving lexer
//!   over an OPEN tag space (frontends add kinds at/above [`SyntaxKind::FIRST_CUSTOM`]).
//! - [`GreenNode`]/[`GreenToken`]/[`GreenChild`] + [`GreenNodeBuilder`] — the immutable,
//!   shareable storage tree and the checkpoint-based builder that constructs it.
//! - [`SyntaxNode`]/[`SyntaxToken`]/[`SyntaxElement`] — the red cursor adding absolute
//!   offsets and parent links over a green tree.
//! - [`SyntaxError`]/[`SyntaxErrorKind`]/[`SyntaxErrors`] — typed problems kept in a side
//!   table OUTSIDE the tree, each mapped to a [`crate::i18n::MessageKey`].
//! - [`Parser`]/[`Parse`] — the resilient driver with EOF-anchored recovery, bounded opaque
//!   fallback, and a zero-progress guard.
//!
//! No Gradle/Groovy/Kotlin keywords or semantics live here. Errors never panic on bad input.
//!
//! # Example
//!
//! ```
//! use gradle_analyzer::gradle::syntax::{Parser, SyntaxNode};
//!
//! // Lex + parse a trivial input, tolerating everything, then round-trip via the red tree.
//! let source = "a = 1 // note\n";
//! let parse = Parser::new(source).parse_with(|p| {
//!     while !p.at_eof() {
//!         p.bump_any();
//!     }
//! });
//! assert_eq!(parse.text(), source);
//!
//! let red = SyntaxNode::new_root(parse.green);
//! assert_eq!(red.text(), source);
//! ```

pub mod builder;
pub mod errors;
pub mod green;
pub mod lexer;
pub mod parser;
pub mod red;
pub mod span;
pub mod token;

pub use builder::{Checkpoint, GreenNodeBuilder};
pub use errors::{SyntaxError, SyntaxErrorKind, SyntaxErrors};
pub use green::{GreenChild, GreenNode, GreenToken};
pub use lexer::{Lexed, tokenize};
pub use parser::{Parse, Parser};
pub use red::{SyntaxElement, SyntaxNode, SyntaxToken};
pub use span::TextSpan;
pub use token::{SyntaxKind, Token};

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_tolerant(source: &str) -> Parse {
        Parser::new(source).parse_with(|p| {
            while !p.at_eof() {
                p.bump_any();
            }
        })
    }

    #[test]
    fn lex_parse_red_round_trip_on_messy_input() {
        let source = "  plugin {\n  id = \"x\" /*c*/\n} // tail\n\t$";
        let parse = parse_tolerant(source);
        assert_eq!(parse.text(), source, "green tree round-trips");

        let red = SyntaxNode::new_root(parse.green);
        assert_eq!(red.text(), source, "red tree round-trips");
        assert_eq!(red.kind(), SyntaxKind::ROOT);
        assert_eq!(red.span(), TextSpan::new(0, source.len()));
    }

    #[test]
    fn stray_byte_surfaces_in_tokens_but_does_not_break_round_trip() {
        let source = "a $ b";
        let lexed = tokenize(source);
        assert!(lexed.tokens.iter().any(|t| t.kind == SyntaxKind::ERROR));
        let parse = parse_tolerant(source);
        assert_eq!(parse.text(), source);
    }

    #[test]
    fn truncated_input_never_panics_and_keeps_a_tree() {
        for source in ["", "{", "\"unterminated", "/*open", "}}}", "= = ="] {
            let parse = parse_tolerant(source);
            assert_eq!(parse.text(), source, "round-trip holds for {source:?}");
            let _ = SyntaxNode::new_root(parse.green);
        }
    }
}
