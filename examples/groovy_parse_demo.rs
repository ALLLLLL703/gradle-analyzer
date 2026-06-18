//! Manual-QA demo for the Task 6 tolerant Groovy (`.gradle`) frontend.
//!
//! Run with:
//!
//! ```text
//! cargo run --example groovy_parse_demo
//! ```
//!
//! It parses, over the real public `parse_groovy` API:
//!
//! 1. a real-world-style `build.gradle` WITH noise (`def`, a typed local, `if`/`for`, a
//!    regex `=~`, a `.each { }` closure chain, and map/list punctuation), and
//! 2. a BROKEN script (unclosed `dependencies {`).
//!
//! For each it prints the tree shape (kind @ span), the `text() == input` round-trip check,
//! and the typed error side table with spans. The binary-observable PASS is: the noisy-but-
//! valid file shows ZERO `MalformedBlock` errors AND an exact round-trip, while the broken
//! file shows a non-empty tree PLUS a typed `UnclosedBlock` anchored to the last token.

use gradle_analyzer::gradle::parser::groovy::{
    ARG_LIST, ASSIGNMENT, CALL, CLOSURE, DECLARATION, LIST_LITERAL, NAMED_ARG, PATH, STATEMENT,
};
use gradle_analyzer::gradle::parser::parse_groovy;
use gradle_analyzer::gradle::syntax::{Parse, SyntaxElement, SyntaxErrorKind, SyntaxKind, SyntaxNode};

const NOISY_VALID: &str = "\
plugins {
    id 'java'
}

def projectName = 'demo'
String descriptor = \"build-${projectName}\"
final int retries = 3

if (project.hasProperty('release')) {
    version = '2.0.0'
}

for (module in ['app', 'core']) {
    println \"configuring ${module}\"
}

def names = ['alpha', 'beta']
names.each { name ->
    println name
}

if (descriptor =~ /build-.*/) {
    println 'matched'
}

repositories {
    mavenCentral()
}

dependencies {
    implementation 'org.apache.commons:commons-lang3:3.14.0'
    implementation libs.guava
    implementation group: 'com.google.guava', name: 'guava', version: '33.0.0'
}
";

const BROKEN: &str = "\
plugins {
    id 'java'
}

dependencies {
    implementation 'org.apache.commons:commons-lang3:3.14.0'
";

/// The user's real-world acceptance target (slashy regex + `from({...})` closure args).
const ACCEPTANCE: &str =
    include_str!("../tests/fixtures/groovy/acceptance/slay_the_spire2_build.gradle");

fn main() {
    println!("=== gradle-analyzer Groovy (.gradle) frontend demo ===\n");

    println!("########## 1. NOISY-BUT-VALID build.gradle ##########");
    let valid = parse_groovy(NOISY_VALID);
    report(NOISY_VALID, &valid);
    println!("\n{}\n", noisy_verdict(NOISY_VALID, &valid));

    println!("########## 2. BROKEN build.gradle (unclosed dependencies) ##########");
    let broken = parse_groovy(BROKEN);
    report(BROKEN, &broken);
    println!("\n{}", broken_verdict(BROKEN, &broken));

    println!("\n########## 3. ACCEPTANCE: real-world slay_the_spire2_build.gradle ##########");
    let real = parse_groovy(ACCEPTANCE);
    let red = SyntaxNode::new_root(real.green.clone());
    let round_trip = red.text() == ACCEPTANCE;
    println!("  file bytes   : {}", ACCEPTANCE.len());
    println!("  error_count  : {}", real.errors.len());
    println!("  round_trip   : {round_trip}");
    if !real.errors.is_empty() {
        for error in real.errors.as_slice() {
            println!(
                "    {:?} @ {}..{}",
                error.kind,
                error.span.start,
                error.span.end()
            );
        }
    }
    println!(
        "\n{}",
        if real.errors.is_empty() && round_trip {
            "PASS: real-world build.gradle parses with ZERO errors and round-trips exactly."
        } else {
            "CHECK: acceptance file did not reach 0 errors / exact round-trip."
        }
    );
}

/// Prints tree shape, round-trip check, and the typed error list for one parse.
fn report(source: &str, parse: &Parse) {
    let red = SyntaxNode::new_root(parse.green.clone());
    println!("--- tree shape (kind @ span) ---");
    print_node(&red, 0);
    println!("\n--- round-trip ---");
    println!("  text() == input : {}", red.text() == source);
    println!("\n--- typed errors (side table) ---");
    if parse.errors.is_empty() {
        println!("  (none)");
    } else {
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
}

fn print_node(node: &SyntaxNode, depth: usize) {
    let span = node.span();
    println!("{:indent$}{} @ {}..{}", "", kind_name(node.kind()), span.start, span.end(), indent = depth * 2);
    for child in node.children() {
        match child {
            SyntaxElement::Node(child_node) => print_node(child_node, depth + 1),
            SyntaxElement::Token(token) => {
                if token.kind().is_trivia() {
                    continue;
                }
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

/// The PASS line for the noisy-but-valid file.
fn noisy_verdict(source: &str, parse: &Parse) -> String {
    let red = SyntaxNode::new_root(parse.green.clone());
    let round_trips = red.text() == source;
    let malformed = parse
        .errors
        .as_slice()
        .iter()
        .filter(|e| e.kind == SyntaxErrorKind::MalformedBlock)
        .count();
    if round_trips && malformed == 0 {
        "PASS: noisy-but-valid Groovy round-trips with ZERO MalformedBlock errors.".into()
    } else {
        format!("CHECK: round_trips={round_trips} malformed_block_count={malformed}")
    }
}

/// The PASS line for the broken file.
fn broken_verdict(source: &str, parse: &Parse) -> String {
    let red = SyntaxNode::new_root(parse.green.clone());
    let round_trips = red.text() == source;
    let non_empty = !parse.green.children().is_empty();
    let anchored = parse
        .errors
        .as_slice()
        .iter()
        .find(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
        .map(|e| e.span.start != 0 && e.span.start <= source.len());
    match anchored {
        Some(true) if round_trips && non_empty => {
            "PASS: broken file gives a non-empty tree + last-token-anchored UnclosedBlock.".into()
        }
        other => format!(
            "CHECK: round_trips={round_trips} non_empty={non_empty} unclosed_anchored={other:?}"
        ),
    }
}

/// Renders Groovy custom kinds by name, deferring to the substrate names otherwise.
fn kind_name(kind: SyntaxKind) -> &'static str {
    match kind {
        STATEMENT => "STATEMENT",
        DECLARATION => "DECLARATION",
        ASSIGNMENT => "ASSIGNMENT",
        CALL => "CALL",
        ARG_LIST => "ARG_LIST",
        NAMED_ARG => "NAMED_ARG",
        CLOSURE => "CLOSURE",
        PATH => "PATH",
        LIST_LITERAL => "LIST_LITERAL",
        other => other.builtin_name(),
    }
}

/// The demo doubles as a gated smoke test so `cargo test` exercises both verdicts.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noisy_valid_has_zero_malformed_block_and_round_trips() {
        let parse = parse_groovy(NOISY_VALID);
        let red = SyntaxNode::new_root(parse.green.clone());
        assert_eq!(red.text(), NOISY_VALID, "noisy-valid round-trips");
        let malformed = parse
            .errors
            .as_slice()
            .iter()
            .filter(|e| e.kind == SyntaxErrorKind::MalformedBlock)
            .count();
        assert_eq!(malformed, 0, "ZERO MalformedBlock on noisy-but-valid Groovy");
    }

    #[test]
    fn broken_file_yields_non_empty_tree_and_anchored_unclosed_block() {
        let parse = parse_groovy(BROKEN);
        assert!(!parse.green.children().is_empty(), "non-empty tree");
        assert_eq!(parse.text(), BROKEN, "round-trip holds on broken input");
        let unclosed = parse
            .errors
            .as_slice()
            .iter()
            .find(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
            .expect("broken file reports UnclosedBlock");
        assert_ne!(unclosed.span.start, 0, "anchored, not EOF-zero");
    }

    #[test]
    fn acceptance_real_file_parses_with_zero_errors_and_round_trips() {
        let parse = parse_groovy(ACCEPTANCE);
        let red = SyntaxNode::new_root(parse.green.clone());
        assert_eq!(red.text(), ACCEPTANCE, "acceptance file round-trips exactly");
        assert!(
            parse.errors.is_empty(),
            "acceptance file must parse with ZERO errors, got {:?}",
            parse.errors.as_slice()
        );
    }
}
