//! Source positions.
//!
//! Every syntactic element the assembler produces carries a [`Span`], so a linter can
//! underline it and a language server can turn it into an LSP range. Spans are byte
//! offsets into the original source, which keeps them cheap to store and compare;
//! [`LineIndex`] converts them to line/column pairs on demand.

use std::fmt;
use std::ops::Range;

/// A half-open byte range within a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    /// Byte offset of the first byte.
    pub start: u32,
    /// Byte offset one past the last byte.
    pub end: u32,
}

impl Span {
    /// Creates a span from a start and end offset.
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// An empty span at `offset`, for diagnostics that point between characters.
    pub const fn empty(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// Returns the length of the span in bytes.
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if the span covers no bytes.
    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    /// Returns `true` if `offset` falls inside the span.
    ///
    /// The end offset is included, so a caret sitting immediately after a token still
    /// resolves to it — which is what an editor wants for hover and go-to-definition.
    pub const fn touches(self, offset: u32) -> bool {
        offset >= self.start && offset <= self.end
    }

    /// Returns the smallest span covering both inputs.
    pub fn join(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Slices the source text this span refers to.
    ///
    /// Returns `None` if the span is out of bounds or splits a character, so a stale
    /// span cannot panic.
    pub fn text(self, source: &str) -> Option<&str> {
        source.get(self.start as usize..self.end as usize)
    }

    /// Returns the span as a [`Range`], for slicing and for LSP conversion.
    pub const fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl From<Range<u32>> for Span {
    fn from(range: Range<u32>) -> Self {
        Self::new(range.start, range.end)
    }
}

/// A zero-based line and column, in the shape a language server wants.
///
/// `column` counts UTF-8 bytes from the start of the line, not characters; convert to
/// UTF-16 code units at the LSP boundary if the client needs that encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based byte column within the line.
    pub column: u32,
}

impl Position {
    /// Creates a position.
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

impl fmt::Display for Position {
    /// Formats as the one-based `line:column` humans expect in an error message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.column + 1)
    }
}

/// Maps byte offsets to line/column positions and back.
///
/// Built once per source file; every lookup is a binary search over the line starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offset at which each line begins. Always starts with `0`.
    line_starts: Vec<u32>,
    length: u32,
}

impl LineIndex {
    /// Indexes a source file.
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset as u32 + 1);
            }
        }
        Self {
            line_starts,
            length: source.len() as u32,
        }
    }

    /// Returns the number of lines. A file always has at least one.
    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    /// Returns the byte offset at which `line` starts, if that line exists.
    pub fn line_start(&self, line: u32) -> Option<u32> {
        self.line_starts.get(line as usize).copied()
    }

    /// Returns the zero-based line containing `offset`.
    ///
    /// An offset past the end of the file resolves to the last line, so a caret at the
    /// very end of a buffer still has somewhere to go.
    pub fn line_of(&self, offset: u32) -> u32 {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line as u32,
            // `binary_search` gives the first line starting after `offset`; the line
            // containing it is the one before. Index 0 always holds `0 <= offset`, so
            // the result is never zero here and the subtraction cannot underflow.
            Err(next) => next as u32 - 1,
        }
    }

    /// Converts a byte offset into a line/column position.
    pub fn position(&self, offset: u32) -> Position {
        let line = self.line_of(offset);
        let start = self.line_starts[line as usize];
        Position::new(line, offset.saturating_sub(start))
    }

    /// Converts a line/column position back into a byte offset.
    ///
    /// Returns `None` if the line does not exist. A column past the end of the line is
    /// clamped to the line's end rather than rejected.
    pub fn offset(&self, position: Position) -> Option<u32> {
        let start = self.line_start(position.line)?;
        let end = self.line_end(position.line)?;
        Some((start + position.column).min(end))
    }

    /// Returns the byte offset one past the end of `line`, excluding the newline.
    pub fn line_end(&self, line: u32) -> Option<u32> {
        if line >= self.line_count() {
            return None;
        }
        Some(match self.line_starts.get(line as usize + 1) {
            // Step back over the `\n` that starts the next line.
            Some(next_start) => next_start - 1,
            None => self.length,
        })
    }

    /// Returns the span covering `line`, excluding its line terminator.
    pub fn line_span(&self, line: u32) -> Option<Span> {
        Some(Span::new(self.line_start(line)?, self.line_end(line)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_through_offsets() {
        let source = "abc\ndefg\n\nhi";
        let index = LineIndex::new(source);
        assert_eq!(index.line_count(), 4);
        for offset in 0..=source.len() as u32 {
            let position = index.position(offset);
            assert_eq!(index.offset(position), Some(offset), "offset {offset}");
        }
    }

    #[test]
    fn line_spans_exclude_the_newline() {
        let index = LineIndex::new("abc\ndefg\n\nhi");
        assert_eq!(index.line_span(0), Some(Span::new(0, 3)));
        assert_eq!(index.line_span(1), Some(Span::new(4, 8)));
        // An empty line is a zero-width span, not a missing one.
        assert_eq!(index.line_span(2), Some(Span::new(9, 9)));
        assert_eq!(index.line_span(3), Some(Span::new(10, 12)));
        assert_eq!(index.line_span(4), None);
    }

    #[test]
    fn an_offset_past_the_end_lands_on_the_last_line() {
        let index = LineIndex::new("ab\ncd");
        assert_eq!(index.position(5), Position::new(1, 2));
        assert_eq!(index.position(99).line, 1);
    }

    #[test]
    fn spans_slice_and_join() {
        let source = "Load X";
        let span = Span::new(0, 4);
        assert_eq!(span.text(source), Some("Load"));
        assert_eq!(span.join(Span::new(5, 6)), Span::new(0, 6));
        assert!(span.touches(0) && span.touches(4) && !span.touches(5));
        // A span that would split a multi-byte character yields nothing.
        assert_eq!(Span::new(0, 1).text("\u{e9}"), None);
    }
}
