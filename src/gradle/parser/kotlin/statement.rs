//! Top-level and block-level statement dispatch for the Kotlin-DSL nucleus.
//!
//! [`parse_statement`] is the single dispatch shared by the document root and every block
//! body, so nucleus constructs parse uniformly at any depth. It recognizes imports, package
//! headers, assignments, and generic calls; routes non-nucleus keywords and unrecognized
//! shapes to the bounded opaque fallback; and folds plugin infix suffixes (`version "x"`,
//! `apply false`) into their call so a plugin line stays one node.

use crate::gradle::syntax::{Parser, SyntaxKind};

use super::blocks::{bump_opaque_balanced, parse_block};
use super::expr::{parse_access_path, parse_arg_list, parse_index, parse_type_args, parse_value};
use super::kinds;

/// Plugin-spec infix keywords folded into the preceding call (`id("x") version "1"`).
const PLUGIN_INFIX_KEYWORDS: &[&str] = &["version", "apply"];

/// Parses one statement at the current position, always making forward progress.
///
/// Dispatches on the leading token: `import`/`package` headers, non-nucleus keywords (to
/// the opaque fallback), backtick-quoted identifiers (`` `kotlin-dsl` ``), access-path-led
/// calls/assignments, bare blocks, stray separators, and otherwise the opaque fallback.
pub(super) fn parse_statement(p: &mut Parser) {
    if p.at(SyntaxKind::IDENT) {
        let text = p.current_text().unwrap_or_default();
        if text == "import" {
            parse_header(p, kinds::IMPORT);
            return;
        }
        if text == "package" {
            parse_header(p, kinds::PACKAGE);
            return;
        }
        if kinds::is_non_nucleus_keyword(text) {
            bump_opaque_balanced(p);
            return;
        }
        parse_path_statement(p);
        return;
    }
    if p.at_text("`") {
        parse_backtick_call(p);
        return;
    }
    if p.at_text("{") {
        parse_block(p);
        return;
    }
    if p.at_text(";") || p.at_text(",") {
        p.bump();
        return;
    }
    bump_opaque_balanced(p);
}

/// Parses an `import`/`package` dotted header with an optional `.*` and `as` alias.
fn parse_header(p: &mut Parser, kind: SyntaxKind) {
    p.start_node(kind);
    p.bump();
    if p.at(SyntaxKind::IDENT) {
        p.bump();
    }
    while p.at_text(".") {
        p.bump();
        if p.at(SyntaxKind::IDENT) || p.at_text("*") {
            p.bump();
        } else {
            break;
        }
    }
    if p.at(SyntaxKind::IDENT) && p.current_text() == Some("as") {
        p.bump();
        if p.at(SyntaxKind::IDENT) {
            p.bump();
        }
    }
    p.finish_node();
}

/// Parses an access-path-led statement: a call (with optional type args, args, lambda) or
/// an assignment (`lhs = value`, including `extra["x"] = ...`).
fn parse_path_statement(p: &mut Parser) {
    let cp = p.checkpoint();
    parse_access_path(p);
    while p.at_text("[") {
        parse_index(p);
    }

    let mut is_call = false;
    if p.at_text("<") {
        parse_type_args(p);
        is_call = true;
    }
    if p.at_text("(") {
        parse_arg_list(p);
        is_call = true;
    }
    if p.at_text("{") {
        parse_block(p);
        is_call = true;
    }

    if p.at_text("=") {
        p.bump();
        parse_value(p);
        p.start_node_at(cp, kinds::ASSIGNMENT);
        p.finish_node();
        return;
    }

    if is_call {
        parse_plugin_suffixes(p);
    }
    p.start_node_at(cp, kinds::CALL);
    p.finish_node();
}

/// Folds `version "x"` / `apply false` infix suffixes into the current plugin call.
fn parse_plugin_suffixes(p: &mut Parser) {
    while p.at(SyntaxKind::IDENT)
        && p
            .current_text()
            .is_some_and(|t| PLUGIN_INFIX_KEYWORDS.contains(&t))
    {
        p.start_node(kinds::PLUGIN_SUFFIX);
        p.bump();
        parse_value(p);
        p.finish_node();
    }
}

/// Parses a backtick-quoted identifier (`` `kotlin-dsl` ``) as a bare call.
///
/// The substrate lexer emits each backtick as a lone `ERROR` token (no side-table error),
/// so this groups `` ` ... ` `` into one access path and wraps it as a `CALL`, keeping the
/// surrounding block parsing as clean statements instead of one swallowed opaque run.
fn parse_backtick_call(p: &mut Parser) {
    let cp = p.checkpoint();
    p.start_node(kinds::ACCESS_PATH);
    p.bump();
    while !p.at_eof() && !p.at_text("`") {
        p.bump();
    }
    if p.at_text("`") {
        p.bump();
    }
    p.finish_node();
    p.start_node_at(cp, kinds::CALL);
    p.finish_node();
}
