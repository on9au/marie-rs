//! Instruction encoding and decoding for the MARIE instruction set.
//!
//! A MARIE instruction word is 16 bits: a 4-bit opcode in bits 15-12 and a
//! 12-bit address field in bits 11-0.
//!
//! ```text
//!  15  12 11                     0
//! +------+------------------------+
//! | op   |        operand         |
//! +------+------------------------+
//! ```

use std::fmt;
use std::str::FromStr;

use crate::address::MemoryAddress;
use crate::directive::UnknownMnemonic;
use crate::value::Value;

/// A MARIE opcode.
///
/// This is the MARIE.js instruction set. Note that it differs from the textbook
/// (Null & Lobur) MARIE in one respect: opcode `0xA` is `LoadImmi`, which loads a
/// 12-bit *unsigned immediate*, rather than `Clear`. `Clear` is an assembler alias
/// for `LoadImmi 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Opcode {
    /// `JnS X` — store the return address at `X`, then jump to `X + 1`.
    JnS = 0x0,
    /// `Load X` — `AC <- M[X]`.
    Load = 0x1,
    /// `Store X` — `M[X] <- AC`.
    Store = 0x2,
    /// `Add X` — `AC <- AC + M[X]`.
    Add = 0x3,
    /// `Subt X` — `AC <- AC - M[X]`.
    Subt = 0x4,
    /// `Input` — `AC <- IN`.
    Input = 0x5,
    /// `Output` — `OUT <- AC`.
    Output = 0x6,
    /// `Halt` — stop execution.
    Halt = 0x7,
    /// `Skipcond C` — skip the next instruction if condition `C` holds.
    SkipCond = 0x8,
    /// `Jump X` — `PC <- X`.
    Jump = 0x9,
    /// `LoadImmi X` — `AC <- X`, where `X` is a 12-bit unsigned immediate.
    LoadImmi = 0xA,
    /// `AddI X` — `AC <- AC + M[M[X]]`.
    AddI = 0xB,
    /// `JumpI X` — `PC <- M[X]`.
    JumpI = 0xC,
    /// `LoadI X` — `AC <- M[M[X]]`.
    LoadI = 0xD,
    /// `StoreI X` — `M[M[X]] <- AC`.
    StoreI = 0xE,
}

impl Opcode {
    /// Every opcode, in numeric order.
    pub const ALL: [Opcode; 15] = [
        Opcode::JnS,
        Opcode::Load,
        Opcode::Store,
        Opcode::Add,
        Opcode::Subt,
        Opcode::Input,
        Opcode::Output,
        Opcode::Halt,
        Opcode::SkipCond,
        Opcode::Jump,
        Opcode::LoadImmi,
        Opcode::AddI,
        Opcode::JumpI,
        Opcode::LoadI,
        Opcode::StoreI,
    ];

    /// Decodes an opcode from its 4-bit encoding.
    ///
    /// Returns `None` for `0xF`, which is not assigned, and for any value that does
    /// not fit in a nibble.
    pub const fn from_nibble(nibble: u8) -> Option<Self> {
        Some(match nibble {
            0x0 => Opcode::JnS,
            0x1 => Opcode::Load,
            0x2 => Opcode::Store,
            0x3 => Opcode::Add,
            0x4 => Opcode::Subt,
            0x5 => Opcode::Input,
            0x6 => Opcode::Output,
            0x7 => Opcode::Halt,
            0x8 => Opcode::SkipCond,
            0x9 => Opcode::Jump,
            0xA => Opcode::LoadImmi,
            0xB => Opcode::AddI,
            0xC => Opcode::JumpI,
            0xD => Opcode::LoadI,
            0xE => Opcode::StoreI,
            _ => return None,
        })
    }

    /// Returns the 4-bit encoding of this opcode.
    pub const fn to_nibble(self) -> u8 {
        self as u8
    }

    /// Looks up an opcode by mnemonic, case-insensitively.
    ///
    /// This does not resolve the `Clear` and `ADR` aliases, which are
    /// [directives](crate::Directive) as far as the assembler is concerned.
    pub fn from_mnemonic(mnemonic: &str) -> Option<Self> {
        let lowered = mnemonic.to_ascii_lowercase();
        Opcode::ALL
            .into_iter()
            .find(|opcode| opcode.mnemonic().eq_ignore_ascii_case(&lowered))
    }

    /// Returns the assembly mnemonic for this opcode.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Opcode::JnS => "JnS",
            Opcode::Load => "Load",
            Opcode::Store => "Store",
            Opcode::Add => "Add",
            Opcode::Subt => "Subt",
            Opcode::Input => "Input",
            Opcode::Output => "Output",
            Opcode::Halt => "Halt",
            Opcode::SkipCond => "Skipcond",
            Opcode::Jump => "Jump",
            Opcode::LoadImmi => "LoadImmi",
            Opcode::AddI => "AddI",
            Opcode::JumpI => "JumpI",
            Opcode::LoadI => "LoadI",
            Opcode::StoreI => "StoreI",
        }
    }

    /// Returns `true` if this opcode takes an operand.
    ///
    /// `Input`, `Output` and `Halt` ignore the address field of the instruction word.
    pub const fn takes_operand(self) -> bool {
        !matches!(self, Opcode::Input | Opcode::Output | Opcode::Halt)
    }

    /// Returns a human-readable description of this opcode's effect.
    pub const fn description(self) -> &'static str {
        match self {
            Opcode::JnS => "Store the address of the next instruction at X, then jump to X + 1",
            Opcode::Load => "Load the value at address X into the AC (AC <- M[X])",
            Opcode::Store => "Store the AC into memory at address X (M[X] <- AC)",
            Opcode::Add => "Add the value at address X to the AC (AC <- AC + M[X])",
            Opcode::Subt => "Subtract the value at address X from the AC (AC <- AC - M[X])",
            Opcode::Input => "Read the next value from user input (AC <- IN)",
            Opcode::Output => "Output the value in the AC (OUT <- AC)",
            Opcode::Halt => "Stop execution of the program",
            Opcode::SkipCond => "Skip the next instruction if the condition indicated by X holds",
            Opcode::Jump => "Jump to address X (PC <- X)",
            Opcode::LoadImmi => "Set the AC to the 12-bit unsigned immediate X (AC <- X)",
            Opcode::AddI => "Add the value at the address held at X to the AC (AC <- AC + M[M[X]])",
            Opcode::JumpI => "Jump to the address held at X (PC <- M[X])",
            Opcode::LoadI => "Load the value at the address held at X into the AC (AC <- M[M[X]])",
            Opcode::StoreI => "Store the AC at the address held at X (M[M[X]] <- AC)",
        }
    }
}

impl fmt::Display for Opcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

impl FromStr for Opcode {
    type Err = UnknownMnemonic;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Opcode::from_mnemonic(s).ok_or_else(|| UnknownMnemonic(s.to_owned()))
    }
}

/// The condition tested by a `Skipcond` instruction.
///
/// The condition is selected by bits 11-10 of the address field. All four
/// encodings are valid in MARIE.js, so decoding a `Skipcond` can never fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipCondition {
    /// `Skipcond 000` — skip if `AC < 0`.
    Negative,
    /// `Skipcond 400` — skip if `AC == 0`.
    Zero,
    /// `Skipcond 800` — skip if `AC > 0`.
    Positive,
    /// `Skipcond C00` — skip if `AC != 0`.
    NonZero,
}

impl SkipCondition {
    /// Extracts the condition from a `Skipcond` operand.
    ///
    /// Only bits 11-10 are inspected; the remaining bits are ignored, matching MARIE.js.
    pub const fn from_operand(operand: MemoryAddress) -> Self {
        match operand.value() & 0x0C00 {
            0x0000 => SkipCondition::Negative,
            0x0400 => SkipCondition::Zero,
            0x0800 => SkipCondition::Positive,
            _ => SkipCondition::NonZero,
        }
    }

    /// Returns the canonical operand encoding for this condition.
    pub const fn to_operand(self) -> MemoryAddress {
        MemoryAddress::new(match self {
            SkipCondition::Negative => 0x000,
            SkipCondition::Zero => 0x400,
            SkipCondition::Positive => 0x800,
            SkipCondition::NonZero => 0xC00,
        })
    }

    /// Returns `true` if this condition holds for the given accumulator value.
    pub const fn holds(self, ac: Value) -> bool {
        match self {
            SkipCondition::Negative => ac.is_negative(),
            SkipCondition::Zero => ac.is_zero(),
            SkipCondition::Positive => ac.is_positive(),
            SkipCondition::NonZero => !ac.is_zero(),
        }
    }
}

impl fmt::Display for SkipCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            SkipCondition::Negative => "AC < 0",
            SkipCondition::Zero => "AC = 0",
            SkipCondition::Positive => "AC > 0",
            SkipCondition::NonZero => "AC != 0",
        };
        f.write_str(text)
    }
}

/// A decoded MARIE instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instruction {
    opcode: Opcode,
    operand: MemoryAddress,
}

impl Instruction {
    /// Builds an instruction from an opcode and operand.
    pub const fn new(opcode: Opcode, operand: MemoryAddress) -> Self {
        Self { opcode, operand }
    }

    /// Decodes an instruction word.
    ///
    /// Returns `None` if bits 15-12 hold the unassigned opcode `0xF`.
    pub const fn decode(word: Value) -> Option<Self> {
        let bits = word.to_bits();
        match Opcode::from_nibble((bits >> 12) as u8) {
            Some(opcode) => Some(Self {
                opcode,
                operand: MemoryAddress::new_masked(bits),
            }),
            None => None,
        }
    }

    /// Encodes this instruction back into a machine word.
    pub const fn encode(self) -> Value {
        Value::from_bits(((self.opcode.to_nibble() as u16) << 12) | self.operand.value())
    }

    /// Returns the opcode.
    pub const fn opcode(self) -> Opcode {
        self.opcode
    }

    /// Returns the 12-bit address field.
    ///
    /// For `Input`, `Output` and `Halt` this field is ignored during execution.
    pub const fn operand(self) -> MemoryAddress {
        self.operand
    }

    /// Returns the condition tested by this instruction, if it is a `Skipcond`.
    pub const fn skip_condition(self) -> Option<SkipCondition> {
        match self.opcode {
            Opcode::SkipCond => Some(SkipCondition::from_operand(self.operand)),
            _ => None,
        }
    }
}

impl fmt::Display for Instruction {
    /// Formats the instruction as assembly, with a hexadecimal operand.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.opcode.takes_operand() {
            write!(f, "{} {}", self.opcode, self.operand)
        } else {
            fmt::Display::fmt(&self.opcode, f)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_encode_round_trips_for_every_opcode() {
        for opcode in Opcode::ALL {
            let instruction = Instruction::new(opcode, MemoryAddress::new(0x123));
            let word = instruction.encode();
            assert_eq!(word.to_bits() >> 12, opcode.to_nibble() as u16);
            assert_eq!(Instruction::decode(word), Some(instruction));
        }
    }

    #[test]
    fn opcode_f_is_unassigned() {
        assert_eq!(Opcode::from_nibble(0xF), None);
        assert_eq!(Instruction::decode(Value::from_bits(0xF000)), None);
    }

    #[test]
    fn skipcond_decodes_all_four_encodings() {
        let cases = [
            (0x000, SkipCondition::Negative),
            (0x400, SkipCondition::Zero),
            (0x800, SkipCondition::Positive),
            (0xC00, SkipCondition::NonZero),
        ];
        for (operand, expected) in cases {
            let word = Value::from_bits(0x8000 | operand);
            let instruction = Instruction::decode(word).unwrap();
            assert_eq!(instruction.skip_condition(), Some(expected));
            assert_eq!(expected.to_operand().value(), operand);
        }
        // Low bits of the operand are ignored.
        assert_eq!(
            SkipCondition::from_operand(MemoryAddress::new(0x4FF)),
            SkipCondition::Zero
        );
    }

    #[test]
    fn skip_conditions_match_signed_accumulator_semantics() {
        assert!(SkipCondition::Negative.holds(Value::from_bits(0x8000)));
        assert!(!SkipCondition::Negative.holds(Value::ZERO));
        assert!(SkipCondition::Zero.holds(Value::ZERO));
        assert!(SkipCondition::Positive.holds(Value::new(1)));
        assert!(!SkipCondition::Positive.holds(Value::new(-1)));
        assert!(SkipCondition::NonZero.holds(Value::new(-1)));
        assert!(!SkipCondition::NonZero.holds(Value::ZERO));
    }

    #[test]
    fn mnemonics_round_trip_case_insensitively() {
        for opcode in Opcode::ALL {
            let mnemonic = opcode.mnemonic();
            assert_eq!(Opcode::from_mnemonic(mnemonic), Some(opcode));
            assert_eq!(
                Opcode::from_mnemonic(&mnemonic.to_ascii_uppercase()),
                Some(opcode)
            );
            assert_eq!(mnemonic.parse::<Opcode>(), Ok(opcode));
        }
        // Aliases are directives, not opcodes.
        assert_eq!(Opcode::from_mnemonic("Clear"), None);
        assert_eq!(Opcode::from_mnemonic("Adr"), None);
        assert_eq!(Opcode::from_mnemonic("nope"), None);
    }

    #[test]
    fn operandless_opcodes_are_the_expected_three() {
        let without: Vec<_> = Opcode::ALL
            .into_iter()
            .filter(|opcode| !opcode.takes_operand())
            .collect();
        assert_eq!(without, vec![Opcode::Input, Opcode::Output, Opcode::Halt]);
    }

    #[test]
    fn display_omits_operand_for_operandless_opcodes() {
        let halt = Instruction::new(Opcode::Halt, MemoryAddress::new(0x123));
        assert_eq!(halt.to_string(), "Halt");
        let load = Instruction::new(Opcode::Load, MemoryAddress::new(0x00A));
        assert_eq!(load.to_string(), "Load 00A");
    }
}
