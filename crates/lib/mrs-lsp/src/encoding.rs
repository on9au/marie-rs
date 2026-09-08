//! Converting between byte offsets and LSP positions.
//!
//! Every span this workspace produces is a byte offset, because that is what makes
//! them cheap and unambiguous. LSP positions are not: a `Position.character` counts
//! *code units* in an encoding the client and server negotiate, and the default is
//! UTF-16. Getting this wrong is invisible in ASCII source and silently misplaces
//! every range as soon as a label contains a non-ASCII character, so the conversion
//! lives in one place with its own tests rather than being open-coded per feature.
//!
//! The three encodings differ only in how a character is counted:
//!
//! | character | UTF-8 | UTF-16 | UTF-32 |
//! |-----------|-------|--------|--------|
//! | `a`       | 1     | 1      | 1      |
//! | `é`       | 2     | 1      | 1      |
//! | `😀`      | 4     | 2      | 1      |

use lsp_types::{Position, PositionEncodingKind, Range};
use mrs_asm::span::{LineIndex, Span};

/// How a client counts columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    /// Columns are byte offsets. Free to compute, but a client must opt in.
    Utf8,
    /// Columns are UTF-16 code units. The protocol default, which every client
    /// supports and which a server must therefore be able to speak.
    #[default]
    Utf16,
    /// Columns are Unicode scalar values.
    Utf32,
}

impl PositionEncoding {
    /// Picks the best encoding the client will accept.
    ///
    /// UTF-8 is preferred because it needs no conversion at all; UTF-16 is the
    /// fallback, since the protocol requires every client to support it.
    pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> Self {
        let Some(offered) = offered else {
            return PositionEncoding::Utf16;
        };
        for kind in offered {
            if *kind == PositionEncodingKind::UTF8 {
                return PositionEncoding::Utf8;
            }
        }
        for kind in offered {
            if *kind == PositionEncodingKind::UTF32 {
                return PositionEncoding::Utf32;
            }
        }
        PositionEncoding::Utf16
    }

    /// The protocol name for this encoding.
    pub fn kind(self) -> PositionEncodingKind {
        match self {
            PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
            PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
            PositionEncoding::Utf32 => PositionEncodingKind::UTF32,
        }
    }

    /// Measures `text` in this encoding's code units.
    pub fn measure(self, text: &str) -> u32 {
        match self {
            PositionEncoding::Utf8 => text.len() as u32,
            PositionEncoding::Utf16 => text.chars().map(|c| c.len_utf16() as u32).sum(),
            PositionEncoding::Utf32 => text.chars().count() as u32,
        }
    }

    /// The width of one character in this encoding.
    fn width(self, character: char) -> u32 {
        match self {
            PositionEncoding::Utf8 => character.len_utf8() as u32,
            PositionEncoding::Utf16 => character.len_utf16() as u32,
            PositionEncoding::Utf32 => 1,
        }
    }
}

/// Converts between this workspace's spans and a client's positions.
#[derive(Debug, Clone, Copy)]
pub struct Positions<'a> {
    source: &'a str,
    lines: &'a LineIndex,
    encoding: PositionEncoding,
}

impl<'a> Positions<'a> {
    /// Builds a converter for one document.
    pub fn new(source: &'a str, lines: &'a LineIndex, encoding: PositionEncoding) -> Self {
        Self {
            source,
            lines,
            encoding,
        }
    }

    /// Converts a byte offset into an LSP position.
    pub fn position(&self, offset: u32) -> Position {
        let line = self.lines.line_of(offset);
        let start = self.lines.line_start(line).unwrap_or(0);
        // A stale offset must not panic, and must not slice through a character.
        let prefix = self
            .source
            .get(start as usize..offset as usize)
            .unwrap_or_default();
        Position::new(line, self.encoding.measure(prefix))
    }

    /// Converts an LSP position into a byte offset.
    ///
    /// A column past the end of the line clamps to the line's end, which is what an
    /// editor means when the caret sits in the virtual space after the text.
    pub fn offset(&self, position: Position) -> Option<u32> {
        let start = self.lines.line_start(position.line)?;
        let end = self.lines.line_end(position.line)?;
        let line = self.source.get(start as usize..end as usize)?;

        let mut units = 0u32;
        for (byte, character) in line.char_indices() {
            if units >= position.character {
                return Some(start + byte as u32);
            }
            units += self.encoding.width(character);
        }
        Some(end)
    }

    /// Converts a span into an LSP range.
    pub fn range(&self, span: Span) -> Range {
        Range::new(self.position(span.start), self.position(span.end))
    }

    /// Converts an LSP position into a byte offset, clamping instead of failing.
    ///
    /// A position past the end of the document clamps to its end. Clients are not
    /// supposed to send those, but a stale request racing an edit can, and answering
    /// nothing at all is a worse response than answering about the whole file.
    pub fn offset_clamped(&self, position: Position) -> u32 {
        if let Some(offset) = self.offset(position) {
            return offset;
        }
        if position.line >= self.lines.line_count() {
            self.source.len() as u32
        } else {
            0
        }
    }

    /// Converts an LSP range into a span, clamping to the document.
    pub fn span_clamped(&self, range: Range) -> Span {
        let start = self.offset_clamped(range.start);
        let end = self.offset_clamped(range.end);
        Span::new(start.min(end), start.max(end))
    }

    /// Converts an LSP range into a span.
    pub fn span(&self, range: Range) -> Option<Span> {
        Some(Span::new(
            self.offset(range.start)?,
            self.offset(range.end)?,
        ))
    }

    /// The length of a span in the negotiated encoding, for semantic tokens.
    pub fn length(&self, span: Span) -> u32 {
        let text = span.text(self.source).unwrap_or_default();
        self.encoding.measure(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `é` is two bytes and one UTF-16 unit; `😀` is four bytes and two.
    const SOURCE: &str = "caf\u{e9}, Load X\n\u{1F600}b, Halt\n";

    fn positions(encoding: PositionEncoding) -> (LineIndex, PositionEncoding) {
        (LineIndex::new(SOURCE), encoding)
    }

    #[test]
    fn columns_are_counted_in_the_negotiated_encoding() {
        // The comma after `café` is the fourth character of the line.
        let comma = SOURCE.find(',').unwrap() as u32;
        for (encoding, expected) in [
            (PositionEncoding::Utf8, 5),
            (PositionEncoding::Utf16, 4),
            (PositionEncoding::Utf32, 4),
        ] {
            let (lines, encoding) = positions(encoding);
            let converter = Positions::new(SOURCE, &lines, encoding);
            assert_eq!(
                converter.position(comma),
                Position::new(0, expected),
                "{encoding:?}"
            );
        }
    }

    #[test]
    fn an_astral_character_is_two_units_in_utf16_and_one_in_utf32() {
        let b = SOURCE.find('b').unwrap() as u32;
        for (encoding, expected) in [
            (PositionEncoding::Utf8, 4),
            (PositionEncoding::Utf16, 2),
            (PositionEncoding::Utf32, 1),
        ] {
            let (lines, encoding) = positions(encoding);
            let converter = Positions::new(SOURCE, &lines, encoding);
            assert_eq!(
                converter.position(b),
                Position::new(1, expected),
                "{encoding:?}"
            );
        }
    }

    #[test]
    fn positions_round_trip_through_offsets_in_every_encoding() {
        for encoding in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            let lines = LineIndex::new(SOURCE);
            let converter = Positions::new(SOURCE, &lines, encoding);
            // Only character boundaries round-trip; interior bytes have no position.
            for (offset, _) in SOURCE.char_indices() {
                let offset = offset as u32;
                let position = converter.position(offset);
                assert_eq!(
                    converter.offset(position),
                    Some(offset),
                    "{encoding:?} at {offset}"
                );
            }
        }
    }

    #[test]
    fn a_column_past_the_end_of_a_line_clamps_to_it() {
        let lines = LineIndex::new(SOURCE);
        let converter = Positions::new(SOURCE, &lines, PositionEncoding::Utf16);
        let end = converter.offset(Position::new(0, 9_999)).unwrap();
        assert_eq!(lines.line_of(end), 0, "stays on the same line");
        assert_eq!(&SOURCE[end as usize..end as usize + 1], "\n");
    }

    #[test]
    fn an_out_of_range_line_has_no_offset() {
        let lines = LineIndex::new(SOURCE);
        let converter = Positions::new(SOURCE, &lines, PositionEncoding::Utf16);
        assert_eq!(converter.offset(Position::new(999, 0)), None);
    }

    #[test]
    fn a_range_past_the_end_clamps_to_the_document() {
        let lines = LineIndex::new(SOURCE);
        let converter = Positions::new(SOURCE, &lines, PositionEncoding::Utf16);
        let whole = converter.span_clamped(Range::new(Position::new(0, 0), Position::new(999, 0)));
        assert_eq!(whole, Span::new(0, SOURCE.len() as u32));
        assert_eq!(
            converter.span(Range::new(Position::new(0, 0), Position::new(999, 0),)),
            None,
            "the strict form still refuses"
        );
    }

    #[test]
    fn utf8_is_preferred_and_utf16_is_the_fallback() {
        assert_eq!(
            PositionEncoding::negotiate(Some(&[
                PositionEncodingKind::UTF16,
                PositionEncodingKind::UTF8
            ])),
            PositionEncoding::Utf8
        );
        assert_eq!(
            PositionEncoding::negotiate(Some(&[PositionEncodingKind::UTF16])),
            PositionEncoding::Utf16
        );
        assert_eq!(
            PositionEncoding::negotiate(Some(&[PositionEncodingKind::UTF32])),
            PositionEncoding::Utf32
        );
        // A client that says nothing gets the protocol default.
        assert_eq!(PositionEncoding::negotiate(None), PositionEncoding::Utf16);
        assert_eq!(
            PositionEncoding::negotiate(Some(&[])),
            PositionEncoding::Utf16
        );
    }

    #[test]
    fn span_lengths_are_measured_in_the_negotiated_encoding() {
        let lines = LineIndex::new(SOURCE);
        let label = Span::new(0, 5); // "café"
        assert_eq!(
            Positions::new(SOURCE, &lines, PositionEncoding::Utf8).length(label),
            5
        );
        assert_eq!(
            Positions::new(SOURCE, &lines, PositionEncoding::Utf16).length(label),
            4
        );
    }
}
