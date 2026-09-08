//! Splitting source lines into their syntactic parts.
//!
//! MARIE assembly is line-oriented, and MARIE.js recognises a line with one of three
//! regular expressions tried in order: a blank-or-comment line, an `ORG` directive, and
//! the general `label, operator operand / comment` form. This module reproduces those
//! three, byte for byte in their acceptance, but hands back spans instead of strings.
//!
//! # Why this is hand-written
//!
//! The general form is
//!
//! ```text
//! ^\s*(?:(?<label>[^,/]+),)?\s*(?<operator>[^\s,]+?)(?:\s+(?<operand>[^\s,]+?))?\s*(?:/.*)?$
//! ```
//!
//! and its two lazy quantifiers mean the split is decided by backtracking, not by a
//! simple scan. `Load/c` is `Load` plus the comment `/c`, but `/c` on its own is an
//! operator named `/c`, because stopping earlier leaves nothing that can match the
//! tail. Reimplementing this as "split on the first slash" would quietly diverge, so
//! the search order of the original is reproduced directly: try the shortest operator,
//! then prefer an operand over none, then require the tail to match.
//!
//! The crate has no dependencies, so pulling in a regex engine was not an option
//! either — but even with one, the subtlety above is worth spelling out in code.

use crate::span::Span;

/// The kind of a lexical token, for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenKind {
    /// A label definition, not including the trailing comma.
    Label,
    /// The comma that terminates a label.
    Comma,
    /// The `ORG` keyword.
    OriginKeyword,
    /// An instruction mnemonic or directive name.
    Mnemonic,
    /// The operand: a literal or a label reference.
    Operand,
    /// A `/` comment, running to the end of the line.
    Comment,
    /// Text that fits none of the above, on a line that failed to parse.
    Unknown,
}

/// A lexical token with its source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// Where it is.
    pub span: Span,
}

impl Token {
    /// Creates a token.
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// How a single source line was recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineForm {
    /// Empty, whitespace, or nothing but a comment. Contributes no word.
    Blank {
        /// The comment, if the line had one.
        comment: Option<Span>,
    },
    /// An `ORG hhh` directive, whose operand is always exactly three hex digits.
    Origin {
        /// The `ORG` keyword itself.
        keyword: Span,
        /// The three hex digits.
        digits: Span,
        /// The trailing comment, if any.
        comment: Option<Span>,
    },
    /// A labelled or unlabelled statement.
    Statement {
        /// The label, without its comma, if the line defines one.
        label: Option<Span>,
        /// The mnemonic.
        mnemonic: Span,
        /// The operand, if the line has one.
        operand: Option<Span>,
        /// The trailing comment, if any.
        comment: Option<Span>,
    },
    /// The line matched none of the three forms.
    Malformed,
}

/// Recognises one line, whose first byte sits at `base` in the whole source.
///
/// `line` must not contain a `\n`. A trailing `\r` from a CRLF file is treated as
/// whitespace, exactly as JavaScript's `\s` does.
pub fn split_line(line: &str, base: u32) -> LineForm {
    let cursor = Cursor::new(line, base);
    if let Some(form) = cursor.blank() {
        return form;
    }
    if let Some(form) = cursor.origin() {
        return form;
    }
    cursor.statement().unwrap_or(LineForm::Malformed)
}

/// Tokenizes a whole source file for syntax highlighting.
///
/// Lines after an `END` directive are still tokenized: an editor colours text the
/// assembler will never look at, and stopping early would leave it grey.
pub fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut base = 0u32;
    for line in source.split('\n') {
        push_line_tokens(&mut tokens, line, base);
        // `+ 1` steps over the `\n` that `split` removed.
        base += line.len() as u32 + 1;
    }
    tokens
}

/// Appends the tokens of one line, in source order.
fn push_line_tokens(tokens: &mut Vec<Token>, line: &str, base: u32) {
    match split_line(line, base) {
        LineForm::Blank { comment } => {
            tokens.extend(comment.map(|span| Token::new(TokenKind::Comment, span)));
        }
        LineForm::Origin {
            keyword,
            digits,
            comment,
        } => {
            tokens.push(Token::new(TokenKind::OriginKeyword, keyword));
            tokens.push(Token::new(TokenKind::Operand, digits));
            tokens.extend(comment.map(|span| Token::new(TokenKind::Comment, span)));
        }
        LineForm::Statement {
            label,
            mnemonic,
            operand,
            comment,
        } => {
            if let Some(label) = label {
                tokens.push(Token::new(TokenKind::Label, label));
                // The comma is the single byte the label span stops short of.
                tokens.push(Token::new(
                    TokenKind::Comma,
                    Span::new(label.end, label.end + 1),
                ));
            }
            tokens.push(Token::new(TokenKind::Mnemonic, mnemonic));
            tokens.extend(operand.map(|span| Token::new(TokenKind::Operand, span)));
            tokens.extend(comment.map(|span| Token::new(TokenKind::Comment, span)));
        }
        LineForm::Malformed => {
            let trimmed = line.trim_end();
            if !trimmed.trim_start().is_empty() {
                let start = trimmed.len() - trimmed.trim_start().len();
                tokens.push(Token::new(
                    TokenKind::Unknown,
                    Span::new(base + start as u32, base + trimmed.len() as u32),
                ));
            }
        }
    }
}

/// A line, indexed by character so that spans never split a UTF-8 sequence.
struct Cursor<'a> {
    /// `(byte offset, character)` for each character in the line.
    chars: Vec<(usize, char)>,
    base: u32,
    /// Byte length of the line, for spans that reach its end.
    length: usize,
    line: &'a str,
}

impl<'a> Cursor<'a> {
    fn new(line: &'a str, base: u32) -> Self {
        Self {
            chars: line.char_indices().collect(),
            base,
            length: line.len(),
            line,
        }
    }

    /// Number of characters in the line.
    fn len(&self) -> usize {
        self.chars.len()
    }

    /// The character at index `i`, or `None` at the end of the line.
    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).map(|(_, c)| *c)
    }

    /// The byte offset of character index `i`, where `len()` maps to the line's end.
    fn offset(&self, i: usize) -> u32 {
        let byte = match self.chars.get(i) {
            Some((offset, _)) => *offset,
            None => self.length,
        };
        self.base + byte as u32
    }

    /// A span covering character indices `start..end`.
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.offset(start), self.offset(end))
    }

    /// The text of character indices `start..end`.
    fn text(&self, start: usize, end: usize) -> &'a str {
        let from = self.chars.get(start).map_or(self.length, |(o, _)| *o);
        let to = self.chars.get(end).map_or(self.length, |(o, _)| *o);
        &self.line[from..to]
    }

    /// Advances past a run of whitespace, implementing `\s*`.
    fn skip_whitespace(&self, mut i: usize) -> usize {
        while self.at(i).is_some_and(char::is_whitespace) {
            i += 1;
        }
        i
    }

    /// `^\s*(?:/.*)?$` — an empty, whitespace-only, or comment-only line.
    fn blank(&self) -> Option<LineForm> {
        let i = self.skip_whitespace(0);
        match self.at(i) {
            None => Some(LineForm::Blank { comment: None }),
            Some('/') => Some(LineForm::Blank {
                comment: Some(self.span(i, self.len())),
            }),
            Some(_) => None,
        }
    }

    /// `^\s*org\s+([0-9a-f]{3})\s*(?:/.*)?$`, case-insensitively.
    ///
    /// The three-digit operand is not negotiable: `ORG 10` and `ORG 1000` both fall
    /// through to the statement form, where `org` is reported as an unknown mnemonic.
    fn origin(&self) -> Option<LineForm> {
        let start = self.skip_whitespace(0);
        let keyword_end = start + 3;
        if keyword_end > self.len() || !self.text(start, keyword_end).eq_ignore_ascii_case("org") {
            return None;
        }
        // `\s+` requires at least one space between the keyword and the digits.
        if !self.at(keyword_end).is_some_and(char::is_whitespace) {
            return None;
        }
        let digits_start = self.skip_whitespace(keyword_end);
        let digits_end = digits_start + 3;
        if digits_end > self.len() {
            return None;
        }
        if !self
            .text(digits_start, digits_end)
            .chars()
            .all(|c| c.is_ascii_hexdigit())
        {
            return None;
        }
        // A fourth hex digit leaves a character the tail cannot consume, so `ORG 1000`
        // is not an origin directive at all.
        let comment = self.tail(digits_end)?;
        Some(LineForm::Origin {
            keyword: self.span(start, keyword_end),
            digits: self.span(digits_start, digits_end),
            comment,
        })
    }

    /// The general statement form, reproducing the original's backtracking order.
    fn statement(&self) -> Option<LineForm> {
        let leading = self.skip_whitespace(0);
        // `\s*` is greedy, so the longest run of leading whitespace is tried first; the
        // label group is greedy-optional, so a label is preferred over none. Shrinking
        // the whitespace only matters for a line like `  , Load X`, where it lets the
        // label match a single space and produce a whitespace error rather than a
        // shapeless one.
        for taken in (0..=leading).rev() {
            if let Some(form) = self.with_label(taken) {
                return Some(form);
            }
            if taken == leading {
                // Skipping the label group leaves the following `\s*` to absorb the
                // rest of the whitespace, so every value of `taken` lands here; try it
                // once, in the position the greedy match reaches.
                if let Some(form) = self.body(leading, None) {
                    return Some(form);
                }
            }
        }
        None
    }

    /// `(?<label>[^,/]+),` starting at character index `start`.
    fn with_label(&self, start: usize) -> Option<LineForm> {
        let mut i = start;
        while let Some(c) = self.at(i) {
            if c == ',' || c == '/' {
                break;
            }
            i += 1;
        }
        // `[^,/]+` runs to the first comma or slash; the comma is required, and it must
        // have consumed at least one character. Shrinking the run cannot help, because
        // there is no earlier comma for it to stop at.
        if self.at(i) != Some(',') || i == start {
            return None;
        }
        self.body(i + 1, Some(self.span(start, i)))
    }

    /// `\s*(?<operator>[^\s,]+?)(?:\s+(?<operand>[^\s,]+?))?\s*(?:/.*)?$`
    fn body(&self, start: usize, label: Option<Span>) -> Option<LineForm> {
        let mnemonic_start = self.skip_whitespace(start);
        // The operator is lazy: try the shortest first and stop at the first length
        // whose remainder matches.
        for end in mnemonic_start + 1..=self.len() {
            let c = self.at(end - 1)?;
            if c.is_whitespace() || c == ',' {
                // `[^\s,]` cannot cover this character, and no longer operator can
                // avoid it either.
                return None;
            }
            if let Some((operand, comment)) = self.after_mnemonic(end) {
                return Some(LineForm::Statement {
                    label,
                    mnemonic: self.span(mnemonic_start, end),
                    operand,
                    comment,
                });
            }
        }
        None
    }

    /// The optional operand group, then the tail. Returns `(operand, comment)`.
    fn after_mnemonic(&self, start: usize) -> Option<(Option<Span>, Option<Span>)> {
        // `(?:\s+ ...)?` is greedy-optional, so an operand is preferred over none.
        if self.at(start).is_some_and(char::is_whitespace) {
            let operand_start = self.skip_whitespace(start);
            for end in operand_start + 1..=self.len() {
                let c = self.at(end - 1)?;
                if c.is_whitespace() || c == ',' {
                    break;
                }
                if let Some(comment) = self.tail(end) {
                    return Some((Some(self.span(operand_start, end)), comment));
                }
            }
        }
        // Fall back to no operand at all.
        self.tail(start).map(|comment| (None, comment))
    }

    /// `\s*(?:/.*)?$` — returns `Some(comment)` if the rest of the line is consumable.
    ///
    /// The outer `Option` says whether the tail matched; the inner one says whether
    /// there was a comment.
    fn tail(&self, start: usize) -> Option<Option<Span>> {
        let i = self.skip_whitespace(start);
        match self.at(i) {
            None => Some(None),
            Some('/') => Some(Some(self.span(i, self.len()))),
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a form as `label|mnemonic|operand|comment` for compact assertions.
    fn shape(line: &str) -> String {
        let part = |span: Option<Span>| match span {
            Some(span) => span.text(line).unwrap_or("<bad>").to_owned(),
            None => "-".to_owned(),
        };
        match split_line(line, 0) {
            LineForm::Blank { comment } => format!("blank|{}", part(comment)),
            LineForm::Origin {
                digits, comment, ..
            } => format!("org|{}|{}", part(Some(digits)), part(comment)),
            LineForm::Statement {
                label,
                mnemonic,
                operand,
                comment,
            } => format!(
                "{}|{}|{}|{}",
                part(label),
                part(Some(mnemonic)),
                part(operand),
                part(comment)
            ),
            LineForm::Malformed => "malformed".to_owned(),
        }
    }

    #[test]
    fn recognises_the_ordinary_shapes() {
        assert_eq!(shape(""), "blank|-");
        assert_eq!(shape("   \t "), "blank|-");
        assert_eq!(shape("  / just a comment"), "blank|/ just a comment");
        assert_eq!(shape("Halt"), "-|Halt|-|-");
        assert_eq!(shape("  Load X"), "-|Load|X|-");
        assert_eq!(shape("Foo, Load X"), "Foo|Load|X|-");
        assert_eq!(shape("Foo,Load X"), "Foo|Load|X|-");
        assert_eq!(shape("Foo, Load X / note"), "Foo|Load|X|/ note");
    }

    #[test]
    fn origin_requires_exactly_three_hex_digits() {
        assert_eq!(shape("ORG 100"), "org|100|-");
        assert_eq!(shape("  org 0ff / start"), "org|0ff|/ start");
        assert_eq!(shape("OrG A0B"), "org|A0B|-");
        // Too few, too many, or non-hex fall through to the statement form, where
        // `org` will be reported as an unknown mnemonic.
        assert_eq!(shape("ORG 10"), "-|ORG|10|-");
        assert_eq!(shape("ORG 1000"), "-|ORG|1000|-");
        assert_eq!(shape("ORG xyz"), "-|ORG|xyz|-");
        // `\s+` is required between the keyword and the digits.
        assert_eq!(shape("ORG100"), "-|ORG100|-|-");
    }

    #[test]
    fn comments_bind_the_way_the_lazy_operator_makes_them() {
        // A slash directly after a complete operator starts a comment...
        assert_eq!(shape("Load/c"), "-|Load|-|/c");
        assert_eq!(shape("Load X/c"), "-|Load|X|/c");
        // ...but a slash with nothing before it has to be part of the operator,
        // because an empty operator cannot match.
        assert_eq!(shape("Foo, /c"), "Foo|/c|-|-");
        // A slash inside what would be a label defeats the label group entirely.
        assert_eq!(shape("Fo/o, Load X"), "-|Fo|-|/o, Load X");
    }

    #[test]
    fn malformed_lines_are_rejected_rather_than_guessed_at() {
        // Two operands: the operand cannot contain whitespace and nothing can absorb
        // the third word.
        assert_eq!(shape("Load X Y"), "malformed");
        // A comma with no label before it.
        assert_eq!(shape(", Load X"), "malformed");
        // A trailing comma leaves no operator.
        assert_eq!(shape("Foo,"), "malformed");
    }

    #[test]
    fn labels_keep_the_whitespace_that_makes_them_invalid() {
        // The label group stops at the comma, so a space before it lands in the label
        // and is reported later as a whitespace error rather than silently trimmed.
        assert_eq!(shape("Foo , Load X"), "Foo |Load|X|-");
        assert_eq!(shape("My Label, Halt"), "My Label|Halt|-|-");
        // Shrinking the leading whitespace lets a single space become the label.
        assert_eq!(shape("  , Halt"), " |Halt|-|-");
    }

    #[test]
    fn spans_survive_multi_byte_characters() {
        let line = "caf\u{e9}, Load X";
        let LineForm::Statement { label, .. } = split_line(line, 0) else {
            panic!("expected a statement");
        };
        assert_eq!(label.unwrap().text(line), Some("caf\u{e9}"));
    }

    #[test]
    fn a_carriage_return_counts_as_trailing_whitespace() {
        assert_eq!(shape("Load X\r"), "-|Load|X|-");
        assert_eq!(shape("\r"), "blank|-");
    }

    #[test]
    fn tokenize_walks_the_whole_file_with_absolute_spans() {
        let source = "Foo, Load X / c\nHalt\n";
        let tokens = tokenize(source);
        let rendered: Vec<_> = tokens
            .iter()
            .map(|t| (t.kind, t.span.text(source).unwrap()))
            .collect();
        assert_eq!(
            rendered,
            vec![
                (TokenKind::Label, "Foo"),
                (TokenKind::Comma, ","),
                (TokenKind::Mnemonic, "Load"),
                (TokenKind::Operand, "X"),
                (TokenKind::Comment, "/ c"),
                (TokenKind::Mnemonic, "Halt"),
            ]
        );
    }
}
