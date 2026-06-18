//! Block-keyword hover: a known block call head at an offset → its purpose hover.
//!
//! Facts cover a block's CONTENTS, not the block keyword itself, so hovering the
//! `dependencies`/`plugins`/`repositories`/`tasks` head needs a tiny tolerant scan. This
//! walks call nodes (DSL-aware via the frontends' `CALL` kind), and when a call's head IDENT
//! token both names a known block and contains the offset, renders that block's purpose.

use crate::gradle::parser::{groovy as gv, kotlin::kinds as kt};
use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use crate::gradle::workspace::DslLanguage;
use crate::i18n::MessageKey;

use super::HoverModel;

/// Returns the block-purpose hover when a known block keyword head is at `offset`.
pub(super) fn hover_block_keyword(
    root: &SyntaxNode,
    language: DslLanguage,
    offset: usize,
) -> Option<HoverModel> {
    let call_kind = match language {
        DslLanguage::Kotlin => kt::CALL,
        DslLanguage::Groovy => gv::CALL,
    };
    find_block_head(root, call_kind, offset)
}

/// Recursively searches call nodes for a block-keyword head token covering `offset`.
fn find_block_head(node: &SyntaxNode, call_kind: SyntaxKind, offset: usize) -> Option<HoverModel> {
    for child in node.child_nodes() {
        if child.kind() == call_kind
            && let Some(model) = block_head_of(&child, offset)
        {
            return Some(model);
        }
        if let Some(found) = find_block_head(&child, call_kind, offset) {
            return Some(found);
        }
    }
    None
}

/// Returns the hover if this call's first head IDENT is a known block keyword over `offset`.
///
/// The head is a bare IDENT token (Groovy) or the leading IDENT inside an `ACCESS_PATH`
/// node (Kotlin), so the first non-trivia child is inspected token-or-node-first.
fn block_head_of(call: &SyntaxNode, offset: usize) -> Option<HoverModel> {
    let head = call.children().iter().find(|child| !is_trivia(child))?;
    let token_span = head_ident_span(head)?;
    if !token_span.contains(offset) {
        return None;
    }
    let keyword = call.text();
    let key = block_message_key(first_word(&keyword))?;
    Some(HoverModel::new(key, Vec::new(), token_span))
}

/// Returns the span of the leading IDENT of a call head (token directly, or inside a node).
fn head_ident_span(element: &SyntaxElement) -> Option<crate::gradle::syntax::TextSpan> {
    match element {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => Some(token.span()),
        SyntaxElement::Node(node) => leading_ident(node),
        _ => None,
    }
}

/// Returns the span of the first descendant IDENT token in `node`.
fn leading_ident(node: &SyntaxNode) -> Option<crate::gradle::syntax::TextSpan> {
    node.children().iter().find_map(|child| match child {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => Some(token.span()),
        SyntaxElement::Node(inner) => leading_ident(inner),
        _ => None,
    })
}

/// Returns the first whitespace-free word of `text` (the call head keyword).
fn first_word(text: &str) -> &str {
    text.trim_start()
        .split(|c: char| c.is_whitespace() || c == '(' || c == '{' || c == '.')
        .next()
        .unwrap_or("")
}

/// Returns `true` if `element` is a trivia token (whitespace/comment).
fn is_trivia(element: &SyntaxElement) -> bool {
    matches!(element, SyntaxElement::Token(token) if token.kind().is_trivia())
}

/// Maps a block keyword to its purpose message key, if it names a known block.
fn block_message_key(keyword: &str) -> Option<MessageKey> {
    match keyword {
        "plugins" => Some(MessageKey::HoverBlockPlugins),
        "dependencies" => Some(MessageKey::HoverBlockDependencies),
        "repositories" => Some(MessageKey::HoverBlockRepositories),
        "tasks" => Some(MessageKey::HoverBlockTasks),
        _ => None,
    }
}
