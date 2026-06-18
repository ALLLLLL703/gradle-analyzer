//! Byte-based source spans shared by every syntax facility.
//!
//! A [`TextSpan`] is a half-open byte range `[start, start + len)` into the original
//! UTF-8 source. Spans are the common currency between the lexer, the green/red trees,
//! and the typed error side table, so keeping them byte-accurate is what makes the whole
//! substrate round-trippable and lets diagnostics point at exact source ranges.

/// A half-open byte range into a UTF-8 source string.
///
/// Stored as `start` + `len` (rather than `start..end`) so a span is `Copy`, cheap, and
/// always non-negative in width. All offsets are byte offsets, not char offsets.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::syntax::TextSpan;
///
/// let span = TextSpan::new(2, 3);
/// assert_eq!(span.end(), 5);
/// assert_eq!(span.text("abXYZcd"), "XYZ");
/// assert!(span.contains(3));
/// assert!(!span.contains(5));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextSpan {
    /// Byte offset of the first byte in the span.
    pub start: usize,
    /// Length of the span in bytes.
    pub len: usize,
}

impl TextSpan {
    /// Builds a span from a start offset and a byte length.
    pub const fn new(start: usize, len: usize) -> Self {
        Self { start, len }
    }

    /// Builds a span from a half-open `[start, end)` byte range.
    ///
    /// `end` is clamped to `start` so the width is never negative.
    pub const fn from_range(start: usize, end: usize) -> Self {
        let len = end.saturating_sub(start);
        Self { start, len }
    }

    /// Builds an empty (zero-width) span anchored at `offset`.
    ///
    /// Used by recovery to anchor an error at the END of the last consumed token rather
    /// than at an arbitrary EOF-zero position.
    pub const fn empty_at(offset: usize) -> Self {
        Self { start: offset, len: 0 }
    }

    /// Returns the exclusive end offset (`start + len`).
    pub const fn end(self) -> usize {
        self.start + self.len
    }

    /// Returns `true` if the span covers zero bytes.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Returns `true` if `offset` falls within `[start, end)`.
    pub const fn contains(self, offset: usize) -> bool {
        offset >= self.start && offset < self.end()
    }

    /// Returns the smallest span covering both `self` and `other`.
    ///
    /// The result runs from the lower start to the higher end, so merging is order
    /// independent and absorbs gaps between the two spans.
    pub fn merge(self, other: TextSpan) -> TextSpan {
        let start = self.start.min(other.start);
        let end = self.end().max(other.end());
        TextSpan::from_range(start, end)
    }

    /// Slices `source` by this span, clamping to the source bounds.
    ///
    /// Clamping keeps the call infallible even if a span outlives an edit; a span past the
    /// end yields `""` rather than panicking.
    pub fn text(self, source: &str) -> &str {
        let start = self.start.min(source.len());
        let end = self.end().min(source.len());
        &source[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_and_emptiness_are_byte_accurate() {
        let span = TextSpan::new(4, 3);
        assert_eq!(span.end(), 7);
        assert!(!span.is_empty());
        assert!(TextSpan::empty_at(9).is_empty());
        assert_eq!(TextSpan::empty_at(9).start, 9);
    }

    #[test]
    fn from_range_clamps_negative_width() {
        assert_eq!(TextSpan::from_range(5, 2), TextSpan::new(5, 0));
        assert_eq!(TextSpan::from_range(2, 5), TextSpan::new(2, 3));
    }

    #[test]
    fn contains_is_half_open() {
        let span = TextSpan::new(2, 3);
        assert!(!span.contains(1));
        assert!(span.contains(2));
        assert!(span.contains(4));
        assert!(!span.contains(5));
    }

    #[test]
    fn merge_is_order_independent_and_absorbs_gaps() {
        let a = TextSpan::new(2, 2);
        let b = TextSpan::new(8, 1);
        assert_eq!(a.merge(b), TextSpan::from_range(2, 9));
        assert_eq!(b.merge(a), TextSpan::from_range(2, 9));
    }

    #[test]
    fn text_slices_and_clamps() {
        let source = "let x = 42";
        assert_eq!(TextSpan::new(4, 1).text(source), "x");
        assert_eq!(TextSpan::new(8, 2).text(source), "42");
        assert_eq!(TextSpan::new(100, 5).text(source), "");
        assert_eq!(TextSpan::new(8, 999).text(source), "42");
    }
}
