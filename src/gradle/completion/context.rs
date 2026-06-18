//! Layer 1+2: text-state suppression and AST/line context classification.
//!
//! [`classify`] lowers a cursor `offset` to an optional [`CompletionContext`]. A `None`
//! result is the suppression signal (the cursor sits in a comment, a string literal, or a
//! non-block `OPAQUE`/`ERROR_NODE` region) and short-circuits the engine to an empty result.
//! Otherwise it reports which recognized block encloses the cursor and the position within
//! it (a fresh statement, or a `libs.*` catalog-accessor / `dependsOn(` task-reference site).
//!
//! Block detection REUSES the semantic view layer ([`child_statements`]): it descends the
//! recognized nucleus statements, follows a call's block body when it contains the cursor,
//! and reads the innermost call head — the same head-extraction the extractors trust, so the
//! two never disagree about what `dependencies {` is.

use std::rc::Rc;

use crate::gradle::semantic::view::{Statement, child_statements};
use crate::gradle::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use crate::gradle::workspace::DslLanguage;

/// Which recognized block encloses the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionBlockContext {
    /// The document root (no enclosing recognized block).
    TopLevel,
    /// Inside a `dependencies { }` block.
    Dependencies,
    /// Inside a `plugins { }` block.
    Plugins,
    /// Inside a `repositories { }` block.
    Repositories,
    /// Inside a `tasks { }` / `tasks.register {}` / `task name {}` block.
    Tasks,
    /// Inside a recognized block we do not special-case (e.g. `buildscript {`).
    Other,
}

/// Where, within its block, the cursor sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionPosition {
    /// A fresh statement position (start of a line / after whitespace).
    Statement,
    /// Immediately after a `libs.` dotted accessor; carries the typed prefix.
    CatalogAccessor {
        /// The dotted prefix typed so far (e.g. `libs.`, `libs.bun`).
        typed: String,
    },
    /// Inside a `dependsOn(` task-reference site (text-backed fallback while mid-type).
    TaskReference,
}

/// The classified completion context: which block, and the position within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    /// The enclosing recognized block.
    pub block: CompletionBlockContext,
    /// The position within the block.
    pub position: CompletionPosition,
}

/// Classifies the cursor at `offset`, returning `None` to SUPPRESS completion.
///
/// Suppression order is deliberate: a comment/string token suppresses unconditionally; a
/// `libs.` accessor site is recognized even inside an otherwise-opaque argument region (so
/// `implementation(libs.|)` still completes); only then does a non-block opaque/error region
/// suppress; otherwise the enclosing block + statement position is reported. Never panics on
/// malformed input — it walks the tolerant tree and a text prefix, both total functions.
pub fn classify(
    root: &Rc<SyntaxNode>,
    text: &str,
    offset: usize,
    lang: DslLanguage,
) -> Option<CompletionContext> {
    let offset = offset.min(text.len());

    // 1. Text-state suppression: never complete inside a comment or string literal.
    if let Some(token) = token_covering(root, offset)
        && is_suppressing_token(token.kind())
    {
        return None;
    }

    let block = innermost_block(root, offset, lang)
        .map(|head| block_context(&head))
        .unwrap_or(CompletionBlockContext::TopLevel);

    // 2. `libs.` accessor site wins even inside a partially-opaque argument region.
    if let Some(typed) = catalog_accessor_prefix(text, offset) {
        return Some(CompletionContext {
            block,
            position: CompletionPosition::CatalogAccessor { typed },
        });
    }

    // 3. A non-block opaque/error region the engine does not understand: suppress.
    let deepest = deepest_node(root, offset);
    if matches!(deepest.kind(), SyntaxKind::OPAQUE | SyntaxKind::ERROR_NODE) {
        return None;
    }

    // 4. A `dependsOn(` task-reference site (text-backed fallback).
    let position = if at_task_reference_site(text, offset) {
        CompletionPosition::TaskReference
    } else {
        CompletionPosition::Statement
    };

    Some(CompletionContext { block, position })
}

/// Returns `true` for token kinds that suppress completion when the cursor is inside them.
fn is_suppressing_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::STRING
    )
}

/// Maps an innermost call head to its [`CompletionBlockContext`].
fn block_context(head: &str) -> CompletionBlockContext {
    if head.starts_with("dependencies") {
        CompletionBlockContext::Dependencies
    } else if head.starts_with("plugins") || head == "pluginManagement" {
        CompletionBlockContext::Plugins
    } else if head.starts_with("repositories") {
        CompletionBlockContext::Repositories
    } else if head.starts_with("tasks") || head == "task" {
        CompletionBlockContext::Tasks
    } else {
        CompletionBlockContext::Other
    }
}

/// Finds the head of the innermost recognized block whose body contains `offset`.
///
/// Descends recognized nucleus statements (via the semantic view), following any call whose
/// block body strictly contains the cursor; the deepest such head wins. Returns `None` at
/// the document root (no enclosing recognized block).
fn innermost_block(node: &SyntaxNode, offset: usize, lang: DslLanguage) -> Option<String> {
    for statement in child_statements(node, lang) {
        let Statement::Call(call) = statement else {
            continue;
        };
        let Some(block) = call.block.as_ref() else {
            continue;
        };
        let span = block.span();
        if span.start < offset && offset < span.end() {
            return Some(innermost_block(block, offset, lang).unwrap_or(call.head));
        }
    }
    None
}

/// Returns the deepest node whose span covers `offset` (start <= offset < end).
fn deepest_node(node: &Rc<SyntaxNode>, offset: usize) -> Rc<SyntaxNode> {
    for child in node.child_nodes() {
        let span = child.span();
        if span.start <= offset && offset < span.end() {
            return deepest_node(&child, offset);
        }
    }
    Rc::clone(node)
}

/// Returns the leaf token whose span covers `offset` (start <= offset < end), if any.
fn token_covering(node: &Rc<SyntaxNode>, offset: usize) -> Option<Rc<SyntaxToken>> {
    for child in node.children() {
        match child {
            SyntaxElement::Node(inner) => {
                let span = inner.span();
                if span.start <= offset && offset < span.end() {
                    return token_covering(inner, offset);
                }
            }
            SyntaxElement::Token(token) => {
                let span = token.span();
                if span.start <= offset && offset < span.end() {
                    return Some(Rc::clone(token));
                }
            }
        }
    }
    None
}

/// Extracts the typed `libs.` accessor prefix ending at `offset`, if the cursor is on one.
///
/// Walks back over `[A-Za-z0-9_.]` to the maximal trailing run; returns it only when the run
/// starts at the `libs` catalog root and contains a `.` (so a bare `libs` word is not yet an
/// accessor site). The maximal-run rule enforces a word boundary, so `mylibs.x` is excluded.
fn catalog_accessor_prefix(text: &str, offset: usize) -> Option<String> {
    let before = &text[..offset];
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_accessor_char(*c))
        .last()
        .map(|(idx, _)| idx)?;
    let run = &before[start..];
    if (run == "libs" || run.starts_with("libs.")) && run.contains('.') {
        return Some(run.to_string());
    }
    None
}

/// Returns `true` for a character that is part of a dotted accessor path.
fn is_accessor_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Returns `true` if the cursor sits inside an unclosed `dependsOn(` call on the line.
///
/// Text-backed fallback for an incomplete task reference: looks at the current line up to the
/// cursor for a `dependsOn(` with no closing `)` after it, so a mid-type `dependsOn("` still
/// offers task names even before the call parses.
fn at_task_reference_site(text: &str, offset: usize) -> bool {
    let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_prefix = &text[line_start..offset];
    match line_prefix.rfind("dependsOn") {
        Some(idx) => {
            let after = &line_prefix[idx + "dependsOn".len()..];
            after.trim_start().starts_with('(') && !after.contains(')')
        }
        None => false,
    }
}
