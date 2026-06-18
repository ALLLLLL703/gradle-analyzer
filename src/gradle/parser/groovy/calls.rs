//! Expression, call, and argument parsing — the optional-paren command-chain core.
//!
//! The hard part of Groovy is the paren-less call `methodName arg1, arg2`. The disambiguation
//! that keeps this tolerant (and never floods MalformedBlock) is two rules:
//! a bare-argument list only CONTINUES across a `,` (so `id 'java'` on one line and
//! `id 'app'` on the next stay separate statements), and a trailing closure is checked
//! BEFORE bare args. Anything unrecognized degrades into bumped tokens, never an error.

use crate::gradle::syntax::{Parser, SyntaxErrorKind, SyntaxKind};

use super::blocks::parse_closure;
use super::{ARG_LIST, ASSIGNMENT, CALL, LIST_LITERAL, NAMED_ARG, PATH};

/// What an expression head turned out to be — drives the statement-level dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Head {
    /// A bare identifier: a command-target and assignment-target candidate.
    Ref,
    /// A dotted path (`libs.junit.core`): assignment-target, not a command target.
    Path,
    /// An expression that already ended in a `(...)`/`[...]` call.
    Called,
    /// A literal, list, group, or anything else.
    Other,
}

/// Parses one statement whose head is an expression (post prefix/control-flow handling).
///
/// Dispatches the head into an assignment, a trailing-closure call, a command-chain call,
/// or a plain expression statement — wrapping the recognized shape in a typed node.
pub(super) fn parse_statement_core(p: &mut Parser) {
    let cp = p.checkpoint();
    let head = parse_expression(p);

    if p.at_text("=") {
        p.bump();
        if !at_value_terminator(p) {
            parse_value(p);
        }
        p.start_node_at(cp, ASSIGNMENT);
        p.finish_node();
        return;
    }

    if p.at_text("{") {
        parse_closure(p);
        p.start_node_at(cp, CALL);
        p.finish_node();
        return;
    }

    if head == Head::Ref && starts_bare_arg(p) {
        parse_bare_args(p);
        if p.at_text("{") {
            parse_closure(p);
        }
        p.start_node_at(cp, CALL);
        p.finish_node();
        return;
    }

    if head == Head::Called {
        p.start_node_at(cp, CALL);
        p.finish_node();
    }
}

/// Parses a value: an atom-with-postfix followed by any coalesced binary-operator tail.
pub(super) fn parse_value(p: &mut Parser) {
    parse_expression(p);
    while cur_is_op(p) {
        bump_op_run(p);
        if at_value_start(p) {
            parse_expression(p);
        } else {
            break;
        }
    }
}

/// Parses an atom plus its postfix `.member` / `(args)` / `[index]` chain.
fn parse_expression(p: &mut Parser) -> Head {
    let cp = p.checkpoint();
    let atom = parse_atom(p);
    let mut saw_dot = false;
    let mut saw_call = false;
    loop {
        if p.at_text(".") {
            p.bump();
            if p.at(SyntaxKind::IDENT) || p.at(SyntaxKind::STRING) {
                p.bump();
            }
            saw_dot = true;
        } else if p.at_text("(") {
            parse_arg_list(p);
            saw_call = true;
        } else if p.at_text("[") {
            parse_bracketed(p);
            saw_call = true;
        } else {
            break;
        }
    }
    if saw_call {
        return Head::Called;
    }
    if saw_dot && atom == Head::Ref {
        p.start_node_at(cp, PATH);
        p.finish_node();
        return Head::Path;
    }
    atom
}

/// Parses a single atom; always makes one token of progress when not at EOF.
fn parse_atom(p: &mut Parser) -> Head {
    if p.at(SyntaxKind::IDENT) {
        p.bump();
        return Head::Ref;
    }
    if p.at(SyntaxKind::STRING) || p.at(SyntaxKind::NUMBER) {
        p.bump();
        return Head::Other;
    }
    if p.at_text("[") {
        parse_bracketed(p);
        return Head::Other;
    }
    if p.at_text("(") {
        parse_arg_list(p);
        return Head::Other;
    }
    p.bump_any();
    Head::Other
}

/// Parses a parenthesized argument list `( ... )`, reporting an unclosed paren.
pub(super) fn parse_arg_list(p: &mut Parser) {
    p.start_node(ARG_LIST);
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
        if p.at_text("}") || p.at_text("]") {
            p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
            break;
        }
        parse_arg(p);
    }
    p.finish_node();
}

/// Parses a bracketed list/map literal `[ ... ]`, reporting an unclosed bracket.
fn parse_bracketed(p: &mut Parser) {
    p.start_node(LIST_LITERAL);
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
        if p.at_text("}") || p.at_text(")") {
            p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
            break;
        }
        parse_arg(p);
    }
    p.finish_node();
}

/// Parses a comma-separated bare-argument run; only continues across a `,`.
fn parse_bare_args(p: &mut Parser) {
    p.start_node(ARG_LIST);
    parse_arg(p);
    while p.at_text(",") {
        p.bump();
        if at_arg_terminator(p) {
            break;
        }
        parse_arg(p);
    }
    p.finish_node();
}

/// Parses one argument; a trailing `:` promotes the parsed value to a `key: value` named arg.
fn parse_arg(p: &mut Parser) {
    let cp = p.checkpoint();
    parse_value(p);
    if p.at_text(":") {
        p.bump();
        if !at_arg_terminator(p) && !p.at_text(",") {
            parse_value(p);
        }
        p.start_node_at(cp, NAMED_ARG);
        p.finish_node();
    }
}

/// Returns `true` if the current token can begin a bare command argument.
fn starts_bare_arg(p: &Parser) -> bool {
    p.at(SyntaxKind::STRING) || p.at(SyntaxKind::NUMBER) || at_non_keyword_ident(p)
}

/// Returns `true` if the current token can begin a value (atom).
fn at_value_start(p: &Parser) -> bool {
    p.at(SyntaxKind::STRING)
        || p.at(SyntaxKind::NUMBER)
        || p.at_text("[")
        || p.at_text("(")
        || at_non_keyword_ident(p)
}

/// Returns `true` for an identifier that does not begin a new statement (not a keyword).
fn at_non_keyword_ident(p: &Parser) -> bool {
    p.at(SyntaxKind::IDENT) && p.current_text().is_none_or(|t| !is_statement_keyword(t))
}

/// Returns `true` at a token that ends a value or argument run.
fn at_value_terminator(p: &Parser) -> bool {
    p.at_eof() || p.at_text("}") || p.at_text(")") || p.at_text("]") || p.at_text(";")
}

/// Returns `true` at a token that ends an argument run (adds `{` and `,` to the value set).
fn at_arg_terminator(p: &Parser) -> bool {
    at_value_terminator(p) || p.at_text("{")
}

/// Returns `true` if the current token is a single binary-operator punctuation byte.
fn cur_is_op(p: &Parser) -> bool {
    p.current_text()
        .is_some_and(|t| t.len() == 1 && is_op_byte(t.as_bytes()[0]))
}

/// Coalesces a run of adjacent operator bytes (so `==`, `=~`, `->`, `&&` are one operator).
fn bump_op_run(p: &mut Parser) {
    while cur_is_op(p) {
        p.bump();
    }
}

/// Operator punctuation bytes — excludes `.` (postfix), `:` `,` (separators), and delimiters.
const fn is_op_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'=' | b'<' | b'>' | b'!' | b'&' | b'|' | b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'~' | b'?'
    )
}

/// Identifiers that begin a new statement and so must NOT be slurped as an operand/argument.
fn is_statement_keyword(text: &str) -> bool {
    matches!(
        text,
        "if" | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "try"
            | "catch"
            | "finally"
            | "return"
            | "throw"
            | "assert"
            | "synchronized"
    )
}
