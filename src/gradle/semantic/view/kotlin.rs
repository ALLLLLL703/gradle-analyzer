//! Lowering Kotlin-DSL red-tree nodes into the DSL-agnostic [`super`] statement view.
//!
//! Kotlin shapes a call as an `ACCESS_PATH` head node followed by `ARG_LIST`/`TYPE_ARGS`/
//! `BLOCK`/`PLUGIN_SUFFIX` siblings, and an assignment as `ASSIGNMENT[ACCESS_PATH "=" value]`.
//! A nested `project(":core")` inside an arg list appears as adjacent `ACCESS_PATH` + `ARG_LIST`
//! siblings, which this module folds back into a [`super::ArgExpr::Call`]. Only nucleus nodes
//! are lowered; `OPAQUE`/`ERROR_NODE` children are skipped.

use std::rc::Rc;

use crate::gradle::parser::kotlin::kinds;
use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

use super::{AssignExpr, ArgExpr, CallExpr, PluginSuffix, Statement, unquote};

/// Lowers the direct nucleus child statements of `node` (root or block body).
pub(super) fn child_statements(node: &SyntaxNode) -> Vec<Statement> {
    let mut out = Vec::new();
    for child in node.child_nodes() {
        match child.kind() {
            k if k == kinds::CALL => out.push(Statement::Call(lower_call(&child))),
            k if k == kinds::ASSIGNMENT => out.push(Statement::Assignment(lower_assignment(&child))),
            k if k == kinds::IMPORT => out.push(Statement::Import {
                path: import_path(&child),
                span: child.span(),
            }),
            _ => {} // OPAQUE / ERROR_NODE / PACKAGE / others: skipped by design.
        }
    }
    out
}

/// Lowers a Kotlin `CALL` node into a normalized [`CallExpr`].
fn lower_call(call: &Rc<SyntaxNode>) -> CallExpr {
    let mut head = String::new();
    let mut head_raw = String::new();
    let mut args = Vec::new();
    let mut block = None;
    let mut suffixes = Vec::new();

    for child in call.child_nodes() {
        match child.kind() {
            k if k == kinds::ACCESS_PATH && head.is_empty() => {
                head = dotted_idents(&child);
                head_raw = clean_head_raw(&child.text());
            }
            k if k == kinds::ARG_LIST => args = lower_args(&child),
            k if k == kinds::BLOCK => block = Some(Rc::clone(&child)),
            k if k == kinds::PLUGIN_SUFFIX => suffixes.push(lower_suffix(&child)),
            _ => {}
        }
    }

    CallExpr {
        head,
        head_raw,
        args,
        block,
        suffixes,
        span: call.span(),
    }
}

/// Lowers an `ASSIGNMENT` node into a normalized [`AssignExpr`].
fn lower_assignment(node: &Rc<SyntaxNode>) -> AssignExpr {
    let target = node
        .child_nodes()
        .find(|n| n.kind() == kinds::ACCESS_PATH)
        .map(|n| dotted_idents(&n))
        .unwrap_or_default();
    let value = first_string_token(node).map(|s| ArgExpr::Str(unquote(&s)));
    AssignExpr {
        target,
        value,
        span: node.span(),
    }
}

/// Lowers a Kotlin `ARG_LIST` body into flat [`ArgExpr`]s, folding `path(args)` into a call.
fn lower_args(arg_list: &SyntaxNode) -> Vec<ArgExpr> {
    let items: Vec<SyntaxElement> = arg_list
        .children()
        .iter()
        .filter(|c| is_meaningful(c))
        .cloned()
        .collect();

    let mut args = Vec::new();
    let mut index = 0;
    while index < items.len() {
        match &items[index] {
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::STRING => {
                args.push(ArgExpr::Str(unquote(token.text())));
                index += 1;
            }
            SyntaxElement::Node(node) if node.kind() == kinds::NAMED_ARG => {
                args.push(lower_named_arg(node));
                index += 1;
            }
            SyntaxElement::Node(node) if node.kind() == kinds::ACCESS_PATH => {
                // A following ARG_LIST means this path is the head of a nested call.
                let path = dotted_idents(node);
                if let Some(SyntaxElement::Node(next)) = items.get(index + 1)
                    && next.kind() == kinds::ARG_LIST
                {
                    args.push(ArgExpr::Call(CallExpr {
                        head: path,
                        head_raw: clean_head_raw(&node.text()),
                        args: lower_args(next),
                        block: None,
                        suffixes: Vec::new(),
                        span: node.span().merge(next.span()),
                    }));
                    index += 2;
                    continue;
                }
                args.push(ArgExpr::Path(path));
                index += 1;
            }
            _ => index += 1,
        }
    }
    args
}

/// Lowers a `NAMED_ARG` node (`name = value`) into [`ArgExpr::Named`].
fn lower_named_arg(node: &SyntaxNode) -> ArgExpr {
    let name = first_ident(node).unwrap_or_default();
    let value = first_string_token(node)
        .map(|s| ArgExpr::Str(unquote(&s)))
        .unwrap_or(ArgExpr::Str(String::new()));
    ArgExpr::Named {
        name,
        value: Box::new(value),
    }
}

/// Lowers a `PLUGIN_SUFFIX` node (`version "x"` / `apply false`) into a [`PluginSuffix`].
fn lower_suffix(node: &SyntaxNode) -> PluginSuffix {
    let keyword = first_ident(node).unwrap_or_default();
    let value = node.children().iter().skip(1).find_map(|c| match c {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::STRING => Some(unquote(t.text())),
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT && first_ident_is_not(t.text(), &keyword) => {
            Some(t.text().to_string())
        }
        _ => None,
    });
    PluginSuffix { keyword, value }
}

/// Returns `true` if `text` is an ident other than the suffix keyword (`false`/`true` value).
fn first_ident_is_not(text: &str, keyword: &str) -> bool {
    text != keyword
}

/// Reads an `import` header's dotted path (everything after the `import` keyword token).
fn import_path(node: &SyntaxNode) -> String {
    let mut parts = Vec::new();
    let mut seen_keyword = false;
    for child in node.children() {
        if let SyntaxElement::Token(token) = child {
            if token.kind().is_trivia() {
                continue;
            }
            if !seen_keyword {
                seen_keyword = true; // skip the leading `import`
                continue;
            }
            if token.kind() == SyntaxKind::IDENT {
                parts.push(token.text().to_string());
            }
        }
    }
    parts.join(".")
}

/// Joins an `ACCESS_PATH` node's `IDENT` tokens with `.` (`tasks.register`, `libs.guava`).
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
fn first_ident(node: &SyntaxNode) -> Option<String> {
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

/// Cleans a head path's raw text for plugin-id fallback: trims and strips backticks.
fn clean_head_raw(text: &str) -> String {
    text.trim().replace('`', "")
}

/// Returns `true` for a child that carries meaning (a node, or a non-trivia value token).
fn is_meaningful(child: &SyntaxElement) -> bool {
    match child {
        SyntaxElement::Node(_) => true,
        SyntaxElement::Token(token) => {
            !token.kind().is_trivia()
                && matches!(token.kind(), SyntaxKind::STRING | SyntaxKind::NUMBER | SyntaxKind::IDENT)
        }
    }
}
