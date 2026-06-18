//! Groovy slashy-string (`/regex/`) recognition as a token-stream re-lex pass.
//!
//! The shared substrate lexer ([`crate::gradle::syntax`]) is generic: it knows nothing about
//! Groovy's slashy strings, so it mis-lexes a `"` inside `/"path"/` as the start of a normal
//! string (which then runs unterminated to end of line). Because that lexer must NOT learn
//! Groovy specifics, this frontend pass fixes it up: it walks the generic token stream and,
//! wherever a `/` appears in REGEX/VALUE position (after `=~`, `=`, `(`, `,`, `[`, `:`, an
//! operator, `return`, or at statement-value start — NOT after an identifier/number/`)`/`]`
//! where `/` is division), it re-scans the raw source from that `/` to the closing unescaped
//! `/` (treating `"`/`'` as literal and `\X` as escaped) and replaces that whole run with ONE
//! [`SyntaxKind::STRING`] token. Lexer errors that fell inside the merged span are dropped.
//!
//! Tolerant by design: a slashy with no closing `/` before end-of-line/input merges only up
//! to that boundary (bounding the damage to one line) and never emits an error.

use crate::gradle::syntax::{SyntaxErrors, TextSpan, Token, tokenize};
use crate::gradle::syntax::{SyntaxError, SyntaxKind};

/// The result of the Groovy re-lex: a token stream with slashy strings merged, plus the
/// lexer errors that survive (those inside a merged slashy span are dropped).
pub(super) struct Relexed {
    /// The trivia-preserving token stream with slashy runs folded into single STRING tokens.
    pub tokens: Vec<Token>,
    /// Lexer errors that did not fall inside a merged slashy span.
    pub errors: SyntaxErrors,
}

/// Re-lexes `source`, merging every regex-context slashy string into one STRING token.
///
/// The returned tokens still tile the source exactly (so the parse round-trips), and the
/// returned errors drop any lexer diagnostic that landed inside a merged slashy.
pub(super) fn relex(source: &str) -> Relexed {
    let lexed = tokenize(source);
    let bytes = source.as_bytes();

    let mut tokens: Vec<Token> = Vec::with_capacity(lexed.tokens.len());
    let mut merged_spans: Vec<TextSpan> = Vec::new();
    let mut prev: Option<SyntaxKind> = None;
    let mut prev_text: Option<&str> = None;
    let mut idx = 0;

    while idx < lexed.tokens.len() {
        let token = lexed.tokens[idx];
        if is_slash_punct(token, source) && starts_regex(prev, prev_text) {
            let end = scan_slashy(bytes, token.span.start);
            let span = merged_span(&lexed.tokens, idx, end);
            tokens.push(Token::new(SyntaxKind::STRING, span));
            merged_spans.push(span);
            idx = advance_past(&lexed.tokens, idx, span.end());
            prev = Some(SyntaxKind::STRING);
            prev_text = None;
            continue;
        }
        tokens.push(token);
        if !token.kind.is_trivia() {
            prev = Some(token.kind);
            prev_text = Some(token.text(source));
        }
        idx += 1;
    }

    Relexed { tokens, errors: filter_errors(lexed.errors, &merged_spans) }
}

/// Returns `true` if `token` is exactly a single `/` punctuation byte.
fn is_slash_punct(token: Token, source: &str) -> bool {
    token.kind == SyntaxKind::PUNCT && token.text(source) == "/"
}

/// Returns `true` if a `/` after `prev` begins a slashy string rather than division.
///
/// A slashy opens at statement-value start (no previous token), after a value-expecting
/// operator/delimiter, or after a value-expecting keyword — but NOT after something that
/// produces a value (identifier, number, string, `)`, `]`), where `/` is division.
fn starts_regex(prev: Option<SyntaxKind>, prev_text: Option<&str>) -> bool {
    match prev {
        None => true,
        Some(SyntaxKind::IDENT) => prev_text.is_some_and(value_expecting_keyword),
        Some(SyntaxKind::PUNCT) => prev_text.is_some_and(value_expecting_punct),
        Some(_) => false,
    }
}

/// Returns `true` for keywords after which a `/` opens a regex (`return x =~ /.../`, etc.).
fn value_expecting_keyword(text: &str) -> bool {
    matches!(text, "return" | "case" | "in" | "new" | "assert")
}

/// Returns `true` for single-byte punctuation after which a `/` opens a regex.
fn value_expecting_punct(text: &str) -> bool {
    text.len() == 1 && is_value_expecting_byte(text.as_bytes()[0])
}

/// Punctuation bytes that expect a value next, so a following `/` is a slashy, not division.
const fn is_value_expecting_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'=' | b'(' | b',' | b'[' | b':' | b'{' | b';' | b'&' | b'|' | b'!' | b'<' | b'>'
            | b'+' | b'-' | b'*' | b'%' | b'^' | b'~' | b'?'
    )
}

/// Scans a slashy string from the opening `/` at `start`, returning the byte offset just past
/// the closing unescaped `/` — or the end-of-line/input if it is unterminated.
fn scan_slashy(bytes: &[u8], start: usize) -> usize {
    let mut pos = start + 1;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' if pos + 1 < bytes.len() => pos += 2,
            b'\n' => return pos,
            b'/' => return pos + 1,
            _ => pos += 1,
        }
    }
    pos
}

/// Returns the span the merged STRING must cover: from the `/` token's start to at least
/// `end`, extended to swallow any raw token that straddles `end` so no byte gap is left.
fn merged_span(tokens: &[Token], slash_idx: usize, end: usize) -> TextSpan {
    let start = tokens[slash_idx].span.start;
    let mut covered = end;
    for token in &tokens[slash_idx..] {
        if token.span.start < end && token.span.end() > covered {
            covered = token.span.end();
        }
        if token.span.start >= end {
            break;
        }
    }
    TextSpan::from_range(start, covered)
}

/// Returns the index of the first token at or past `covered`, skipping the merged run.
fn advance_past(tokens: &[Token], from: usize, covered: usize) -> usize {
    let mut idx = from;
    while idx < tokens.len() && tokens[idx].span.start < covered {
        idx += 1;
    }
    idx
}

/// Drops lexer errors whose span starts inside any merged slashy span.
fn filter_errors(errors: SyntaxErrors, merged: &[TextSpan]) -> SyntaxErrors {
    if merged.is_empty() {
        return errors;
    }
    let mut kept = SyntaxErrors::new();
    for error in errors.into_vec() {
        let inside = merged
            .iter()
            .any(|span| error.span.start >= span.start && error.span.start < span.end());
        if !inside {
            kept.push_error(SyntaxError::new(error.kind, error.span));
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuilt(source: &str) -> String {
        relex(source).tokens.iter().map(|t| t.text(source)).collect()
    }

    #[test]
    fn slashy_with_quotes_merges_to_one_string_and_drops_inner_errors() {
        let source = "m =~ /\"path\".*?\"([^\"]*)\"/\n";
        let relexed = relex(source);
        assert!(relexed.errors.is_empty(), "inner string errors dropped");
        let strings: Vec<_> = relexed
            .tokens
            .iter()
            .filter(|t| t.kind == SyntaxKind::STRING)
            .collect();
        assert_eq!(strings.len(), 1, "the whole slashy is ONE string token");
        assert_eq!(strings[0].text(source), "/\"path\".*?\"([^\"]*)\"/");
        assert_eq!(rebuilt(source), source, "tokens still tile the source");
    }

    #[test]
    fn division_is_left_as_punct() {
        let source = "a / b / c";
        let relexed = relex(source);
        let slashes = relexed
            .tokens
            .iter()
            .filter(|t| t.kind == SyntaxKind::PUNCT && t.text(source) == "/")
            .count();
        assert_eq!(slashes, 2, "both slashes stay division PUNCT");
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn comments_are_untouched() {
        let source = "// hi\n/* block */\nx = 1\n";
        let relexed = relex(source);
        assert!(relexed.errors.is_empty());
        assert_eq!(rebuilt(source), source);
        assert!(relexed.tokens.iter().any(|t| t.kind == SyntaxKind::LINE_COMMENT));
        assert!(relexed.tokens.iter().any(|t| t.kind == SyntaxKind::BLOCK_COMMENT));
    }

    #[test]
    fn unterminated_slashy_merges_only_to_eol() {
        let source = "m =~ /abc\nnext = 1\n";
        let relexed = relex(source);
        assert_eq!(rebuilt(source), source, "round-trip preserved");
        let slashy = relexed
            .tokens
            .iter()
            .find(|t| t.kind == SyntaxKind::STRING && t.text(source).starts_with('/'))
            .expect("slashy merged");
        assert_eq!(slashy.text(source), "/abc", "merge bounded to end of line");
    }

    #[test]
    fn simple_slashy_after_operator_merges() {
        let source = "m =~ /abc/\n";
        let relexed = relex(source);
        assert!(relexed.errors.is_empty());
        let slashy = relexed
            .tokens
            .iter()
            .find(|t| t.kind == SyntaxKind::STRING)
            .expect("slashy merged");
        assert_eq!(slashy.text(source), "/abc/");
        assert_eq!(rebuilt(source), source);
    }
}
