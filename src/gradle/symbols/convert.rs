//! The protocol boundary: [`SymbolNode`] tree → `tower_lsp` `DocumentSymbol` tree.
//!
//! This is the ONLY file in the outline feature that imports LSP types, keeping the builder
//! and its tests protocol-free. It maps each [`OutlineKind`] to a `SymbolKind` and converts
//! byte spans to LSP `Range`s via the reusable [`LineIndex`]. The `selection_range` is
//! guaranteed to sit within `range` (the builder anchors it to the name token, a sub-span of
//! the construct), satisfying the LSP requirement.

use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

use crate::gradle::syntax::TextSpan;
use crate::util::line_index::{LineCol, LineIndex};

use super::node::{OutlineKind, SymbolNode};

/// Converts an outline `nodes` tree into LSP `DocumentSymbol`s using `index` for ranges.
pub fn to_document_symbols(nodes: &[SymbolNode], index: &LineIndex) -> Vec<DocumentSymbol> {
    nodes.iter().map(|node| convert_node(node, index)).collect()
}

/// Converts a single outline node (and its children) to a `DocumentSymbol`.
fn convert_node(node: &SymbolNode, index: &LineIndex) -> DocumentSymbol {
    let children = if node.children.is_empty() {
        None
    } else {
        Some(to_document_symbols(&node.children, index))
    };
    #[allow(deprecated)] // `deprecated` is a required struct field in lsp-types.
    DocumentSymbol {
        name: node.name.clone(),
        detail: node.detail.clone(),
        kind: lsp_kind(node.kind),
        tags: None,
        deprecated: None,
        range: to_range(node.span, index),
        selection_range: to_range(node.selection, index),
        children,
    }
}

/// Maps an [`OutlineKind`] to the closest LSP `SymbolKind`.
fn lsp_kind(kind: OutlineKind) -> SymbolKind {
    match kind {
        OutlineKind::Section => SymbolKind::NAMESPACE,
        OutlineKind::Project => SymbolKind::MODULE,
        OutlineKind::Plugin => SymbolKind::MODULE,
        OutlineKind::Repository => SymbolKind::INTERFACE,
        OutlineKind::Dependency => SymbolKind::FIELD,
        OutlineKind::Task => SymbolKind::FUNCTION,
        OutlineKind::Property => SymbolKind::PROPERTY,
        OutlineKind::Block => SymbolKind::NAMESPACE,
    }
}

/// Converts a byte span to an LSP `Range` via the line index.
fn to_range(span: TextSpan, index: &LineIndex) -> Range {
    Range {
        start: to_position(index.line_col(span.start)),
        end: to_position(index.line_col(span.end())),
    }
}

/// Converts a [`LineCol`] to an LSP `Position`.
fn to_position(line_col: LineCol) -> Position {
    Position {
        line: line_col.line,
        character: line_col.character,
    }
}
