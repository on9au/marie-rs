//! MARIE assembler directives.
//!
//! These are recognised in the mnemonic position of a source line but, unlike an
//! [`Opcode`](crate::Opcode), are handled by the assembler rather than the CPU.

use std::fmt;
use std::str::FromStr;

use crate::instruction::Opcode;
use crate::literal::Radix;

/// A directive understood by the MARIE assembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Directive {
    /// `ORG hhh` — sets the address the following code is assembled at.
    ///
    /// May appear only once, before any instruction, and its operand is always three
    /// hexadecimal digits.
    Org,
    /// `DEC n` — emits a signed or unsigned decimal literal.
    Dec,
    /// `OCT n` — emits an unsigned octal literal.
    Oct,
    /// `HEX n` — emits an unsigned hexadecimal literal.
    Hex,
    /// `ADR x` — emits an address as a bare word.
    ///
    /// Encoded as `JnS x`, since opcode `0x0` leaves the address in the low 12 bits.
    Adr,
    /// `Clear` — sets the accumulator to zero. An alias for `LoadImmi 0`.
    Clear,
    /// `END` — stops assembly. Anything after it is ignored.
    End,
}

impl Directive {
    /// Every directive.
    pub const ALL: [Directive; 7] = [
        Directive::Org,
        Directive::Dec,
        Directive::Oct,
        Directive::Hex,
        Directive::Adr,
        Directive::Clear,
        Directive::End,
    ];

    /// Looks up a directive by mnemonic, case-insensitively.
    pub fn from_mnemonic(mnemonic: &str) -> Option<Self> {
        Some(match mnemonic.to_ascii_lowercase().as_str() {
            "org" => Directive::Org,
            "dec" => Directive::Dec,
            "oct" => Directive::Oct,
            "hex" => Directive::Hex,
            "adr" => Directive::Adr,
            "clear" => Directive::Clear,
            "end" => Directive::End,
            _ => return None,
        })
    }

    /// Returns the canonical spelling of this directive.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Directive::Org => "ORG",
            Directive::Dec => "DEC",
            Directive::Oct => "OCT",
            Directive::Hex => "HEX",
            Directive::Adr => "ADR",
            Directive::Clear => "Clear",
            Directive::End => "END",
        }
    }

    /// Returns the radix of the literal this directive emits, if it emits one.
    pub const fn literal_radix(self) -> Option<Radix> {
        Some(match self {
            Directive::Dec => Radix::Decimal,
            Directive::Oct => Radix::Octal,
            Directive::Hex => Radix::Hexadecimal,
            _ => return None,
        })
    }

    /// Returns `true` if this directive requires an operand.
    ///
    /// `Clear` and `END` are the only ones that do not.
    pub const fn takes_operand(self) -> bool {
        !matches!(self, Directive::Clear | Directive::End)
    }

    /// Returns `true` if this directive emits a word into the program image.
    ///
    /// `ORG` and `END` steer the assembler instead of producing output.
    pub const fn emits_word(self) -> bool {
        !matches!(self, Directive::Org | Directive::End)
    }

    /// Returns the instruction this directive is an alias for, if it is one.
    ///
    /// `ADR x` assembles as `JnS x` and `Clear` as `LoadImmi 0`; the assembler can
    /// rewrite them and then take the ordinary instruction path.
    pub const fn alias_opcode(self) -> Option<Opcode> {
        Some(match self {
            Directive::Adr => Opcode::JnS,
            Directive::Clear => Opcode::LoadImmi,
            _ => return None,
        })
    }

    /// Returns a human-readable description of this directive.
    pub const fn description(self) -> &'static str {
        match self {
            Directive::Org => "Assemble the following code at address X",
            Directive::Dec => "Emit the decimal literal X",
            Directive::Oct => "Emit the octal literal X",
            Directive::Hex => "Emit the hexadecimal literal X",
            Directive::Adr => "Emit the address X as a word",
            Directive::Clear => "Set the AC to zero (an alias for LoadImmi 0)",
            Directive::End => "Stop assembling",
        }
    }
}

impl fmt::Display for Directive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

/// Returned when a mnemonic names neither an opcode nor a directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMnemonic(pub String);

impl fmt::Display for UnknownMnemonic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown mnemonic '{}'", self.0)
    }
}

impl std::error::Error for UnknownMnemonic {}

impl FromStr for Directive {
    type Err = UnknownMnemonic;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Directive::from_mnemonic(s).ok_or_else(|| UnknownMnemonic(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_round_trip_case_insensitively() {
        for directive in Directive::ALL {
            let mnemonic = directive.mnemonic();
            assert_eq!(Directive::from_mnemonic(mnemonic), Some(directive));
            assert_eq!(
                Directive::from_mnemonic(&mnemonic.to_ascii_uppercase()),
                Some(directive)
            );
            assert_eq!(
                Directive::from_mnemonic(&mnemonic.to_ascii_lowercase()),
                Some(directive)
            );
        }
        assert_eq!(Directive::from_mnemonic("nope"), None);
        assert!("nope".parse::<Directive>().is_err());
    }

    #[test]
    fn aliases_name_their_opcodes() {
        assert_eq!(Directive::Adr.alias_opcode(), Some(Opcode::JnS));
        assert_eq!(Directive::Clear.alias_opcode(), Some(Opcode::LoadImmi));
        assert_eq!(Directive::Dec.alias_opcode(), None);
    }

    #[test]
    fn literal_directives_name_their_radix() {
        assert_eq!(Directive::Dec.literal_radix(), Some(Radix::Decimal));
        assert_eq!(Directive::Oct.literal_radix(), Some(Radix::Octal));
        assert_eq!(Directive::Hex.literal_radix(), Some(Radix::Hexadecimal));
        assert_eq!(Directive::Org.literal_radix(), None);
    }

    #[test]
    fn steering_directives_emit_nothing() {
        assert!(!Directive::Org.emits_word());
        assert!(!Directive::End.emits_word());
        assert!(Directive::Dec.emits_word());
        assert!(Directive::Clear.emits_word());
        assert!(!Directive::Clear.takes_operand());
        assert!(!Directive::End.takes_operand());
        assert!(Directive::Org.takes_operand());
    }
}
