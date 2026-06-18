//! Outline for version-catalog TOML files (`libs.versions.toml`).
//!
//! Version catalogs are TOML, not a Gradle DSL, so the syntax frontends never parse them and
//! the semantic catalog facts carry ZERO spans (their keys come from a TOML reader, not the
//! source byte range). To still offer a useful, range-correct outline this pass runs a tiny
//! line scanner: each `[table]` header becomes a [`OutlineKind::Block`] section and each
//! top-level `key =` becomes a [`OutlineKind::Property`] nested under the current table. It
//! is intentionally minimal — no value parsing, no nested-table semantics — but every range
//! is a real byte span the editor can navigate to.

use crate::gradle::syntax::TextSpan;

use super::node::{OutlineKind, SymbolNode};

/// Builds a sectioned outline for a version-catalog TOML `source`.
pub fn build(source: &str) -> Vec<SymbolNode> {
    let mut sections: Vec<SymbolNode> = Vec::new();
    let mut loose: Vec<SymbolNode> = Vec::new();
    let mut line_start = 0usize;

    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        let content_len = line.trim_end_matches(['\n', '\r']).len();
        let span = TextSpan::new(line_start, content_len);

        if let Some(table) = table_header(trimmed) {
            sections.push(SymbolNode::container(
                table,
                OutlineKind::Block,
                span,
                span,
                Vec::new(),
            ));
        } else if let Some(key) = top_level_key(trimmed) {
            let property = SymbolNode::leaf(key, None, OutlineKind::Property, span, span);
            match sections.last_mut() {
                Some(section) => section.children.push(property),
                None => loose.push(property),
            }
        }
        line_start += line.len();
    }

    loose.extend(sections);
    loose
}

/// Returns the table name for a `[name]` header line, if the line is one.
fn table_header(trimmed: &str) -> Option<String> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let name = inner.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Returns the key for a `key = value` line, ignoring comments and blank lines.
fn top_level_key(trimmed: &str) -> Option<String> {
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}
