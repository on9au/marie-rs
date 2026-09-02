//! The micro-programs executed by the control unit.
//!
//! Every MARIE instruction is a fixed sequence of register-transfer-level operations.
//! Spelling them out as data rather than as straight-line code buys three things:
//! the debugger can single-step *within* an instruction, each operation has a single
//! well-defined effect that [`crate::history`] can reverse, and the sequences can be
//! read side by side with the MARIE.js microcode they mirror.

use std::fmt;

use mrs_core::Opcode;

use crate::registers::Register;

/// A single register-transfer-level operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MicroOp {
    /// `target <- source`.
    Transfer {
        /// The register written.
        target: Register,
        /// The register read.
        source: Register,
    },
    /// `MBR <- M[MAR]`.
    ReadMemory,
    /// `M[MAR] <- MBR`.
    WriteMemory,
    /// `PC <- PC + 1`.
    IncrementPc,
    /// Selects the micro-program for the opcode held in `IR`.
    ///
    /// Faults if the opcode is unassigned.
    Decode,
    /// `AC <- AC + MBR`.
    Add,
    /// `AC <- AC - MBR`.
    Subtract,
    /// `AC <- IR & 0xFFF`, zero-extended.
    LoadImmediate,
    /// Evaluates the `Skipcond` condition selected by `IR` into the comparison latch.
    Compare,
    /// `PC <- PC + 1` if the comparison latch is set.
    SkipIfComparison,
    /// `IN <- device`. Stalls if the device is not ready.
    ReadInput,
    /// `device <- OUT`.
    WriteOutput,
    /// Stops the machine.
    Halt,
}

impl fmt::Display for MicroOp {
    /// Formats the operation in register-transfer notation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MicroOp::Transfer { target, source } => write!(f, "{target} <- {source}"),
            MicroOp::ReadMemory => f.write_str("MBR <- M[MAR]"),
            MicroOp::WriteMemory => f.write_str("M[MAR] <- MBR"),
            MicroOp::IncrementPc => f.write_str("PC <- PC + 1"),
            MicroOp::Decode => f.write_str("decode IR"),
            MicroOp::Add => f.write_str("AC <- AC + MBR"),
            MicroOp::Subtract => f.write_str("AC <- AC - MBR"),
            MicroOp::LoadImmediate => f.write_str("AC <- IR[11-0]"),
            MicroOp::Compare => f.write_str("compare AC"),
            MicroOp::SkipIfComparison => f.write_str("PC <- PC + 1 if comparison"),
            MicroOp::ReadInput => f.write_str("IN <- input"),
            MicroOp::WriteOutput => f.write_str("output <- OUT"),
            MicroOp::Halt => f.write_str("halt"),
        }
    }
}

/// Shorthand for a register transfer.
const fn transfer(target: Register, source: Register) -> MicroOp {
    MicroOp::Transfer { target, source }
}

use Register::{Ac, In, Ir, Mar, Mbr, Out, Pc};

/// The fetch and decode phase, run before every instruction.
///
/// `MAR <- PC; MBR <- M[MAR]; IR <- MBR; PC <- PC + 1; decode`.
pub const FETCH_DECODE: &[MicroOp] = &[
    transfer(Mar, Pc),
    MicroOp::ReadMemory,
    transfer(Ir, Mbr),
    MicroOp::IncrementPc,
    MicroOp::Decode,
];

/// `M[X] <- PC; PC <- X + 1`. Note that the accumulator is left alone.
const JNS: &[MicroOp] = &[
    transfer(Mar, Ir),
    transfer(Mbr, Pc),
    MicroOp::WriteMemory,
    transfer(Pc, Mar),
    MicroOp::IncrementPc,
];

/// `AC <- M[X]`.
const LOAD: &[MicroOp] = &[transfer(Mar, Ir), MicroOp::ReadMemory, transfer(Ac, Mbr)];

/// `M[X] <- AC`.
const STORE: &[MicroOp] = &[transfer(Mar, Ir), transfer(Mbr, Ac), MicroOp::WriteMemory];

/// `AC <- AC + M[X]`.
const ADD: &[MicroOp] = &[transfer(Mar, Ir), MicroOp::ReadMemory, MicroOp::Add];

/// `AC <- AC - M[X]`.
const SUBT: &[MicroOp] = &[transfer(Mar, Ir), MicroOp::ReadMemory, MicroOp::Subtract];

/// `IN <- device; AC <- IN`.
const INPUT: &[MicroOp] = &[MicroOp::ReadInput, transfer(Ac, In)];

/// `OUT <- AC; device <- OUT`.
const OUTPUT: &[MicroOp] = &[transfer(Out, Ac), MicroOp::WriteOutput];

/// Stops the machine.
const HALT: &[MicroOp] = &[MicroOp::Halt];

/// Tests the condition selected by `IR`, then skips a word if it holds.
const SKIPCOND: &[MicroOp] = &[MicroOp::Compare, MicroOp::SkipIfComparison];

/// `PC <- X`.
const JUMP: &[MicroOp] = &[transfer(Pc, Ir)];

/// `AC <- X`, zero-extended from 12 bits.
const LOAD_IMMI: &[MicroOp] = &[MicroOp::LoadImmediate];

/// `AC <- AC + M[M[X]]`.
const ADD_I: &[MicroOp] = &[
    transfer(Mar, Ir),
    MicroOp::ReadMemory,
    transfer(Mar, Mbr),
    MicroOp::ReadMemory,
    MicroOp::Add,
];

/// `PC <- M[X]`.
const JUMP_I: &[MicroOp] = &[transfer(Mar, Ir), MicroOp::ReadMemory, transfer(Pc, Mbr)];

/// `AC <- M[M[X]]`.
const LOAD_I: &[MicroOp] = &[
    transfer(Mar, Ir),
    MicroOp::ReadMemory,
    transfer(Mar, Mbr),
    MicroOp::ReadMemory,
    transfer(Ac, Mbr),
];

/// `M[M[X]] <- AC`.
const STORE_I: &[MicroOp] = &[
    transfer(Mar, Ir),
    MicroOp::ReadMemory,
    transfer(Mar, Mbr),
    transfer(Mbr, Ac),
    MicroOp::WriteMemory,
];

/// Returns the execute phase for an opcode.
///
/// `MAR <- IR` relies on writes to `MAR` being masked to 12 bits, which is how the
/// operand is extracted from the instruction word.
pub const fn execute(opcode: Opcode) -> &'static [MicroOp] {
    match opcode {
        Opcode::JnS => JNS,
        Opcode::Load => LOAD,
        Opcode::Store => STORE,
        Opcode::Add => ADD,
        Opcode::Subt => SUBT,
        Opcode::Input => INPUT,
        Opcode::Output => OUTPUT,
        Opcode::Halt => HALT,
        Opcode::SkipCond => SKIPCOND,
        Opcode::Jump => JUMP,
        Opcode::LoadImmi => LOAD_IMMI,
        Opcode::AddI => ADD_I,
        Opcode::JumpI => JUMP_I,
        Opcode::LoadI => LOAD_I,
        Opcode::StoreI => STORE_I,
    }
}

/// The total number of micro-operations in one full cycle of `opcode`.
pub const fn cycle_length(opcode: Opcode) -> usize {
    FETCH_DECODE.len() + execute(opcode).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_execute_phase_is_non_empty_and_fits_a_u8_program_counter() {
        for opcode in Opcode::ALL {
            let program = execute(opcode);
            assert!(!program.is_empty(), "{opcode} has no micro-program");
            assert!(cycle_length(opcode) <= u8::MAX as usize);
        }
    }

    #[test]
    fn only_the_terminating_ops_appear_where_expected() {
        // `Halt` belongs to exactly one instruction, and `Decode` only to the fetch.
        for opcode in Opcode::ALL {
            let has_halt = execute(opcode).contains(&MicroOp::Halt);
            assert_eq!(has_halt, opcode == Opcode::Halt, "{opcode}");
            assert!(!execute(opcode).contains(&MicroOp::Decode), "{opcode}");
        }
        assert_eq!(
            FETCH_DECODE.last(),
            Some(&MicroOp::Decode),
            "decode must be the last fetch step"
        );
    }

    #[test]
    fn micro_ops_render_as_register_transfers() {
        assert_eq!(transfer(Mar, Pc).to_string(), "MAR <- PC");
        assert_eq!(MicroOp::ReadMemory.to_string(), "MBR <- M[MAR]");
    }
}
