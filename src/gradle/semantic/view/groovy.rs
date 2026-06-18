//! Lowering Groovy-DSL red-tree nodes into the DSL-agnostic [`super`] statement view.
//!
//! Groovy leads a call with bare `IDENT` (`.`-joined) tokens for the head, then an `ARG_LIST`
//! and/or `CLOSURE`; `ARG_LIST` children are `STRING` tokens, `NAMED_ARG` nodes (`key: value`),
//! `PATH` nodes (`libs.junit.core`), bare `IDENT`s (`task hello`), or nested `CALL`s
//! (`project(':core')`). An assignment is `ASSIGNMENT[PATH/IDENT "=" value]`. A Groovy `import`
//! parses as a call headed by `import`, so it is recognized here and re-lowered to an import.
//! Only nucleus nodes are lowered; `OPAQUE`/`ERROR_NODE` children are skipped.

use std::rc::Rc;

use crate::gradle::parser::groovy as g;
use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

use super::{AssignExpr, ArgExpr, CallExpr, Statement, unquote};

/// Lowers the direct nucleus child statements of `node` (root or closure body).
pub(super) fn child_statements(node: &SyntaxNode) -> Vec<Statement> {
    let mut out = Vec::new();
    for child in node.child_nodes() {
        match child.kind() {
            k if k == g::CALL => out.push(lower_call_statement(&child)),
            k if k == g::ASSIGNMENT => out.push(Statement::Assignment(lower_assignment(&child))),
            k if k == g::DECLARATION => out.extend(child_statements(&child)),
            _ => {} // OPAQUE / ERROR_NODE / others: skipped by design.
        }
    }
    out
}

/// Lowers a Groovy `CALL`, re-routing an `import`-headed call to a [`Statement::Import`].
fn lower_call_statement(call: &Rc<SyntaxNode>) -> Statement {
    let expr = lower_call(call);
    if expr.head == "import"
        && let Some(path) = expr.args.iter().find_map(arg_as_path_or_str)
    {
        return Statement::Import {
            path,
            span: expr.span,
        };
    }
    Statement::Call(expr)
}

/// Lowers a Groovy `CALL` node into a normalized [`CallExpr`].
fn lower_call(call: &Rc<SyntaxNode>) -> CallExpr {
    let head = leading_head(call);
    let mut args = Vec::new();
    let mut block = None;

    for child in call.child_nodes() {
        match child.kind() {
            k if k == g::ARG_LIST => args = lower_args(&child),
            k if k == g::CLOSURE => block = Some(Rc::clone(&child)),
            _ => {}
        }
    }

    CallExpr {
        head: head.clone(),
        head_raw: head,
        args,
        block,
        suffixes: Vec::new(),
        span: call.span(),
    }
}

/// Lowers an `ASSIGNMENT` node into a normalized [`AssignExpr`].
fn lower_assignment(node: &Rc<SyntaxNode>) -> AssignExpr {
    let target = node
        .child_nodes()
        .find(|n| n.kind() == g::PATH)
        .map(|n| dotted_idents(&n))
        .or_else(|| leading_ident(node))
        .unwrap_or_default();
    let value = first_string_token(node).map(|s| ArgExpr::Str(unquote(&s)));
    AssignExpr {
        target,
        value,
        span: node.span(),
    }
}

/// Lowers a Groovy `ARG_LIST` body into flat [`ArgExpr`]s.
fn lower_args(arg_list: &SyntaxNode) -> Vec<ArgExpr> {
    let mut args = Vec::new();
    for child in arg_list.children() {
        match child {
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::STRING => {
                args.push(ArgExpr::Str(unquote(token.text())));
            }
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => {
                args.push(ArgExpr::Path(token.text().to_string()));
            }
            SyntaxElement::Node(node) if node.kind() == g::NAMED_ARG => {
                args.push(lower_named_arg(node));
            }
            SyntaxElement::Node(node) if node.kind() == g::PATH => {
                args.push(ArgExpr::Path(dotted_idents(node)));
            }
            SyntaxElement::Node(node) if node.kind() == g::CALL => {
                args.push(ArgExpr::Call(lower_call(&Rc::new((**node).clone()))));
            }
            _ => {}
        }
    }
    args
}

/// Lowers a `NAMED_ARG` node (`name: value`) into [`ArgExpr::Named`].
fn lower_named_arg(node: &SyntaxNode) -> ArgExpr {
    let name = leading_ident(node).unwrap_or_default();
    let value = first_string_token(node)
        .map(|s| ArgExpr::Str(unquote(&s)))
        .or_else(|| {
            node.child_nodes()
                .find(|n| n.kind() == g::PATH)
                .map(|n| ArgExpr::Path(dotted_idents(&n)))
        })
        .unwrap_or(ArgExpr::Str(String::new()));
    ArgExpr::Named {
        name,
        value: Box::new(value),
    }
}

/// Returns the string an arg carries, preferring a string literal then a path.
fn arg_as_path_or_str(arg: &ArgExpr) -> Option<String> {
    match arg {
        ArgExpr::Str(text) => Some(text.clone()),
        ArgExpr::Path(path) => Some(path.clone()),
        _ => None,
    }
}

/// Joins a call's leading `IDENT` tokens (before the first node) with `.` as its head.
fn leading_head(call: &SyntaxNode) -> String {
    let mut parts = Vec::new();
    for child in call.children() {
        match child {
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => {
                parts.push(token.text().to_string());
            }
            SyntaxElement::Token(token) if token.kind().is_trivia() || token.text() == "." => {}
            SyntaxElement::Token(_) => break,
            SyntaxElement::Node(_) => break,
        }
    }
    parts.join(".")
}

/// Joins a `PATH` node's `IDENT` tokens with `.` (`libs.junit.core`, `rootProject.name`).
fn dotted_idents(node: &SyntaxNode) -> String {
    node.children()
        .iter()
        .filter_map(|c| match c {
            SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Returns the first non-trivia `IDENT` token text inside `node`.
fn leading_ident(node: &SyntaxNode) -> Option<String> {
    node.children().iter().find_map(|c| match c {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
        _ => None,
    })
}

/// Returns the first `STRING` token text inside `node` (not unquoted).
fn first_string_token(node: &SyntaxNode) -> Option<String> {
    node.children().iter().find_map(|c| match c {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::STRING => Some(t.text().to_string()),
        _ => None,
    })
}
