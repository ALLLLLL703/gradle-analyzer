//! Kotlin-DSL block bodies and the bounded opaque fallback.
//!
//! `parse_block` parses a `{ ... }` body as a sequence of statements, so nested nucleus
//! blocks parse and unknown inner statements degrade locally. `bump_opaque_balanced` is the
//! tolerance mechanism: it collapses an unrecognized run into ONE substrate
//! [`SyntaxKind::OPAQUE`] node (no error — opaque is tolerated, not malformed), tracking
//! brace/paren/bracket depth so it stops at the right boundary and a following nucleus block
//! still parses.

use crate::gradle::syntax::{Parser, SyntaxErrorKind, SyntaxKind};

use super::kinds;
use super::statement::parse_statement;

/// Parses a brace-delimited block / trailing lambda as a run of inner statements.
///
/// Precondition: the parser is positioned at `{`. On EOF before the closing `}`, records one
/// [`SyntaxErrorKind::UnclosedBlock`] anchored (by the substrate) to the last consumed token.
pub(super) fn parse_block(p: &mut Parser) {
    p.start_node(kinds::BLOCK);
    p.bump();
    loop {
        if p.at_eof() {
            p.error_eof_anchored(SyntaxErrorKind::UnclosedBlock);
            break;
        }
        if p.at_text("}") {
            p.bump();
            break;
        }
        parse_statement(p);
    }
    p.finish_node();
}

/// Collapses an out-of-nucleus run into ONE bounded `OPAQUE` node, emitting no error.
///
/// Bumps at least one token (guaranteed progress), then continues while inside a delimiter
/// (`(`/`{`/`[` raise depth, their mates lower it). At depth 0 it STOPS before a closing
/// delimiter belonging to an enclosing block and before a known nucleus-starter identifier,
/// so a following `dependencies { }` is parsed as a real call rather than swallowed.
pub fn bump_opaque_balanced(p: &mut Parser) {
    if p.at_eof() {
        return;
    }
    p.start_node(SyntaxKind::OPAQUE);
    let mut depth: usize = open_delta(p).max(0) as usize;
    p.bump();
    while !p.at_eof() {
        if depth == 0 && stops_opaque_run(p) {
            break;
        }
        if open_delta(p) > 0 {
            depth += 1;
        } else if close_delta(p) < 0 {
            depth = depth.saturating_sub(1);
        }
        p.bump();
    }
    p.finish_node();
}

/// Returns `true` if, at brace depth 0, the current token ends the opaque run.
///
/// The run ends before an enclosing block's closing delimiter (so the enclosing
/// `parse_block` can consume it) or before a known nucleus-starter identifier.
fn stops_opaque_run(p: &Parser) -> bool {
    if p.at_text("}") || p.at_text(")") || p.at_text("]") {
        return true;
    }
    p.at(SyntaxKind::IDENT) && p.current_text().is_some_and(kinds::is_nucleus_starter)
}

/// Returns `+1` if the current token opens a delimiter, else `0`.
fn open_delta(p: &Parser) -> isize {
    if p.at_text("(") || p.at_text("{") || p.at_text("[") {
        1
    } else {
        0
    }
}

/// Returns `-1` if the current token closes a delimiter, else `0`.
fn close_delta(p: &Parser) -> isize {
    if p.at_text(")") || p.at_text("}") || p.at_text("]") {
        -1
    } else {
        0
    }
}

