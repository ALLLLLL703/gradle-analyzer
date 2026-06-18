//! Manual-QA demo for the Task 3 shared tolerant syntax substrate.
//!
//! Run with:
//!
//! ```text
//! cargo run --example syntax_demo
//! ```
//!
//! It lexes + parses a small MESSY input with a tiny in-example block grammar (balanced
//! `{ }` over the substrate's generic tokens — no Gradle/Groovy/Kotlin semantics), then
//! prints, over the real public API:
//!
//! 1. the token stream (kind + span + text, trivia included),
//! 2. the reconstructed `text()` from the red tree (must equal the input),
//! 3. the tree shape (indented kinds + spans), and
//! 4. the typed error list with spans.
//!
//! The binary-observable PASS is: `text()` round-trips exactly AND the malformed run shows a
//! non-empty tree plus a typed `UnclosedBlock` error whose span is non-zero and anchored to
//! the end of the last consumed token (not an empty EOF span).

use gradle_analyzer::gradle::syntax::{
    Parse, Parser, SyntaxElement, SyntaxErrorKind, SyntaxKind, SyntaxNode, tokenize,
};

/// A frontend-style custom kind for the in-example block grammar.
const BLOCK: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM);

fn main() {
    println!("=== gradle-analyzer syntax substrate demo (generic block grammar) ===\n");

    // Deliberately messy: nested block, comments, a string, AND a missing closing brace.
    let source = "a {\n  b = \"x\" /*c*/\n  { } // inner\n"; // outer never closed
    println!("input ({} bytes): {source:?}\n", source.len());

    print_tokens(source);
    let parse = parse_blocks(source);
    print_round_trip(source, &parse);
    print_tree(&parse);
    print_errors(&parse);

    println!("\n{}", verdict(source, &parse));
}

/// Prints every token with its kind, span, and exact text (trivia included).
fn print_tokens(source: &str) {
    println!("--- 1. token stream (trivia preserved) ---");
    let lexed = tokenize(source);
    for token in &lexed.tokens {
        println!(
            "  {:<14} {:>3}..{:<3} {:?}",
            token.kind.builtin_name(),
            token.span.start,
            token.span.end(),
            token.text(source),
        );
    }
    println!();
}

/// Reconstructs the source from the red tree and prints the round-trip check.
fn print_round_trip(source: &str, parse: &Parse) {
    println!("--- 2. reconstructed text() (must equal input) ---");
    let red = SyntaxNode::new_root(parse.green.clone());
    let rebuilt = red.text();
    println!("  red.text() == input : {}", rebuilt == source);
    println!("  rebuilt: {rebuilt:?}\n");
}

/// Prints the indented tree shape (kinds + absolute spans).
fn print_tree(parse: &Parse) {
    println!("--- 3. tree shape (kind @ span) ---");
    let red = SyntaxNode::new_root(parse.green.clone());
    print_node(&red, 0);
    println!();
}

fn print_node(node: &SyntaxNode, depth: usize) {
    let span = node.span();
    println!(
        "{:indent$}{} @ {}..{}",
        "",
        kind_name(node.kind()),
        span.start,
        span.end(),
        indent = depth * 2,
    );
    for child in node.children() {
        match child {
            SyntaxElement::Node(child_node) => print_node(child_node, depth + 1),
            SyntaxElement::Token(token) => {
                let span = token.span();
                println!(
                    "{:indent$}{} @ {}..{} {:?}",
                    "",
                    token.kind().builtin_name(),
                    span.start,
                    span.end(),
                    token.text(),
                    indent = (depth + 1) * 2,
                );
            }
        }
    }
}

/// Prints the typed error side table with spans.
fn print_errors(parse: &Parse) {
    println!("--- 4. typed errors (side table, with spans) ---");
    if parse.errors.is_empty() {
        println!("  (none)");
        return;
    }
    for error in parse.errors.as_slice() {
        println!(
            "  {:?} @ {}..{} (key: {})",
            error.kind,
            error.span.start,
            error.span.end(),
            error.message_key().canonical_name(),
        );
    }
}

/// The binary-observable verdict line.
fn verdict(source: &str, parse: &Parse) -> String {
    let red = SyntaxNode::new_root(parse.green.clone());
    let round_trips = red.text() == source;
    let non_empty = !parse.green.children().is_empty();
    let anchored = parse
        .errors
        .as_slice()
        .iter()
        .find(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
        .map(|e| e.span.start != 0 && e.span.start != source.len());
    match anchored {
        Some(true) if round_trips && non_empty => {
            "PASS: text() round-trips, tree non-empty, UnclosedBlock anchored to last token.".into()
        }
        other => format!(
            "CHECK: round_trips={round_trips} non_empty={non_empty} unclosed_anchored={other:?}"
        ),
    }
}

fn kind_name(kind: SyntaxKind) -> &'static str {
    if kind == BLOCK { "BLOCK" } else { kind.builtin_name() }
}

/// Parses balanced `{ }` blocks; everything else is tolerated by bumping it.
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

fn block(p: &mut Parser) {
    p.start_node(BLOCK);
    p.bump(); // opening "{"
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

/// The demo doubles as a smoke test so `cargo test` exercises the round-trip + anchoring.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_round_trips_and_anchors_unclosed_block() {
        let source = "a {\n  b = \"x\" /*c*/\n  { } // inner\n";
        let parse = parse_blocks(source);

        let red = SyntaxNode::new_root(parse.green.clone());
        assert_eq!(red.text(), source, "round-trip must hold");
        assert!(!parse.green.children().is_empty(), "tree must be non-empty");

        let unclosed = parse
            .errors
            .as_slice()
            .iter()
            .find(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
            .expect("missing outer brace yields UnclosedBlock");
        assert_ne!(unclosed.span.start, 0, "not an EOF-zero span");
        assert_ne!(unclosed.span.start, source.len(), "anchored to last token, not raw EOF");
    }
}
