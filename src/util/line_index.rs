//! Byte-offset → line/character conversion shared by span-emitting features.
//!
//! LSP positions are `(line, character)` pairs, but the syntax substrate speaks byte
//! offsets ([`crate::gradle::syntax::TextSpan`]). [`LineIndex`] bridges the two: it
//! precomputes the byte offset of every line start once, then answers `byte -> LineCol`
//! in `O(log lines)` via binary search. This lives in `util/` because it is purely
//! domain-agnostic text geometry — any feature converting spans to ranges reuses it.
//!
//! Characters are counted in **UTF-16 code units**, the LSP default position encoding, so
//! a build script containing multi-byte text still produces protocol-correct columns. The
//! type is LSP-type-free: it returns a plain [`LineCol`] that the protocol boundary maps to
//! a `tower_lsp` `Position`.

use std::sync::Arc;

/// A zero-based line/character position in UTF-16 code units.
///
/// Mirrors the shape of an LSP `Position` without depending on the protocol crate, so the
/// helper stays reusable outside the LSP layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based character offset within the line, counted in UTF-16 code units.
    pub character: u32,
}

/// Maps byte offsets into a source string to zero-based line/character positions.
///
/// Construct once per document snapshot, then convert as many spans as needed. Offsets past
/// the end of the source clamp to the final position rather than panicking, keeping callers
/// infallible even if a span outlives a concurrent edit.
///
/// # Example
///
/// ```
/// use gradle_analyzer::util::line_index::LineIndex;
///
/// let index = LineIndex::new("plugins {\n    id(\"java\")\n}\n");
/// // Start of the document.
/// let start = index.line_col(0);
/// assert_eq!((start.line, start.character), (0, 0));
/// // The `id` on the second line (byte 14 is the 'i').
/// let id = index.line_col(14);
/// assert_eq!((id.line, id.character), (1, 4));
/// ```
#[derive(Debug, Clone)]
pub struct LineIndex {
    source: Arc<str>,
    /// Byte offset of the first character of each line (line 0 starts at 0).
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Builds an index over `source`, scanning once for line starts.
    pub fn new(source: impl Into<Arc<str>>) -> LineIndex {
        let source = source.into();
        let mut line_starts = vec![0usize];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        LineIndex { source, line_starts }
    }

    /// Converts a byte `offset` into a zero-based [`LineCol`].
    ///
    /// An offset past the source end clamps to the source length, so the result is always a
    /// valid position. The character component counts UTF-16 code units from the line start.
    pub fn line_col(&self, offset: usize) -> LineCol {
        let clamped = offset.min(self.source.len());
        // The line is the last line-start that is <= clamped.
        let line = match self.line_starts.binary_search(&clamped) {
            Ok(exact) => exact,
            Err(next) => next - 1,
        };
        let line_start = self.line_starts[line];
        let character = utf16_len(&self.source[line_start..clamped]);
        LineCol {
            line: line as u32,
            character: character as u32,
        }
    }

    /// Returns the number of lines (always at least 1).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

/// Counts the UTF-16 code units in `text`.
fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_offsets_are_byte_columns_for_ascii() {
        let index = LineIndex::new("abcdef");
        assert_eq!(index.line_col(0), LineCol { line: 0, character: 0 });
        assert_eq!(index.line_col(3), LineCol { line: 0, character: 3 });
        assert_eq!(index.line_count(), 1);
    }

    #[test]
    fn newline_advances_line_and_resets_character() {
        let source = "ab\ncde\nf";
        let index = LineIndex::new(source);
        // 'c' is byte 3, first char of line 1.
        assert_eq!(index.line_col(3), LineCol { line: 1, character: 0 });
        // 'e' is byte 5 => line 1, character 2.
        assert_eq!(index.line_col(5), LineCol { line: 1, character: 2 });
        // 'f' is byte 7 => line 2, character 0.
        assert_eq!(index.line_col(7), LineCol { line: 2, character: 0 });
        assert_eq!(index.line_count(), 3);
    }

    #[test]
    fn offset_at_newline_belongs_to_the_ending_line() {
        let index = LineIndex::new("ab\ncd");
        // Byte 2 is the '\n' itself: still line 0, character 2.
        assert_eq!(index.line_col(2), LineCol { line: 0, character: 2 });
    }

    #[test]
    fn past_end_clamps_to_final_position() {
        let source = "ab\ncd";
        let index = LineIndex::new(source);
        let end = index.line_col(999);
        assert_eq!(end, LineCol { line: 1, character: 2 });
    }

    #[test]
    fn character_counts_utf16_code_units_not_bytes() {
        // "é" is 2 UTF-8 bytes but 1 UTF-16 unit; "𝟙" is 4 UTF-8 bytes but 2 UTF-16 units.
        let source = "é𝟙x";
        let index = LineIndex::new(source);
        // After "é" (2 bytes) -> character 1.
        assert_eq!(index.line_col(2), LineCol { line: 0, character: 1 });
        // After "é𝟙" (2 + 4 = 6 bytes) -> 1 + 2 = 3 UTF-16 units.
        assert_eq!(index.line_col(6), LineCol { line: 0, character: 3 });
    }
}
