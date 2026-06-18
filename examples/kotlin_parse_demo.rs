//! Manual-QA demo for the Task 5 tolerant Kotlin-DSL (`.gradle.kts`) frontend.
//!
//! Run with:
//!
//! ```text
//! cargo run --example kotlin_parse_demo
//! ```
//!
//! It parses TWO inputs over the real public [`parse_kotlin`] API and prints, for each:
//!
//! 1. the tree shape (indented Kotlin/built-in kinds + absolute spans),
//! 2. whether `text()` round-trips the input exactly, and
//! 3. the typed error list with spans + localized message keys.
//!
//! The binary-observable PASS is: the VALID real-world-style script parses with 0 errors and
//! an exact round-trip; the BROKEN script yields a non-empty tree plus a typed error with a
//! non-zero last-token-anchored span; and an out-of-nucleus construct shows up as an `OPAQUE`
//! node with NO false error.

use gradle_analyzer::gradle::parser::kotlin::kinds::kind_name;
use gradle_analyzer::gradle::parser::parse_kotlin;
use gradle_analyzer::gradle::syntax::{Parse, SyntaxElement, SyntaxKind, SyntaxNode};

/// A representative real-world-style `build.gradle.kts` (valid nucleus) with an out-of-nucleus
/// `if` block to demonstrate opaque degradation alongside clean nucleus parsing.
const VALID: &str = "\
import org.gradle.api.tasks.testing.Test

plugins {
    kotlin(\"jvm\") version \"1.9.22\"
    id(\"com.diffplug.spotless\") version \"6.25.0\"
}

group = \"com.example\"
version = \"1.0.0\"

if (project.hasProperty(\"ci\")) {
    println(\"ci build\")
}

repositories {
    mavenCentral()
}

dependencies {
    implementation(\"org.jetbrains.kotlin:kotlin-stdlib:1.9.22\")
    implementation(libs.guava)
    implementation(libs.bundles.networking)
}

tasks.register<Test>(\"integrationTest\") {
    useJUnitPlatform()
}
";

/// A deliberately broken script: the `dependencies {` block is never closed.
const BROKEN: &str = "\
plugins {
    kotlin(\"jvm\")
}

dependencies {
    implementation(\"org.example:lib:1.0\")
";

fn main() {
    println!("=== gradle-analyzer Kotlin-DSL frontend demo ===\n");

    println!("########## CASE 1: VALID build.gradle.kts (with an out-of-nucleus if) ##########");
    report("valid", VALID);

    println!("\n########## CASE 2: BROKEN build.gradle.kts (unclosed dependencies) ##########");
    report("broken", BROKEN);
}

/// Parses `source`, prints the tree shape / round-trip / errors, then the verdict.
fn report(label: &str, source: &str) {
    println!("input ({} bytes):\n{source}", source.len());
    let parse = parse_kotlin(source);

    println!("--- tree shape (kind @ span) ---");
    let root = SyntaxNode::new_root(parse.green.clone());
    print_node(&root, 0);

    let round_trips = root.text() == source;
    println!("\n--- round-trip ---");
    println!("  text() == input : {round_trips}");

    println!("--- typed errors (side table) ---");
    print_errors(&parse);

    println!("\n{}", verdict(label, source, &parse));
}

/// Prints the indented tree (Kotlin + built-in kind names with absolute spans).
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
        if let SyntaxElement::Node(child_node) = child {
            print_node(child_node, depth + 1);
        }
    }
}

/// Prints the typed error side table with spans and localized message keys.
fn print_errors(parse: &Parse) {
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

/// The binary-observable verdict line for each case.
fn verdict(label: &str, source: &str, parse: &Parse) -> String {
    let root = SyntaxNode::new_root(parse.green.clone());
    let round_trips = root.text() == source;
    let has_opaque = has_kind(&root, SyntaxKind::OPAQUE);

    match label {
        "valid" => {
            if round_trips && parse.errors.is_empty() && has_opaque {
                "PASS(valid): 0 errors, exact round-trip, out-of-nucleus `if` shown as OPAQUE."
                    .into()
            } else {
                format!(
                    "CHECK(valid): round_trips={round_trips} errors={} opaque={has_opaque}",
                    parse.errors.len()
                )
            }
        }
        _ => {
            let anchored = parse.errors.as_slice().iter().find_map(|e| {
                (e.span.start != 0 && e.span.start != source.len()).then_some(e.span.start)
            });
            let non_empty = !parse.green.children().is_empty();
            match anchored {
                Some(at) if round_trips && non_empty => format!(
                    "PASS(broken): non-empty tree, round-trip holds, typed error anchored at \
                     byte {at} (non-zero, not raw EOF {}).",
                    source.len()
                ),
                other => format!(
                    "CHECK(broken): round_trips={round_trips} non_empty={non_empty} anchored={other:?}"
                ),
            }
        }
    }
}

/// Returns `true` if any node in the subtree has `kind`.
fn has_kind(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.kind() == kind || node.child_nodes().any(|c| has_kind(&c, kind))
}

/// The demo doubles as a smoke test so `cargo test` exercises both observable PASS paths.
#[cfg(test)]
mod tests {
    use super::*;
    use gradle_analyzer::gradle::syntax::SyntaxErrorKind;

    #[test]
    fn valid_case_round_trips_with_zero_errors_and_an_opaque_region() {
        let parse = parse_kotlin(VALID);
        let root = SyntaxNode::new_root(parse.green.clone());
        assert_eq!(root.text(), VALID, "valid round-trip");
        assert!(parse.errors.is_empty(), "valid has zero errors");
        assert!(has_kind(&root, SyntaxKind::OPAQUE), "out-of-nucleus if is opaque");
    }

    #[test]
    fn broken_case_yields_non_empty_tree_and_anchored_typed_error() {
        let parse = parse_kotlin(BROKEN);
        assert_eq!(parse.text(), BROKEN, "broken round-trip");
        assert!(!parse.green.children().is_empty(), "broken tree non-empty");

        let unclosed = parse
            .errors
            .as_slice()
            .iter()
            .find(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
            .expect("unclosed dependencies block yields UnclosedBlock");
        assert_ne!(unclosed.span.start, 0, "not an EOF-zero span");
        assert_ne!(unclosed.span.start, BROKEN.len(), "anchored to last token, not raw EOF");
    }
}
