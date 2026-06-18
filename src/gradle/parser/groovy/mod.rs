//! Tolerant Groovy DSL (`.gradle`) frontend over the shared syntax substrate.
//!
//! Handwritten resilient recursive descent that drives the shared
//! [`crate::gradle::syntax::Parser`] to a [`Parse`] for the supported Gradle Groovy NUCLEUS:
//! declaration prefixes (`def`/`static`/`final`/`var`/`@Anno`), optional-paren command-chain
//! calls (`id 'java'`, `apply plugin: 'x'`, `implementation 'g:a:v'`), trailing-closure calls
//! (`plugins { }`, `task foo { }`), maps/lists/GStrings (structural), and `libs.*` dotted
//! accessors. EVERYTHING out-of-nucleus (control flow, regex, closure chains, arbitrary
//! operators) degrades into a bounded [`crate::gradle::syntax::SyntaxKind::OPAQUE`] node —
//! never an abort and never a MalformedBlock flood. Typed errors are emitted ONLY for
//! genuinely malformed nucleus constructs (an unclosed `{`/`(` → `UnclosedBlock`, anchored to
//! the end of the last consumed token). No Groovy semantics (that is Task 7).

mod blocks;
mod calls;
mod slashy;

use crate::gradle::syntax::{Parse, Parser, SyntaxKind};

use blocks::parse_closure;
use calls::{parse_arg_list, parse_statement_core};

/// A whole top-level or in-closure statement.
pub const STATEMENT: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM);
/// A declaration with a stripped prefix (`def`/`static`/`final`/`var`/`@Anno`/typed local).
pub const DECLARATION: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 1);
/// An `lhs = rhs` assignment.
pub const ASSIGNMENT: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 2);
/// A method call: paren `f(args)`, command-chain `f a, b`, or trailing-closure `f { }`.
pub const CALL: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 3);
/// A parenthesized or bare argument list.
pub const ARG_LIST: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 4);
/// A `key: value` named argument.
pub const NAMED_ARG: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 5);
/// A `{ ... }` closure / configuration block.
pub const CLOSURE: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 6);
/// A dotted-path reference (`libs.junit.core`, `rootProject.name`).
pub const PATH: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 7);
/// A `[ ... ]` list literal.
pub const LIST_LITERAL: SyntaxKind = SyntaxKind::from_raw(SyntaxKind::FIRST_CUSTOM + 8);

/// Parses Groovy `.gradle` source into a tolerant [`Parse`] (green tree + typed errors).
///
/// Never panics and never aborts: the green tree always round-trips the input exactly, and
/// out-of-nucleus constructs degrade to opaque nodes rather than producing diagnostics.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::parser::parse_groovy;
///
/// let parse = parse_groovy("plugins {\n    id 'java'\n}\n");
/// assert_eq!(parse.text(), "plugins {\n    id 'java'\n}\n"); // exact round-trip
/// assert!(parse.errors.is_empty()); // valid Gradle Groovy parses cleanly
/// ```
pub fn parse_groovy(source: &str) -> Parse {
    let relexed = slashy::relex(source);
    Parser::with_tokens(source, relexed.tokens, relexed.errors).parse_with(|p| {
        while !p.at_eof() {
            parse_statement(p);
        }
    })
}

/// Parses one statement, dispatching declaration prefixes and control flow before the
/// general expression-headed path. Always makes at least one token of progress.
fn parse_statement(p: &mut Parser) {
    if at_decl_prefix(p) {
        parse_declaration(p);
        return;
    }
    if at_control_keyword(p) {
        parse_control_flow(p);
        return;
    }
    if p.at_text("{") {
        parse_closure(p);
        return;
    }
    if is_stray_delimiter(p) {
        p.bump();
        return;
    }
    parse_statement_core(p);
}

/// Consumes declaration prefixes (`@Anno`, `def`/`static`/`final`/`var`/visibility) and then
/// the statement they decorate, wrapping the whole in a [`DECLARATION`] node.
fn parse_declaration(p: &mut Parser) {
    let cp = p.checkpoint();
    loop {
        if p.at_text("@") {
            p.bump();
            if p.at(SyntaxKind::IDENT) {
                p.bump();
            }
            if p.at_text("(") {
                parse_arg_list(p);
            }
        } else if at_modifier(p) {
            p.bump();
        } else {
            break;
        }
    }
    if p.at_text("{") {
        parse_closure(p);
    } else if !p.at_eof() {
        parse_statement_core(p);
    }
    p.start_node_at(cp, DECLARATION);
    p.finish_node();
}

/// Collapses a control-flow head (keyword + condition) into one [`OPAQUE`] node, stopping at
/// a brace so the following `{ }` still parses as its block body.
fn parse_control_flow(p: &mut Parser) {
    p.start_node(SyntaxKind::OPAQUE);
    p.bump();
    while !p.at_eof() && !p.at_text("{") && !p.at_text("}") {
        p.bump();
    }
    p.finish_node();
}

/// Returns `true` if the current token begins a declaration prefix.
fn at_decl_prefix(p: &Parser) -> bool {
    p.at_text("@") || at_modifier(p)
}

/// Returns `true` if the current identifier is a declaration modifier keyword.
fn at_modifier(p: &Parser) -> bool {
    p.at(SyntaxKind::IDENT)
        && matches!(
            p.current_text(),
            Some("def" | "static" | "final" | "var" | "public" | "private" | "protected")
        )
}

/// Returns `true` if the current identifier begins a control-flow construct.
fn at_control_keyword(p: &Parser) -> bool {
    p.at(SyntaxKind::IDENT)
        && matches!(
            p.current_text(),
            Some(
                "if" | "else"
                    | "for"
                    | "while"
                    | "do"
                    | "switch"
                    | "try"
                    | "catch"
                    | "finally"
                    | "return"
                    | "throw"
                    | "assert"
            )
        )
}

/// Returns `true` at a stray closer/separator that should be tolerated as one opaque token.
fn is_stray_delimiter(p: &Parser) -> bool {
    p.at_text("}") || p.at_text(")") || p.at_text("]") || p.at_text(",") || p.at_text(";")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradle::syntax::{SyntaxElement, SyntaxErrorKind, SyntaxNode};
    use std::rc::Rc;

    fn count_kind(node: &SyntaxNode, kind: SyntaxKind, out: &mut usize) {
        if node.kind() == kind {
            *out += 1;
        }
        for child in node.children() {
            if let SyntaxElement::Node(n) = child {
                count_kind(n, kind, out);
            }
        }
    }

    fn kind_count(root: &Rc<SyntaxNode>, kind: SyntaxKind) -> usize {
        let mut n = 0;
        count_kind(root, kind, &mut n);
        n
    }

    fn malformed_count(parse: &Parse) -> usize {
        parse
            .errors
            .as_slice()
            .iter()
            .filter(|e| e.kind == SyntaxErrorKind::MalformedBlock)
            .count()
    }

    #[test]
    fn valid_build_gradle_parses_with_zero_errors_and_round_trips() {
        let source = include_str!("../../../../tests/fixtures/groovy/valid_build.gradle");
        let parse = parse_groovy(source);
        assert_eq!(parse.text(), source, "exact round-trip");
        assert!(
            parse.errors.is_empty(),
            "valid build.gradle must parse with ZERO errors, got {:?}",
            parse.errors.as_slice()
        );
        let root = SyntaxNode::new_root(parse.green.clone());
        // plugins/repositories/dependencies/task all surface as calls carrying closures.
        assert!(kind_count(&root, CLOSURE) >= 4, "expected >=4 closures (blocks)");
        assert!(kind_count(&root, CALL) >= 4, "expected >=4 calls");
    }

    #[test]
    fn valid_settings_gradle_parses_clean() {
        let source = include_str!("../../../../tests/fixtures/groovy/valid_settings.gradle");
        let parse = parse_groovy(source);
        assert_eq!(parse.text(), source, "exact round-trip");
        assert!(
            parse.errors.is_empty(),
            "valid settings.gradle must parse with ZERO errors, got {:?}",
            parse.errors.as_slice()
        );
        let root = SyntaxNode::new_root(parse.green.clone());
        // rootProject.name = '...' is an assignment; include ':app' is a command call.
        assert!(kind_count(&root, ASSIGNMENT) >= 1, "expected >=1 assignment");
        assert!(kind_count(&root, CALL) >= 1, "expected >=1 call (include)");
    }

    #[test]
    fn real_world_noise_produces_zero_false_malformed_block() {
        let source = include_str!("../../../../tests/fixtures/groovy/real_world_noise.gradle");
        let parse = parse_groovy(source);
        assert_eq!(parse.text(), source, "exact round-trip on noisy input");
        // The #1 prior failure mode: def / typed locals / if / for / regex / .each {} /
        // map+list punctuation must DEGRADE to opaque, never flood MalformedBlock.
        assert_eq!(
            malformed_count(&parse),
            0,
            "noisy-but-valid Groovy must yield ZERO MalformedBlock, got errors {:?}",
            parse.errors.as_slice()
        );
        // Stronger: a fully valid (if noisy) script should record NO recovery errors at all.
        assert!(
            parse.errors.is_empty(),
            "noisy-but-valid Groovy degrades silently to opaque, got {:?}",
            parse.errors.as_slice()
        );
        // It must still recognize the real nucleus underneath the noise.
        let root = SyntaxNode::new_root(parse.green.clone());
        assert!(kind_count(&root, CLOSURE) >= 2, "repositories/dependencies still parse");
    }

    #[test]
    fn unclosed_dependencies_yields_anchored_unclosed_block() {
        let source = include_str!("../../../../tests/fixtures/groovy/unclosed_dependencies.gradle");
        let parse = parse_groovy(source);
        // Tree is non-empty and still round-trips despite the malformed tail.
        assert!(!parse.green.children().is_empty(), "non-empty tree");
        assert_eq!(parse.text(), source, "round-trip holds on malformed input");

        let unclosed: Vec<_> = parse
            .errors
            .as_slice()
            .iter()
            .filter(|e| e.kind == SyntaxErrorKind::UnclosedBlock)
            .collect();
        assert_eq!(unclosed.len(), 1, "exactly one UnclosedBlock for the open dependencies brace");
        let span = unclosed[0].span;
        // Anchored to the END of the last consumed token, NOT zero and NOT raw EOF.
        assert_ne!(span.start, 0, "not an EOF-zero span");
        assert!(span.start > 0, "non-zero last-token-anchored start");
    }

    #[test]
    fn unclosed_paren_in_call_is_reported_not_panicked() {
        let parse = parse_groovy("implementation('g:a:v'\n");
        assert_eq!(parse.text(), "implementation('g:a:v'\n");
        assert!(
            parse
                .errors
                .as_slice()
                .iter()
                .any(|e| e.kind == SyntaxErrorKind::UnclosedBlock),
            "unclosed paren arg list reports UnclosedBlock, got {:?}",
            parse.errors.as_slice()
        );
    }

    #[test]
    fn messy_valid_round_trips_without_malformed_noise() {
        let source = include_str!("../../../../tests/fixtures/groovy/messy_valid.gradle");
        let parse = parse_groovy(source);
        assert_eq!(parse.text(), source, "exact round-trip on messy input");
        assert_eq!(
            malformed_count(&parse),
            0,
            "messy-but-valid yields zero MalformedBlock, got {:?}",
            parse.errors.as_slice()
        );
    }

    #[test]
    fn truncated_and_adversarial_inputs_never_panic() {
        for source in ["", "{", "dependencies {", "}}}", "id 'java", "foo(((", "a.b.c.d"] {
            let parse = parse_groovy(source);
            assert_eq!(parse.text(), source, "round-trip holds for {source:?}");
            let _ = SyntaxNode::new_root(parse.green.clone());
        }
    }

    #[test]
    fn command_chain_call_is_recognized() {
        // `id 'java'` is the canonical optional-paren command call.
        let parse = parse_groovy("id 'java'\n");
        assert!(parse.errors.is_empty(), "clean command call, got {:?}", parse.errors.as_slice());
        let root = SyntaxNode::new_root(parse.green.clone());
        assert_eq!(kind_count(&root, CALL), 1, "one command-chain call");
        assert_eq!(parse.text(), "id 'java'\n");
    }

    #[test]
    fn libs_dotted_accessor_parses_as_path() {
        let parse = parse_groovy("implementation libs.junit.jupiter\n");
        assert!(parse.errors.is_empty(), "clean libs accessor, got {:?}", parse.errors.as_slice());
        assert_eq!(parse.text(), "implementation libs.junit.jupiter\n");
    }

    #[test]
    fn acceptance_real_world_build_gradle_parses_with_zero_errors() {
        let source =
            include_str!("../../../../tests/fixtures/groovy/acceptance/slay_the_spire2_build.gradle");
        let parse = parse_groovy(source);
        assert_eq!(parse.text(), source, "acceptance file must round-trip exactly");
        assert!(
            parse.errors.is_empty(),
            "ACCEPTANCE TARGET: real-world build.gradle must parse with ZERO errors, got {:?}",
            parse.errors.as_slice()
        );
    }

    #[test]
    fn slashy_regex_with_embedded_quotes_is_zero_errors() {
        let parse = parse_groovy("def m = s =~ /\"path\".*?\"([^\"]*)\"/\n");
        assert_eq!(parse.text(), "def m = s =~ /\"path\".*?\"([^\"]*)\"/\n");
        assert!(
            parse.errors.is_empty(),
            "a slashy regex with embedded quotes must not flood errors, got {:?}",
            parse.errors.as_slice()
        );
    }

    #[test]
    fn simple_slashy_string_is_zero_errors() {
        let parse = parse_groovy("def m = s =~ /abc/\n");
        assert_eq!(parse.text(), "def m = s =~ /abc/\n");
        assert!(parse.errors.is_empty(), "simple slashy, got {:?}", parse.errors.as_slice());
    }

    #[test]
    fn division_is_not_a_slashy_string() {
        let parse = parse_groovy("def r = a / b / c\n");
        assert_eq!(parse.text(), "def r = a / b / c\n");
        assert!(parse.errors.is_empty(), "division stays division, got {:?}", parse.errors.as_slice());
    }

    #[test]
    fn comments_are_not_slashy_strings() {
        let parse = parse_groovy("// hi\n/* block */\ndef x = 1\n");
        assert_eq!(parse.text(), "// hi\n/* block */\ndef x = 1\n");
        assert!(parse.errors.is_empty(), "comments untouched, got {:?}", parse.errors.as_slice());
    }

    #[test]
    fn closure_literal_as_call_argument_is_zero_errors() {
        let parse = parse_groovy("from({\n  resolveBuiltDllPath()\n})\n");
        assert_eq!(parse.text(), "from({\n  resolveBuiltDllPath()\n})\n");
        assert!(
            parse.errors.is_empty(),
            "closure literal `from({{...}})` must parse cleanly, got {:?}",
            parse.errors.as_slice()
        );
    }

    #[test]
    fn unterminated_slashy_degrades_without_flood() {
        // No closing `/` before EOL: degrade gracefully (no panic, no error flood).
        let parse = parse_groovy("def m = s =~ /abc\n");
        assert_eq!(parse.text(), "def m = s =~ /abc\n", "round-trip preserved");
        assert!(
            parse.errors.len() <= 1,
            "unterminated slashy degrades to at most one tolerant error, got {:?}",
            parse.errors.as_slice()
        );
    }
}
