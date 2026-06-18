//! The server boundary: LSP `Position`/`Location` ↔ the LSP-type-free navigation core.
//!
//! The core ([`super::goto_definition`]/[`super::find_references`]) speaks byte offsets and
//! [`NavTarget`]s. This module is the ONLY place that touches `tower_lsp` types for
//! navigation: it converts a request `Position` to a byte offset, runs the core over a
//! single-document analysis of the snapshot, and converts each resulting [`NavTarget`] back
//! to a `Location`. Keeping it here lets `lsp/server.rs` stay a thin delegation.
//!
//! Single-document scope: the snapshot is analyzed on its own, so every target a live
//! request can render is in the requested document. Cross-document targets (a catalog entry
//! or a settings include in another file) are produced by the core and unit-tested directly,
//! but the live boundary only emits same-document `Location`s for now (documented limitation
//! — a workspace-wide store sweep is a later integration concern, out of Task 12 scope).

use std::path::Path;

use tower_lsp::lsp_types::{Location, Position, Range, Url};

use crate::gradle::parser::{parse_groovy, parse_kotlin};
use crate::gradle::semantic::{DocumentId, SemanticInput, analyze_documents};
use crate::gradle::syntax::TextSpan;
use crate::gradle::workspace::TrackedDocument;

use super::{NavDocument, NavTarget, find_references, goto_definition};

/// Whether to answer a definition or a references request over the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavQuery {
    /// `textDocument/definition`.
    Definition,
    /// `textDocument/references`.
    References,
}

/// Answers `query` at `position` over `snapshot`, returning LSP `Location`s.
///
/// Returns an empty vec for a catalog/unknown document, an out-of-range position, or any
/// position with no confident local target. Same-document targets are converted against the
/// snapshot's URI and text; targets in another document are dropped at this boundary.
pub fn navigate(snapshot: &TrackedDocument, position: Position, query: NavQuery) -> Vec<Location> {
    let Some(language) = snapshot.kind().dsl() else {
        return Vec::new();
    };
    let text = snapshot.text();
    let Some(offset) = position_to_offset(text, position) else {
        return Vec::new();
    };

    let root = workspace_root(snapshot.uri());
    let document_id = document_id_for(&root, snapshot.uri());
    let input = SemanticInput::from_tracked(&root, snapshot);
    let graph = analyze_documents(&[input]);

    let parse = match language {
        crate::gradle::workspace::DslLanguage::Kotlin => parse_kotlin(text),
        crate::gradle::workspace::DslLanguage::Groovy => parse_groovy(text),
    };
    let nav_doc = NavDocument::new(document_id.clone(), language);

    let targets = match query {
        NavQuery::Definition => goto_definition(&nav_doc, &parse, &graph, offset),
        NavQuery::References => find_references(&nav_doc, &parse, &graph, offset),
    };

    targets
        .into_iter()
        .filter_map(|target| target_to_location(snapshot.uri(), &document_id, text, &target))
        .collect()
}

/// Converts a same-document [`NavTarget`] to a `Location`, dropping cross-document targets.
fn target_to_location(
    uri: &Url,
    current: &DocumentId,
    text: &str,
    target: &NavTarget,
) -> Option<Location> {
    let NavTarget::Local { document, span } = target;
    if document != current {
        // Cross-document target: not renderable from a single-doc snapshot (see module doc).
        return None;
    }
    Some(Location {
        uri: uri.clone(),
        range: span_to_range(text, *span),
    })
}

/// Derives the workspace root used for the snapshot's id (its parent directory).
///
/// A single-document analysis only needs an id consistent between input and target, so the
/// file's parent directory is a stable, deterministic root.
fn workspace_root(uri: &Url) -> std::path::PathBuf {
    uri.to_file_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
}

/// Builds the document id the navigation core will tag its targets with.
fn document_id_for(root: &Path, uri: &Url) -> DocumentId {
    uri.to_file_path()
        .map(|path| DocumentId::from_relative_path(root, &path))
        .unwrap_or_else(|_| DocumentId::new(uri.as_str()))
}

/// Converts a UTF-16 LSP [`Position`] to a UTF-8 byte offset into `text`.
///
/// Returns `None` if the line is past the end of the text. The character column is treated as
/// a UTF-16 code-unit count (the LSP default) and clamped to the line's end.
pub fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let mut line_start = 0usize;
    let mut line = 0u32;
    while line < position.line {
        let newline = text[line_start..].find('\n')?;
        line_start += newline + 1;
        line += 1;
    }
    let line_text = match text[line_start..].find('\n') {
        Some(end) => &text[line_start..line_start + end],
        None => &text[line_start..],
    };
    Some(line_start + utf16_column_to_byte(line_text, position.character))
}

/// Maps a UTF-16 code-unit column within `line_text` to a byte offset, clamped to its end.
fn utf16_column_to_byte(line_text: &str, character: u32) -> usize {
    let mut utf16 = 0u32;
    for (byte_idx, ch) in line_text.char_indices() {
        if utf16 >= character {
            return byte_idx;
        }
        utf16 += ch.len_utf16() as u32;
    }
    line_text.len()
}

/// Converts a byte [`TextSpan`] to an LSP UTF-16 [`Range`] over `text`.
pub fn span_to_range(text: &str, span: TextSpan) -> Range {
    Range {
        start: offset_to_position(text, span.start),
        end: offset_to_position(text, span.end()),
    }
}

/// Converts a UTF-8 byte `offset` into `text` to a UTF-16 LSP [`Position`].
fn offset_to_position(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (idx, ch) in text.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + ch.len_utf8();
        }
    }
    let character = text
        .get(line_start..offset)
        .unwrap_or("")
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();
    Position { line, character }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_offset_round_trips_on_multiline_text() {
        let text = "task build {}\ntask check {}\n";
        let pos = Position { line: 1, character: 5 };
        let offset = position_to_offset(text, pos).unwrap();
        assert_eq!(&text[offset..offset + 5], "check");
        let back = offset_to_position(text, offset);
        assert_eq!(back, pos);
    }

    #[test]
    fn position_past_end_is_none() {
        let text = "one line\n";
        assert!(position_to_offset(text, Position { line: 9, character: 0 }).is_none());
    }

    #[test]
    fn span_to_range_spans_a_token() {
        let text = "task build {}\n";
        let span = TextSpan::new(5, 5); // "build"
        let range = span_to_range(text, span);
        assert_eq!(range.start, Position { line: 0, character: 5 });
        assert_eq!(range.end, Position { line: 0, character: 10 });
    }
}
