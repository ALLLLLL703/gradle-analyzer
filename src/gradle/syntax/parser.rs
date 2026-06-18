//! The recoverable recursive-descent parser driver.
//!
//! [`Parser`] is the generic engine the DSL frontends drive: it walks a token stream,
//! emits a green tree through a [`GreenNodeBuilder`], and records problems in a
//! [`SyntaxErrors`] side table — never panicking on bad input. Trivia is skipped for
//! lookahead ([`Parser::at`]/[`Parser::nth`]) but still emitted into the tree, so the
//! result round-trips. Recovery is resilient: a missing closing delimiter anchors its
//! error to the END of the last consumed token (not an empty EOF span), an unrecognized
//! run collapses into one opaque node so a frontend never aborts, and a zero-progress
//! fuel guard makes lookahead behave as if at EOF if a grammar loops without consuming —
//! which also doubles as hung-loop protection.

use std::cell::Cell;

use super::builder::{Checkpoint, GreenNodeBuilder};
use super::errors::{SyntaxError, SyntaxErrorKind, SyntaxErrors};
use super::green::GreenNode;
use super::lexer::tokenize;
use super::span::TextSpan;
use super::token::{SyntaxKind, Token};

/// The output of a parse: the green tree plus the typed error side table.
///
/// The tree and the errors are independent — the tree never embeds diagnostics — so a
/// consumer renders the tree (round-trippable) and the errors (localizable) separately.
#[derive(Debug, Clone)]
pub struct Parse {
    /// The constructed green tree (always present, even on malformed input).
    pub green: GreenNode,
    /// The typed problems discovered while lexing and parsing.
    pub errors: SyntaxErrors,
}

impl Parse {
    /// Reconstructs the exact source text covered by the parsed tree.
    pub fn text(&self) -> String {
        self.green.text()
    }
}

/// Consecutive lookaheads without a [`Parser::bump`] before the zero-progress guard trips.
const LOOKAHEAD_FUEL: u32 = 256;

/// A resilient parser over a trivia-preserving token stream.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::{Parser, SyntaxKind};
///
/// // A tiny grammar: consume every token into the root, tolerating anything.
/// let mut parser = Parser::new("a b c");
/// let parse = parser.parse_with(|p| {
///     while !p.at_eof() {
///         p.bump_any();
///     }
/// });
/// assert_eq!(parse.text(), "a b c");
/// assert!(parse.errors.is_empty());
/// ```
pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    builder: GreenNodeBuilder,
    errors: SyntaxErrors,
    last_token_end: usize,
    fuel: Cell<u32>,
}

impl<'a> Parser<'a> {
    /// Builds a parser over `source` using the default [`tokenize`] lexer.
    ///
    /// Lexer errors (e.g. unterminated strings) are pre-seeded into the error table so the
    /// final [`Parse`] reports them alongside parse-time problems.
    pub fn new(source: &'a str) -> Self {
        let lexed = tokenize(source);
        Self::with_tokens(source, lexed.tokens, lexed.errors)
    }

    /// Builds a parser over a frontend-supplied token stream and seed errors.
    pub fn with_tokens(source: &'a str, tokens: Vec<Token>, errors: SyntaxErrors) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors,
            last_token_end: 0,
            fuel: Cell::new(LOOKAHEAD_FUEL),
        }
    }

    /// Runs `grammar` inside an implicit `ROOT` node and returns the finished [`Parse`].
    pub fn parse_with(mut self, grammar: impl FnOnce(&mut Parser<'a>)) -> Parse {
        self.builder.start_node(SyntaxKind::ROOT);
        grammar(&mut self);
        self.eat_trivia();
        self.flush_remaining();
        self.builder.finish_node();
        Parse { green: self.builder.finish(), errors: self.errors }
    }

    /// Returns the kind of the `n`-th upcoming non-trivia token (`0` = current).
    ///
    /// Returns [`SyntaxKind::EOF`] past the end OR once the zero-progress guard has tripped,
    /// so any well-formed lookahead loop terminates instead of spinning.
    pub fn nth(&self, n: usize) -> SyntaxKind {
        if self.fuel.get() == 0 {
            return SyntaxKind::EOF;
        }
        self.fuel.set(self.fuel.get() - 1);
        self.nth_token(n).map_or(SyntaxKind::EOF, |t| t.kind)
    }

    /// Returns `true` if the current non-trivia token has kind `kind`.
    pub fn at(&self, kind: SyntaxKind) -> bool {
        self.nth(0) == kind
    }

    /// Returns `true` if the current non-trivia token's source text equals `text`.
    ///
    /// Lets a generic grammar match concrete punctuation/words (e.g. `{`) without baking
    /// any keyword into the substrate.
    pub fn at_text(&self, text: &str) -> bool {
        if self.fuel.get() == 0 {
            return false;
        }
        self.nth_token(0)
            .map(|t| t.text(self.source) == text)
            .unwrap_or(false)
    }

    /// Returns `true` if there is no further non-trivia token (or the guard has tripped).
    pub fn at_eof(&self) -> bool {
        self.nth(0).is_eof()
    }

    /// Returns the current non-trivia token's source text, if any.
    pub fn current_text(&self) -> Option<&'a str> {
        self.nth_token(0).map(|t| t.text(self.source))
    }

    /// Returns `true` if the zero-progress guard has tripped (no bump in `LOOKAHEAD_FUEL`
    /// consecutive lookaheads). Exposed so callers and tests can observe the guard.
    pub fn fuel_exhausted(&self) -> bool {
        self.fuel.get() == 0
    }

    /// Consumes the current non-trivia token into the tree (after flushing leading trivia).
    ///
    /// Does nothing at EOF. On a real consume it records the token's end as the recovery
    /// anchor and refills the lookahead fuel.
    pub fn bump(&mut self) {
        self.eat_trivia();
        if let Some(token) = self.tokens.get(self.pos).copied() {
            self.builder.token(token.kind, token.text(self.source));
            self.last_token_end = token.span.end();
            self.pos += 1;
            self.fuel.set(LOOKAHEAD_FUEL);
        }
    }

    /// Alias for [`Parser::bump`] used by grammars as their guaranteed-progress primitive.
    pub fn bump_any(&mut self) {
        self.bump();
    }

    /// Consumes the current token if it matches `kind`; otherwise records `UnexpectedToken`.
    ///
    /// At EOF the error is anchored to the end of the last consumed token, not an empty
    /// terminal span.
    pub fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.error_recover(SyntaxErrorKind::UnexpectedToken);
            false
        }
    }

    /// Records `error_kind` and, if not at EOF, wraps the offending token in an `ERROR_NODE`.
    ///
    /// This both reports the problem and guarantees forward progress (the bad token is
    /// consumed), so a recovery loop cannot stall on it.
    pub fn error_recover(&mut self, error_kind: SyntaxErrorKind) {
        if self.at_eof() {
            self.error_eof_anchored(error_kind);
            return;
        }
        let span = self.current_span().unwrap_or_else(|| self.eof_span());
        self.errors.push(error_kind, span);
        self.builder.start_node(SyntaxKind::ERROR_NODE);
        self.bump();
        self.builder.finish_node();
    }

    /// Collapses a tolerated, unrecognized run into ONE `OPAQUE` node.
    ///
    /// Consumes tokens until the current token's kind is in `recovery` or EOF is reached,
    /// always making at least one token of progress when not at EOF, so a frontend never
    /// aborts on input it does not model.
    pub fn bump_opaque_run(&mut self, recovery: &[SyntaxKind]) {
        if self.at_eof() {
            return;
        }
        self.builder.start_node(SyntaxKind::OPAQUE);
        self.bump();
        while !self.at_eof() && !recovery.contains(&self.nth(0)) {
            self.bump();
        }
        self.builder.finish_node();
    }

    /// Records an error whose span is anchored to the end of the last consumed token.
    ///
    /// Used for missing closing delimiters: the span starts at the real last-token end
    /// rather than at an arbitrary EOF-zero position, so diagnostics point at the malformed
    /// range.
    pub fn error_eof_anchored(&mut self, error_kind: SyntaxErrorKind) {
        self.errors
            .push(error_kind, TextSpan::empty_at(self.last_token_end));
    }

    /// Records an error at an explicit span.
    pub fn error_at(&mut self, error: SyntaxError) {
        self.errors.push_error(error);
    }

    /// Opens a new node of `kind` (builder passthrough).
    pub fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind);
    }

    /// Closes the current node (builder passthrough).
    pub fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Records a checkpoint for retroactive wrapping (builder passthrough).
    pub fn checkpoint(&mut self) -> Checkpoint {
        self.builder.checkpoint()
    }

    /// Retroactively wraps children since `checkpoint` in `kind` (builder passthrough).
    pub fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind);
    }

    fn nth_token(&self, n: usize) -> Option<Token> {
        let mut seen = 0;
        let mut idx = self.pos;
        while let Some(token) = self.tokens.get(idx) {
            if token.kind.is_trivia() {
                idx += 1;
                continue;
            }
            if seen == n {
                return Some(*token);
            }
            seen += 1;
            idx += 1;
        }
        None
    }

    fn current_span(&self) -> Option<TextSpan> {
        self.nth_token(0).map(|t| t.span)
    }

    fn eof_span(&self) -> TextSpan {
        TextSpan::empty_at(self.last_token_end)
    }

    fn eat_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.pos).copied() {
            if !token.kind.is_trivia() {
                break;
            }
            self.builder.token(token.kind, token.text(self.source));
            self.pos += 1;
        }
    }

    fn flush_remaining(&mut self) {
        while let Some(token) = self.tokens.get(self.pos).copied() {
            self.builder.token(token.kind, token.text(self.source));
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A tiny in-test grammar: balanced `{ }` blocks over PUNCT tokens, anything else
    // tolerated. BLOCK is a frontend-style custom kind.
    const BLOCK: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM);

    fn block(p: &mut Parser) {
        p.start_node(BLOCK);
        p.bump(); // the opening "{"
        loop {
            if p.at_eof() {
                p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
                break;
            }
            if p.at_text("}") {
                p.bump();
                break;
            }
            if p.at_text("{") {
                block(p);
            } else {
                p.bump_any();
            }
        }
        p.finish_node();
    }

    fn parse_blocks(source: &str) -> Parse {
        Parser::new(source).parse_with(|p| {
            while !p.at_eof() {
                if p.at_text("{") {
                    block(p);
                } else {
                    p.bump_any();
                }
            }
        })
    }

    #[test]
    fn well_formed_blocks_round_trip_without_errors() {
        let source = "{ { } }";
        let parse = parse_blocks(source);
        assert_eq!(parse.text(), source);
        assert!(parse.errors.is_empty());
    }

    #[test]
    fn missing_close_yields_tree_plus_last_token_anchored_error() {
        // Outer block never closed; trailing newline makes raw EOF (len 5) distinct from
        // the last consumed NON-trivia token end (4), proving last-token anchoring.
        let source = "{ {}\n";
        let parse = parse_blocks(source);

        // Tree is non-empty and round-trips despite the error.
        assert!(!parse.green.children().is_empty());
        assert_eq!(parse.text(), source);

        // Exactly one UnclosedBlock error, anchored to the last token end (4), NOT 0 and
        // NOT raw EOF/source-len (5).
        let unclosed: Vec<_> = parse
            .errors
            .as_slice()
            .iter()
            .filter(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
            .collect();
        assert_eq!(unclosed.len(), 1);
        let span = unclosed[0].span;
        assert_eq!(span.start, 4, "anchored to end of last consumed token '}}'");
        assert_ne!(span.start, 0, "not an empty EOF-zero span");
        assert_ne!(span.start, source.len(), "not raw end-of-input");
    }

    #[test]
    fn opaque_fallback_collapses_unrecognized_run_into_one_node() {
        // Grammar: at "{" parse a block, else collapse the run up to the next "{".
        let source = "junk tokens { } more";
        let parse = Parser::new(source).parse_with(|p| {
            while !p.at_eof() {
                if p.at_text("{") {
                    block(p);
                } else {
                    p.bump_opaque_run(&[SyntaxKind::PUNCT]);
                }
            }
        });
        assert_eq!(parse.text(), source);

        let opaque_count = parse
            .green
            .children()
            .iter()
            .filter(|c| c.kind() == SyntaxKind::OPAQUE)
            .count();
        assert!(opaque_count >= 1, "unrecognized run becomes an opaque node");
    }

    #[test]
    fn zero_progress_loop_is_broken_by_the_fuel_guard() {
        // A deliberately buggy grammar that never consumes. The fuel guard must make the
        // loop terminate (test completing is the proof of no hang) and expose its trip.
        let source = "x y z";
        let parse = Parser::new(source).parse_with(|p| {
            while !p.at_eof() {
                // BUG: forgot to bump — without the guard this spins forever.
                let _ = p.at_text("anything");
            }
            assert!(p.fuel_exhausted(), "guard must trip on a stuck loop");
        });
        // Trailing flush still preserves the source exactly.
        assert_eq!(parse.text(), source);
    }

    #[test]
    fn expect_records_unexpected_then_recovers() {
        let source = "a";
        let parse = Parser::new(source).parse_with(|p| {
            // Expect a "{" we don't have -> UnexpectedToken + ERROR_NODE recovery.
            p.expect(SyntaxKind::PUNCT);
            while !p.at_eof() {
                p.bump_any();
            }
        });
        assert_eq!(parse.text(), source);
        assert!(
            parse
                .errors
                .as_slice()
                .iter()
                .any(|e| e.kind == SyntaxErrorKind::UnexpectedToken)
        );
    }

    #[test]
    fn unterminated_string_error_is_carried_through_parse() {
        let source = "= \"oops";
        let parse = Parser::new(source).parse_with(|p| {
            while !p.at_eof() {
                p.bump_any();
            }
        });
        assert_eq!(parse.text(), source);
        assert!(
            parse
                .errors
                .as_slice()
                .iter()
                .any(|e| e.kind == SyntaxErrorKind::UnterminatedString)
        );
    }
}
