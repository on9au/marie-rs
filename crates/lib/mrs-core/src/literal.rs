//! Parsing of MARIE numeric literals.
//!
//! The rules here are the ones the MARIE.js assembler applies to the `DEC`, `OCT`
//! and `HEX` directives, so an assembler and a linter built on this crate agree with
//! the reference implementation on exactly which literals are accepted.

use std::fmt;

use crate::value::Value;

/// The base a literal is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radix {
    /// Base 2.
    Binary,
    /// Base 8, as written by the `OCT` directive.
    Octal,
    /// Base 10, as written by the `DEC` directive.
    Decimal,
    /// Base 16, as written by the `HEX` directive.
    Hexadecimal,
}

impl Radix {
    /// Returns the numeric base.
    pub const fn base(self) -> u32 {
        match self {
            Radix::Binary => 2,
            Radix::Octal => 8,
            Radix::Decimal => 10,
            Radix::Hexadecimal => 16,
        }
    }

    /// Returns `true` if a literal in this radix may carry a sign.
    ///
    /// Only decimal literals may be negative; the other bases are written as raw bit
    /// patterns.
    pub const fn allows_sign(self) -> bool {
        matches!(self, Radix::Decimal)
    }
}

impl fmt::Display for Radix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Radix::Binary => "binary",
            Radix::Octal => "octal",
            Radix::Decimal => "decimal",
            Radix::Hexadecimal => "hexadecimal",
        };
        f.write_str(name)
    }
}

/// Why a literal could not be turned into a 16-bit word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseWordError {
    /// The literal had no digits.
    Empty,
    /// The literal contained a character that is not a digit in its radix.
    InvalidDigit {
        /// The radix the literal was read in.
        radix: Radix,
    },
    /// The literal carried a sign in a radix that does not permit one.
    UnexpectedSign {
        /// The radix the literal was read in.
        radix: Radix,
    },
    /// The literal does not fit in a 16-bit word.
    OutOfRange,
}

impl fmt::Display for ParseWordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseWordError::Empty => f.write_str("expected a numeric literal"),
            ParseWordError::InvalidDigit { radix } => {
                write!(f, "not a valid {radix} literal")
            }
            ParseWordError::UnexpectedSign { radix } => {
                write!(f, "a {radix} literal cannot be signed")
            }
            ParseWordError::OutOfRange => {
                f.write_str("literal does not fit in a 16-bit word (-32768..=65535)")
            }
        }
    }
}

impl std::error::Error for ParseWordError {}

/// Parses a literal in the given radix into a machine word.
///
/// Decimal literals may be negative and must lie in `-32768..=65535`; literals in
/// every other radix are unsigned and must lie in `0..=65535`. Values above `32767`
/// are reinterpreted as the equivalent bit pattern, so `HEX FFFF` and `DEC -1` both
/// produce `-1`.
///
/// # Errors
///
/// Returns [`ParseWordError`] if the literal is empty, contains a digit that is not
/// valid in `radix`, is signed in a radix that forbids signs, or is out of range.
pub fn parse_word(literal: &str, radix: Radix) -> Result<Value, ParseWordError> {
    let (negative, digits) = split_sign(literal);
    if negative && !radix.allows_sign() {
        return Err(ParseWordError::UnexpectedSign { radix });
    }
    finish(negative, digits, radix)
}

/// Parses a literal whose radix is given by a `0x`, `0o` or `0b` prefix, defaulting
/// to decimal.
///
/// Unlike [`parse_word`], a sign is accepted in every radix; this is the lenient form
/// used for interactive input rather than for assembling source.
///
/// # Errors
///
/// Returns [`ParseWordError`] on the same conditions as [`parse_word`], except that a
/// sign is never rejected.
pub fn parse_prefixed_word(literal: &str) -> Result<Value, ParseWordError> {
    let (negative, body) = split_sign(literal);

    // `get` returns `None` rather than panicking on a non-ASCII boundary.
    let prefix = body.get(..2).map(str::to_ascii_lowercase);
    let (radix, digits) = match prefix.as_deref() {
        Some("0x") => (Radix::Hexadecimal, &body[2..]),
        Some("0o") => (Radix::Octal, &body[2..]),
        Some("0b") => (Radix::Binary, &body[2..]),
        _ => (Radix::Decimal, body),
    };

    finish(negative, digits, radix)
}

/// Splits an optional leading `+`/`-` off a literal.
fn split_sign(literal: &str) -> (bool, &str) {
    let trimmed = literal.trim();
    match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (
            false,
            trimmed.strip_prefix('+').map_or(trimmed, str::trim_start),
        ),
    }
}

/// Converts a sign and a digit string into a word, applying the 16-bit range rules.
fn finish(negative: bool, digits: &str, radix: Radix) -> Result<Value, ParseWordError> {
    if digits.is_empty() {
        return Err(ParseWordError::Empty);
    }
    // `u32::from_str_radix` would accept its own sign characters; they have already
    // been stripped, so any remaining one is a malformed digit.
    if !digits.chars().all(|c| c.is_digit(radix.base())) {
        return Err(ParseWordError::InvalidDigit { radix });
    }

    let magnitude =
        u32::from_str_radix(digits, radix.base()).map_err(|_| ParseWordError::OutOfRange)?;

    // A word holds -32768..=32767 signed, or 0..=65535 as a raw bit pattern.
    let limit = if negative {
        1 << 15
    } else {
        u32::from(u16::MAX)
    };
    if magnitude > limit {
        return Err(ParseWordError::OutOfRange);
    }

    let word = Value::from_bits(magnitude as u16);
    Ok(if negative { -word } else { word })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_radix() {
        assert_eq!(parse_word("1010", Radix::Binary), Ok(Value::new(10)));
        assert_eq!(parse_word("17", Radix::Octal), Ok(Value::new(15)));
        assert_eq!(parse_word("42", Radix::Decimal), Ok(Value::new(42)));
        assert_eq!(parse_word("ff", Radix::Hexadecimal), Ok(Value::new(255)));
        assert_eq!(parse_word("FF", Radix::Hexadecimal), Ok(Value::new(255)));
    }

    #[test]
    fn only_decimal_literals_may_be_signed() {
        assert_eq!(parse_word("-42", Radix::Decimal), Ok(Value::new(-42)));
        assert_eq!(
            parse_word("-1", Radix::Hexadecimal),
            Err(ParseWordError::UnexpectedSign {
                radix: Radix::Hexadecimal
            })
        );
    }

    #[test]
    fn applies_the_16_bit_range_rules() {
        // Unsigned values above 32767 become the equivalent bit pattern.
        assert_eq!(parse_word("FFFF", Radix::Hexadecimal), Ok(Value::new(-1)));
        assert_eq!(parse_word("65535", Radix::Decimal), Ok(Value::new(-1)));
        assert_eq!(
            parse_word("-32768", Radix::Decimal),
            Ok(Value::new(i16::MIN))
        );
        assert_eq!(
            parse_word("65536", Radix::Decimal),
            Err(ParseWordError::OutOfRange)
        );
        assert_eq!(
            parse_word("-32769", Radix::Decimal),
            Err(ParseWordError::OutOfRange)
        );
        assert_eq!(
            parse_word("10000", Radix::Hexadecimal),
            Err(ParseWordError::OutOfRange)
        );
    }

    #[test]
    fn rejects_digits_outside_the_radix() {
        assert_eq!(
            parse_word("19", Radix::Octal),
            Err(ParseWordError::InvalidDigit {
                radix: Radix::Octal
            })
        );
        assert_eq!(
            parse_word("2", Radix::Binary),
            Err(ParseWordError::InvalidDigit {
                radix: Radix::Binary
            })
        );
        assert_eq!(
            parse_word("ff", Radix::Decimal),
            Err(ParseWordError::InvalidDigit {
                radix: Radix::Decimal
            })
        );
        assert_eq!(parse_word("", Radix::Decimal), Err(ParseWordError::Empty));
    }

    #[test]
    fn prefixed_literals_pick_their_radix() {
        assert_eq!(parse_prefixed_word("42"), Ok(Value::new(42)));
        assert_eq!(parse_prefixed_word("-42"), Ok(Value::new(-42)));
        assert_eq!(parse_prefixed_word("+42"), Ok(Value::new(42)));
        assert_eq!(parse_prefixed_word("0x10"), Ok(Value::new(16)));
        assert_eq!(parse_prefixed_word("0X10"), Ok(Value::new(16)));
        assert_eq!(parse_prefixed_word("0o10"), Ok(Value::new(8)));
        assert_eq!(parse_prefixed_word("0b1010"), Ok(Value::new(10)));
        // Signs are allowed in every radix here.
        assert_eq!(parse_prefixed_word("-0x10"), Ok(Value::new(-16)));
        assert_eq!(parse_prefixed_word("0xFFFF"), Ok(Value::new(-1)));
    }

    #[test]
    fn prefixed_parsing_rejects_junk_without_panicking() {
        assert!(parse_prefixed_word("").is_err());
        assert!(parse_prefixed_word("banana").is_err());
        assert!(parse_prefixed_word("0x").is_err());
        assert!(parse_prefixed_word("65536").is_err());
        // Must not slice through a multi-byte character.
        assert!(parse_prefixed_word("\u{e9}9").is_err());
    }
}
