//! DSL-aware extraction of names, coordinates, and task labels from the red tree.
//!
//! The Kotlin and Groovy frontends emit the same numeric [`SyntaxKind`]s but DIFFERENT tree
//! shapes (Kotlin wraps a call name in an `ACCESS_PATH`; Groovy uses a bare leading `IDENT`
//! or a dotted `PATH`). These helpers hide that difference behind a small vocabulary the
//! [`super::builder`] uses: the call's name segments, its first string argument, and the
//! name of a registered task. They are pure functions over [`SyntaxNode`] and never panic.

use std::rc::Rc;

use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

use super::kinds::DslKinds;

/// Returns the dotted name segments leading a call or assignment, e.g. `["tasks",
/// "register"]` or `["plugins"]`.
///
/// Kotlin reads the segments from the leading `ACCESS_PATH` child; Groovy reads a bare
/// leading `IDENT` token or a dotted `PATH` node. An empty vector means no name was
/// recoverable (a malformed head), which callers treat as "unknown".
pub fn call_name_segments(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Vec<String> {
    if kinds.is_kotlin {
        // Kotlin: the first child node is an ACCESS_PATH whose IDENT tokens are the segments.
        if let Some(path) = first_child_node_of(node, kinds.access_path) {
            return ident_segments(&path);
        }
        Vec::new()
    } else {
        // Groovy: a leading PATH node (dotted) or a bare leading IDENT token.
        if let Some(path) = first_child_node_of(node, kinds.path) {
            return ident_segments(&path);
        }
        first_ident_token(node).into_iter().collect()
    }
}

/// Returns the first string-literal argument's inner text (quotes stripped), if any.
///
/// Searches the call's argument-list child for the first `STRING` token. Returns `None` for
/// calls with no string argument (e.g. `mavenCentral()`).
pub fn first_string_arg(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<String> {
    let arg_list = first_child_node_of(node, kinds.arg_list)?;
    first_string_in(&arg_list)
}

/// Returns the first string literal anywhere within `node` (quotes stripped).
///
/// Unlike [`first_string_arg`], this does not require an argument-list child, so it recovers
/// an assignment's value string, which the frontends emit as a direct child of the
/// assignment node (`group = "x"`).
pub fn first_string_value(node: &Rc<SyntaxNode>) -> Option<String> {
    first_string_in(node)
}

/// Returns the first dotted accessor argument (`libs.guava`) as a joined string, if any.
///
/// Used to label catalog-style dependencies whose coordinate is an accessor path rather than
/// a string literal.
pub fn first_accessor_arg(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<String> {
    let arg_list = first_child_node_of(node, kinds.arg_list)?;
    let accessor_kind = if kinds.is_kotlin { kinds.access_path } else { kinds.path };
    let accessor = first_child_node_of(&arg_list, accessor_kind)?;
    let segments = ident_segments(&accessor);
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("."))
    }
}

/// Returns a nested `project(":path")` reference argument, if the call's argument is itself a
/// `project(...)` call.
pub fn first_project_ref_arg(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<String> {
    let arg_list = first_child_node_of(node, kinds.arg_list)?;
    let inner_call = arg_list
        .child_nodes()
        .find(|child| child.kind() == kinds.call)?;
    let name = call_name_segments(&inner_call, kinds);
    if name.last().map(String::as_str) == Some("project") {
        first_string_arg(&inner_call, kinds).map(|p| format!("project({p})"))
    } else {
        None
    }
}

/// Returns the task name for a Groovy `task foo {}` declaration.
///
/// Groovy puts the task name inside the bare argument list as either a `STRING` (`task
/// 'foo'`) or an `IDENT` (`task foo`). Returns `None` when no name token is present.
pub fn groovy_task_name(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> Option<String> {
    let arg_list = first_child_node_of(node, kinds.arg_list)?;
    if let Some(s) = first_string_in(&arg_list) {
        return Some(s);
    }
    arg_list.children().iter().find_map(|child| match child {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
        _ => None,
    })
}

/// Returns the named-argument value for `apply plugin: 'x'` style calls (Groovy).
pub fn named_arg_value(node: &Rc<SyntaxNode>, key: &str, kinds: &DslKinds) -> Option<String> {
    let arg_list = first_child_node_of(node, kinds.arg_list)?;
    for named in arg_list.child_nodes().filter(|c| c.kind() == kinds.named_arg) {
        let mut idents = named.children().iter().filter_map(|c| match c {
            SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
            _ => None,
        });
        if idents.next().as_deref() == Some(key)
            && let Some(value) = first_string_in(&named)
        {
            return Some(value);
        }
    }
    None
}

/// Returns the span of a node's leading name token, falling back to the node's own span.
///
/// Used to compute the LSP `selection_range`: the name identifier when recoverable, else the
/// whole construct so the selection is always a valid sub-range.
pub fn name_selection_span(node: &Rc<SyntaxNode>, kinds: &DslKinds) -> crate::gradle::syntax::TextSpan {
    if kinds.is_kotlin {
        if let Some(path) = first_child_node_of(node, kinds.access_path)
            && let Some(span) = first_ident_span(&path)
        {
            return span;
        }
    } else if let Some(span) = first_ident_span(node) {
        return span;
    }
    node.span()
}

/// Returns the first child NODE of `parent` whose kind matches `kind`.
fn first_child_node_of(parent: &Rc<SyntaxNode>, kind: SyntaxKind) -> Option<Rc<SyntaxNode>> {
    parent.child_nodes().find(|child| child.kind() == kind)
}

/// Collects the `IDENT` token texts directly under `node` (an access path / dotted path).
fn ident_segments(node: &Rc<SyntaxNode>) -> Vec<String> {
    node.children()
        .iter()
        .filter_map(|child| match child {
            SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Returns the first direct `IDENT` token text of `node`.
fn first_ident_token(node: &Rc<SyntaxNode>) -> Option<String> {
    node.children().iter().find_map(|child| match child {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.text().to_string()),
        _ => None,
    })
}

/// Returns the byte span of the first direct `IDENT` token of `node`.
fn first_ident_span(node: &Rc<SyntaxNode>) -> Option<crate::gradle::syntax::TextSpan> {
    node.children().iter().find_map(|child| match child {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::IDENT => Some(t.span()),
        _ => None,
    })
}

/// Finds the first `STRING` token anywhere directly within `node` and strips its quotes.
fn first_string_in(node: &Rc<SyntaxNode>) -> Option<String> {
    node.children().iter().find_map(|child| match child {
        SyntaxElement::Token(t) if t.kind() == SyntaxKind::STRING => Some(strip_quotes(t.text())),
        SyntaxElement::Node(n) => first_string_in(n),
        _ => None,
    })
}

/// Strips a single matching leading/trailing quote (single or double) from a string literal.
///
/// The lexer includes the surrounding quotes in `STRING` tokens; an unterminated literal may
/// have only the opening quote, so trailing removal is conditional on a real closing quote.
fn strip_quotes(literal: &str) -> String {
    let bytes = literal.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && last == first {
            return literal[1..literal.len() - 1].to_string();
        }
    }
    // Unterminated or unquoted: drop a lone leading quote so the label stays readable.
    literal
        .strip_prefix('"')
        .or_else(|| literal.strip_prefix('\''))
        .unwrap_or(literal)
        .to_string()
}
