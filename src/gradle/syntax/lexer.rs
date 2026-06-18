//! The default lossless tokenizer.
//!
//! [`tokenize`] turns a UTF-8 source into a [`Lexed`] stream that preserves EVERY byte —
//! including whitespace and comments — so the tree round-trips. It is the substrate's
//! batteries-included lexer: frontends MAY relex or remap kinds, but this already covers
//! identifiers, numbers, quoted strings (with unterminated recovery), single-byte
//! punctuation, line/block comments (unterminated block recovers to EOF), and a stray-byte
//! [`SyntaxKind::ERROR`] token. Lexical problems are recorded in a [`SyntaxErrors`] side
//! table, never by panicking.

use super::errors::{SyntaxErrorKind, SyntaxErrors};
use super::span::TextSpan;
use super::token::{SyntaxKind, Token};

/// The result of tokenizing a source: the token stream plus any lexical errors.
///
/// The token stream is trivia-preserving and exhaustive (its spans tile the whole input),
/// so concatenating every token's text reproduces the source exactly.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::tokenize;
///
/// let lexed = tokenize("a = 1");
/// let rebuilt: String = lexed.tokens.iter().map(|t| t.text("a = 1")).collect();
/// assert_eq!(rebuilt, "a = 1");
/// assert!(lexed.errors.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct Lexed {
    /// The trivia-preserving token stream.
    pub tokens: Vec<Token>,
    /// Lexical errors discovered during scanning (e.g. unterminated strings).
    pub errors: SyntaxErrors,
}

/// Tokenizes `source` into a trivia-preserving [`Lexed`] stream.
///
/// Never panics: malformed input (unterminated strings/blocks, stray bytes) is recovered
/// into tokens plus side-table errors.
pub fn tokenize(source: &str) -> Lexed {
    let mut scanner = Scanner::new(source);
    scanner.run();
    Lexed { tokens: scanner.tokens, errors: scanner.errors }
}

/// A single forward pass over the source bytes, emitting tokens and lexical errors.
///
/// The scanner walks byte offsets directly (the classes it recognizes are all ASCII), so a
/// multi-byte UTF-8 sequence inside an identifier or string is carried along by its bytes
/// without ever splitting a `char` boundary in a returned span.
struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    errors: SyntaxErrors,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
            errors: SyntaxErrors::new(),
        }
    }

    fn run(&mut self) {
        while self.pos < self.bytes.len() {
            let start = self.pos;
            let byte = self.bytes[start];
            match byte {
                b' ' | b'\t' | b'\r' | b'\n' => self.scan_while(SyntaxKind::WHITESPACE, is_space),
                b'/' if self.peek(1) == Some(b'/') => self.scan_line_comment(),
                b'/' if self.peek(1) == Some(b'*') => self.scan_block_comment(),
                b'"' | b'\'' => self.scan_string(byte),
                _ if is_ident_start(byte) => self.scan_while(SyntaxKind::IDENT, is_ident_continue),
                _ if byte.is_ascii_digit() => self.scan_number(),
                _ if is_punct(byte) => self.emit_single(SyntaxKind::PUNCT),
                _ => self.scan_error_run(),
            }
            debug_assert!(self.pos > start, "scanner must make progress");
        }
    }

    fn scan_while(&mut self, kind: SyntaxKind, pred: fn(u8) -> bool) {
        let start = self.pos;
        self.pos += 1;
        while let Some(byte) = self.current() {
            if pred(byte) {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.emit(kind, start);
    }

    fn scan_line_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while let Some(byte) = self.current() {
            if byte == b'\n' {
                break;
            }
            self.pos += 1;
        }
        self.emit(SyntaxKind::LINE_COMMENT, start);
    }

    fn scan_block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos < self.bytes.len() {
            if self.current() == Some(b'*') && self.peek(1) == Some(b'/') {
                self.pos += 2;
                self.emit(SyntaxKind::BLOCK_COMMENT, start);
                return;
            }
            self.pos += 1;
        }
        // Unterminated block comment: recover by taking the rest as one comment token.
        self.emit(SyntaxKind::BLOCK_COMMENT, start);
    }

    fn scan_string(&mut self, quote: u8) {
        let start = self.pos;
        self.pos += 1;
        let mut terminated = false;
        while let Some(byte) = self.current() {
            if byte == b'\\' && self.peek(1).is_some() {
                self.pos += 2;
                continue;
            }
            if byte == b'\n' {
                break;
            }
            self.pos += 1;
            if byte == quote {
                terminated = true;
                break;
            }
        }
        let span = self.span_from(start);
        self.tokens.push(Token::new(SyntaxKind::STRING, span));
        if !terminated {
            self.errors.push(SyntaxErrorKind::UnterminatedString, span);
        }
    }

    fn scan_number(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while let Some(byte) = self.current() {
            if byte.is_ascii_digit() || byte == b'.' || byte == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.emit(SyntaxKind::NUMBER, start);
    }

    fn scan_error_run(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while let Some(byte) = self.current() {
            if is_known_start(byte) {
                break;
            }
            self.pos += 1;
        }
        self.emit(SyntaxKind::ERROR, start);
    }

    fn emit_single(&mut self, kind: SyntaxKind) {
        let start = self.pos;
        self.pos += 1;
        self.emit(kind, start);
    }

    fn emit(&mut self, kind: SyntaxKind, start: usize) {
        self.tokens.push(Token::new(kind, self.span_from(start)));
    }

    fn span_from(&self, start: usize) -> TextSpan {
        TextSpan::from_range(start, self.pos)
    }

    fn current(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }
}

const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

const fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

const fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

const fn is_punct(byte: u8) -> bool {
    matches!(
        byte,
        b'{' | b'}'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'='
            | b':'
            | b','
            | b'.'
            | b';'
            | b'+'
            | b'-'
            | b'*'
            | b'/'
            | b'<'
            | b'>'
            | b'!'
            | b'&'
            | b'|'
            | b'?'
            | b'@'
            | b'%'
            | b'^'
            | b'~'
    )
}

/// Returns `true` if `byte` could begin a non-error token, used to bound an error run.
const fn is_known_start(byte: u8) -> bool {
    is_space(byte)
        || is_ident_start(byte)
        || byte.is_ascii_digit()
        || is_punct(byte)
        || byte == b'"'
        || byte == b'\''
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuilt(source: &str) -> String {
        tokenize(source).tokens.iter().map(|t| t.text(source)).collect()
    }

    #[test]
    fn tokenizes_basic_assignment_with_trivia() {
        let source = "foo = 42";
        let lexed = tokenize(source);
        let kinds: Vec<_> = lexed.tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::IDENT,
                SyntaxKind::WHITESPACE,
                SyntaxKind::PUNCT,
                SyntaxKind::WHITESPACE,
                SyntaxKind::NUMBER,
            ]
        );
        assert!(lexed.errors.is_empty());
    }

    #[test]
    fn round_trips_valid_source() {
        let source = "plugin { id = \"x\" } // tail\n";
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn round_trips_messy_source() {
        let source = "  \n\t a/*c*/b 12.3 \"s\" :=,{}\n";
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn line_and_block_comments_are_trivia() {
        let source = "a // line\n/* block */ b";
        let lexed = tokenize(source);
        let comment_kinds: Vec<_> = lexed
            .tokens
            .iter()
            .map(|t| t.kind)
            .filter(|k| *k == SyntaxKind::LINE_COMMENT || *k == SyntaxKind::BLOCK_COMMENT)
            .collect();
        assert_eq!(comment_kinds, vec![SyntaxKind::LINE_COMMENT, SyntaxKind::BLOCK_COMMENT]);
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn unterminated_string_yields_token_and_error_not_panic() {
        let source = "x = \"oops";
        let lexed = tokenize(source);
        let string = lexed
            .tokens
            .iter()
            .find(|t| t.kind == SyntaxKind::STRING)
            .expect("an unterminated string still lexes to a STRING token");
        assert_eq!(string.text(source), "\"oops");
        assert_eq!(lexed.errors.len(), 1);
        let err = lexed.errors.as_slice()[0];
        assert_eq!(err.kind, SyntaxErrorKind::UnterminatedString);
        assert_eq!(err.span, string.span);
        assert!(!err.span.is_empty());
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn unterminated_block_comment_recovers_to_eof() {
        let source = "a /* never closed";
        let lexed = tokenize(source);
        assert!(lexed.tokens.iter().any(|t| t.kind == SyntaxKind::BLOCK_COMMENT));
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn stray_byte_becomes_error_token() {
        let source = "a $ b";
        let lexed = tokenize(source);
        assert!(lexed.tokens.iter().any(|t| t.kind == SyntaxKind::ERROR));
        assert_eq!(rebuilt(source), source);
    }

    #[test]
    fn empty_input_yields_no_tokens() {
        let lexed = tokenize("");
        assert!(lexed.tokens.is_empty());
        assert!(lexed.errors.is_empty());
    }
}
