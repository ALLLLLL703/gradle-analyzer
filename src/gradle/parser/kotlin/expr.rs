//! Kotlin-DSL expression parsing: access paths, argument lists, type arguments, values.
//!
//! These are the leaf grammar pieces the statement layer composes. Every function drives
//! the shared substrate [`Parser`] and is tolerant: a missing closing delimiter anchors one
//! [`SyntaxErrorKind::UnclosedBlock`] to the last consumed token (via the substrate's
//! EOF-anchored recovery) and a value it does not model degrades to a bounded opaque run,
//! so valid input parses with zero errors and malformed input never aborts the parse.

use crate::gradle::syntax::{Parser, SyntaxErrorKind, SyntaxKind};

use super::blocks::{bump_opaque_balanced, parse_block};
use super::kinds;

/// Parses a dotted access path (`libs.bundles.x`, `tasks`, `rootProject.name`).
///
/// Consumes a leading identifier then any number of `.segment` continuations, stopping
/// before call/index/type-argument suffixes so the caller can decide their meaning.
pub(super) fn parse_access_path(p: &mut Parser) {
    p.start_node(kinds::ACCESS_PATH);
    if p.at(SyntaxKind::IDENT) {
        p.bump();
    }
    while p.at_text(".") {
        p.bump();
        if p.at(SyntaxKind::IDENT) {
            p.bump();
        } else {
            break;
        }
    }
    p.finish_node();
}

/// Parses a single value: literal, access path with call/index/lambda suffixes, or a list.
///
/// Anything it cannot classify degrades to a bounded opaque run so the parse continues.
pub(super) fn parse_value(p: &mut Parser) {
    if p.at(SyntaxKind::STRING) || p.at(SyntaxKind::NUMBER) {
        p.bump();
        return;
    }
    if p.at(SyntaxKind::IDENT) {
        parse_access_path(p);
        parse_call_suffixes(p);
        return;
    }
    if p.at_text("[") {
        parse_list(p);
        return;
    }
    if p.at_text("{") {
        parse_block(p);
        return;
    }
    bump_opaque_balanced(p);
}

/// Consumes trailing `<...>`, `(...)`, `[...]`, and `{ lambda }` suffixes after a path.
fn parse_call_suffixes(p: &mut Parser) {
    loop {
        if p.at_text("<") {
            parse_type_args(p);
        } else if p.at_text("(") {
            parse_arg_list(p);
        } else if p.at_text("[") {
            parse_index(p);
        } else if p.at_text("{") {
            parse_block(p);
        } else {
            break;
        }
    }
}

/// Parses a parenthesized argument list, tolerating trailing/stray commas and named args.
pub(super) fn parse_arg_list(p: &mut Parser) {
    p.start_node(kinds::ARG_LIST);
    p.bump();
    loop {
        if p.at_eof() {
            p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
            break;
        }
        if p.at_text(")") {
            p.bump();
            break;
        }
        if p.at_text(",") {
            p.bump();
            continue;
        }
        parse_arg(p);
    }
    p.finish_node();
}

/// Parses one argument, recognizing `name = value` as a named argument.
fn parse_arg(p: &mut Parser) {
    let cp = p.checkpoint();
    parse_value(p);
    if p.at_text("=") {
        p.bump();
        parse_value(p);
        p.start_node_at(cp, kinds::NAMED_ARG);
        p.finish_node();
    }
}

/// Parses a structural type-argument list (`<Test>`, `<Map<String, Int>>`).
///
/// Tracks nested angle depth and bails tolerantly if it runs into a `(`/`{` (which means
/// the `<` was not a type-argument list after all), so it can never eat the whole file.
pub(super) fn parse_type_args(p: &mut Parser) {
    p.start_node(kinds::TYPE_ARGS);
    p.bump();
    let mut depth = 1u32;
    loop {
        if p.at_eof() {
            p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
            break;
        }
        if p.at_text("<") {
            depth += 1;
            p.bump();
        } else if p.at_text(">") {
            depth -= 1;
            p.bump();
            if depth == 0 {
                break;
            }
        } else if p.at_text("(") || p.at_text("{") || p.at_text(";") {
            break;
        } else {
            p.bump();
        }
    }
    p.finish_node();
}

/// Parses a bracket index suffix (`["x"]`).
pub(super) fn parse_index(p: &mut Parser) {
    parse_bracketed(p, kinds::INDEX);
}

/// Parses a bracket list literal (`[a, b]`).
pub(super) fn parse_list(p: &mut Parser) {
    parse_bracketed(p, kinds::LIST);
}

/// Shared `[ ... ]` body parser for both index and list, tolerant of stray commas.
fn parse_bracketed(p: &mut Parser, kind: SyntaxKind) {
    p.start_node(kind);
    p.bump();
    loop {
        if p.at_eof() {
            p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
            break;
        }
        if p.at_text("]") {
            p.bump();
            break;
        }
        if p.at_text(",") {
            p.bump();
            continue;
        }
        parse_value(p);
    }
    p.finish_node();
}
