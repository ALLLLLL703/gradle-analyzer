//! Closure / configuration-block parsing (`{ ... }`) and balanced-brace recovery.
//!
//! A closure is the uniform body of every Gradle block (`plugins { }`, `repositories { }`,
//! `task foo { }`) — there is no per-keyword special-casing. Its body is just a run of
//! statements parsed by [`super::parse_statement`]. A closure that never closes reports a
//! single [`SyntaxErrorKind::UnclosedBlock`] anchored to the end of the last consumed token.

use crate::gradle::syntax::{Parser, SyntaxErrorKind};

use super::{CLOSURE, parse_statement};

/// Parses a `{ ... }` closure body, recovering an unclosed brace as `UnclosedBlock`.
///
/// The caller guarantees the current token is `{`. Every loop iteration makes progress:
/// it either consumes the closing `}`, or parses one statement (which itself always bumps
/// at least one token), so the body can never spin.
pub(super) fn parse_closure(p: &mut Parser) {
    p.start_node(CLOSURE);
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
