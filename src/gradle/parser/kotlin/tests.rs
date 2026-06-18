//! Behavior specification for the tolerant Kotlin-DSL frontend.
//!
//! Written failing-first against the STUB: the round-trip assertions pass immediately
//! (the substrate is lossless), but every assertion about nucleus nodes, opaque fallback,
//! and typed errors fails until the real grammar is implemented.

use super::blocks::bump_opaque_balanced;
use super::kinds;
use super::parse_kotlin;
use crate::gradle::syntax::{Parse, Parser, SyntaxErrorKind, SyntaxKind, SyntaxNode};

const VALID_BUILD: &str = include_str!("../../../../tests/fixtures/kotlin/valid_build.gradle.kts");
const VALID_SETTINGS: &str =
    include_str!("../../../../tests/fixtures/kotlin/valid_settings.gradle.kts");
const MESSY_VALID: &str = include_str!("../../../../tests/fixtures/kotlin/messy_valid.gradle.kts");
const OUT_OF_NUCLEUS: &str =
    include_str!("../../../../tests/fixtures/kotlin/out_of_nucleus.gradle.kts");
const MALFORMED: &str =
    include_str!("../../../../tests/fixtures/kotlin/malformed_unclosed.gradle.kts");

/// Reconstructs the source from the red tree (the strongest round-trip check).
fn red_round_trips(source: &str, parse: &Parse) -> bool {
    SyntaxNode::new_root(parse.green.clone()).text() == source
}

/// Recursively counts nodes of `kind` anywhere in the tree.
fn count_kind(node: &SyntaxNode, kind: SyntaxKind) -> usize {
    let here = usize::from(node.kind() == kind);
    here + node.child_nodes().map(|c| count_kind(&c, kind)).sum::<usize>()
}

/// Recursively collects the text of every node of `kind`.
fn texts_of_kind(node: &SyntaxNode, kind: SyntaxKind) -> Vec<String> {
    let mut out = Vec::new();
    if node.kind() == kind {
        out.push(node.text());
    }
    for child in node.child_nodes() {
        out.extend(texts_of_kind(&child, kind));
    }
    out
}

#[test]
fn valid_build_parses_with_zero_errors_and_round_trips() {
    let parse = parse_kotlin(VALID_BUILD);
    assert!(
        parse.errors.is_empty(),
        "valid build should have zero errors, got {:?}",
        parse.errors.as_slice()
    );
    assert_eq!(parse.text(), VALID_BUILD, "green round-trip");
    assert!(red_round_trips(VALID_BUILD, &parse), "red round-trip");

    let root = SyntaxNode::new_root(parse.green.clone());
    // plugins, repositories, dependencies, tasks.register, tasks.named => several CALLs.
    assert!(count_kind(&root, kinds::CALL) >= 5, "nucleus calls recognized");
    assert!(count_kind(&root, kinds::IMPORT) >= 1, "import recognized");
    assert!(count_kind(&root, kinds::ASSIGNMENT) >= 2, "group/version assignments");
}

#[test]
fn valid_settings_parses_cleanly() {
    let parse = parse_kotlin(VALID_SETTINGS);
    assert!(
        parse.errors.is_empty(),
        "valid settings should have zero errors, got {:?}",
        parse.errors.as_slice()
    );
    assert!(red_round_trips(VALID_SETTINGS, &parse));

    let root = SyntaxNode::new_root(parse.green.clone());
    // pluginManagement, dependencyResolutionManagement, include x3 => CALLs.
    assert!(count_kind(&root, kinds::CALL) >= 4);
    // rootProject.name = "..." => an assignment.
    assert!(count_kind(&root, kinds::ASSIGNMENT) >= 1);
}

#[test]
fn dependency_notation_both_string_and_libs_accessor_parse() {
    let parse = parse_kotlin(VALID_BUILD);
    assert!(parse.errors.is_empty());
    let root = SyntaxNode::new_root(parse.green.clone());
    let call_texts = texts_of_kind(&root, kinds::CALL);
    assert!(
        call_texts.iter().any(|t| t.contains("implementation(\"org.jetbrains")),
        "string-notation dependency is a CALL"
    );
    assert!(
        call_texts.iter().any(|t| t.contains("implementation(libs.guava")),
        "libs.* accessor dependency is a CALL"
    );
    assert!(
        call_texts.iter().any(|t| t.contains("libs.bundles.networking")),
        "libs.bundles.* accessor dependency is a CALL"
    );
}

#[test]
fn missing_close_brace_yields_tree_plus_anchored_unclosed_error() {
    let parse = parse_kotlin(MALFORMED);

    // Non-empty tree that still round-trips despite the error.
    assert!(!parse.green.children().is_empty());
    assert!(red_round_trips(MALFORMED, &parse));

    let unclosed: Vec<_> = parse
        .errors
        .as_slice()
        .iter()
        .filter(|e| {
            e.kind == SyntaxErrorKind::UnclosedBlock || e.kind == SyntaxErrorKind::MalformedBlock
        })
        .collect();
    assert_eq!(unclosed.len(), 1, "exactly one unclosed/malformed block error");

    let span = unclosed[0].span;
    assert_ne!(span.start, 0, "not an EOF-zero span");
    assert_ne!(span.start, MALFORMED.len(), "anchored to last token, not raw EOF");
    // Anchored to the END of the last consumed non-trivia token (the `)` of the dep call),
    // which precedes the trailing newline.
    let trimmed_end = MALFORMED.trim_end().len();
    assert_eq!(span.start, trimmed_end, "anchored to end of last consumed token");
}

#[test]
fn out_of_nucleus_constructs_become_opaque_without_false_errors() {
    let parse = parse_kotlin(OUT_OF_NUCLEUS);
    assert!(
        parse.errors.is_empty(),
        "opaque regions must NOT raise false errors, got {:?}",
        parse.errors.as_slice()
    );
    assert!(red_round_trips(OUT_OF_NUCLEUS, &parse));

    let root = SyntaxNode::new_root(parse.green.clone());
    assert!(
        count_kind(&root, SyntaxKind::OPAQUE) >= 1,
        "if/fun degrade to at least one OPAQUE node"
    );
    // Parsing CONTINUED: the trailing nucleus blocks after the opaque region still parse as
    // real CALLs (their block bodies parsed, not swallowed into the opaque run). Leading
    // comment/newline trivia attaches to the following node, so match on substrings.
    let call_texts = texts_of_kind(&root, kinds::CALL);
    assert!(
        call_texts
            .iter()
            .any(|t| t.contains("repositories") && t.contains("mavenCentral()")),
        "repositories block after the opaque region still parses as a CALL"
    );
    assert!(
        call_texts
            .iter()
            .any(|t| t.contains("dependencies") && t.contains("implementation(")),
        "dependencies block after the opaque region still parses as a CALL"
    );
}

#[test]
fn messy_but_valid_round_trips_with_no_errors() {
    let parse = parse_kotlin(MESSY_VALID);
    assert!(
        parse.errors.is_empty(),
        "messy-but-valid should have zero errors, got {:?}",
        parse.errors.as_slice()
    );
    assert!(red_round_trips(MESSY_VALID, &parse));
}

#[test]
fn inline_minimal_build_recognizes_each_nucleus_construct() {
    let source = "\
import a.b.C
group = \"g\"
version = \"1.0\"
plugins {
    id(\"x\") version \"2.0\"
    kotlin(\"jvm\")
}
dependencies {
    implementation(\"a:b:1.0\")
    implementation(libs.foo)
}
tasks.register<Jar>(\"jar\") {
    archiveBaseName.set(\"app\")
}
extra[\"k\"] = \"v\"
";
    let parse = parse_kotlin(source);
    assert!(parse.errors.is_empty(), "errors: {:?}", parse.errors.as_slice());
    assert_eq!(parse.text(), source);

    let root = SyntaxNode::new_root(parse.green.clone());
    assert_eq!(count_kind(&root, kinds::IMPORT), 1);
    assert!(count_kind(&root, kinds::ASSIGNMENT) >= 3, "group, version, extra[]");
    assert!(count_kind(&root, kinds::CALL) >= 3, "plugins, dependencies, tasks.register");
    assert!(count_kind(&root, kinds::TYPE_ARGS) >= 1, "register<Jar>");
}

// --- Adversarial: malformed input must yield tree + typed error, never panic / hang. ---

#[test]
fn truncated_and_unbalanced_inputs_never_panic_and_round_trip() {
    let cases = [
        "",
        "dependencies {",
        "plugins {\n  id(\"x\"",
        "tasks.register<Test>(",
        "group =",
        "implementation(\"a:b:1.0\"",
        "}}} extra[",
        "import",
    ];
    for source in cases {
        let parse = parse_kotlin(source);
        assert_eq!(parse.text(), source, "round-trip holds for {source:?}");
        let _ = SyntaxNode::new_root(parse.green.clone());
    }
}

#[test]
fn unterminated_string_in_dependency_coord_surfaces_an_error() {
    let source = "dependencies {\n    implementation(\"org.example:lib:1.0)\n}\n";
    let parse = parse_kotlin(source);
    assert_eq!(parse.text(), source, "round-trip holds even with an unterminated string");
    assert!(
        parse
            .errors
            .as_slice()
            .iter()
            .any(|e| e.kind == SyntaxErrorKind::UnterminatedString),
        "lexer's unterminated-string error flows through the parse"
    );
}

#[test]
fn deeply_nested_blocks_do_not_overflow_or_error() {
    let mut source = String::new();
    for _ in 0..40 {
        source.push_str("allprojects {\n");
    }
    for _ in 0..40 {
        source.push_str("}\n");
    }
    let parse = parse_kotlin(&source);
    assert_eq!(parse.text(), source);
    assert!(parse.errors.is_empty(), "well-nested blocks are clean");
}

#[test]
fn opaque_consumer_makes_progress_and_emits_no_error() {
    // Drive the opaque consumer directly on a non-nucleus run.
    let source = "weird @#$ tokens here";
    let parse = Parser::new(source).parse_with(|p| {
        while !p.at_eof() {
            bump_opaque_balanced(p);
        }
    });
    assert_eq!(parse.text(), source);
    assert!(parse.errors.is_empty(), "opaque run is tolerated, not malformed");
    let root = SyntaxNode::new_root(parse.green.clone());
    assert!(count_kind(&root, SyntaxKind::OPAQUE) >= 1);
}

#[test]
fn syntax_error_kinds_map_to_localized_message_keys() {
    // The frontend reuses the substrate's SyntaxErrorKind -> MessageKey mapping; prove the
    // mapping is reachable from a real parse (no raw English in the frontend).
    let parse = parse_kotlin(MALFORMED);
    for error in parse.errors.as_slice() {
        let name = error.message_key().canonical_name();
        assert!(name.starts_with("syntax."), "{name} is a localized syntax key");
    }
}
