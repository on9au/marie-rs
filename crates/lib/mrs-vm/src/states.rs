//! MARIE VM states

use std::fmt;

use mrs_core::{MemoryAddress, Opcode, Value};

use crate::MarieVM;
use crate::io::IoError;
use crate::microcode::MicroOp;

mod sealed {
    pub trait Sealed {}
}

/// Marker trait implemented by the type-state markers of [`MarieVM`].
///
/// Sealed: the set of states is fixed by this crate.
pub trait MarieVmState: sealed::Sealed {}

/// The VM has been created and can be programmed, but is not executing.
pub enum Ready {}
impl sealed::Sealed for Ready {}
impl MarieVmState for Ready {}

/// The VM is executing freely until it halts, faults, or suspends.
pub enum Running {}
impl sealed::Sealed for Running {}
impl MarieVmState for Running {}

/// The VM is under debugger control and advances one instruction at a time.
pub enum Stepping {}
impl sealed::Sealed for Stepping {}
impl MarieVmState for Stepping {}

/// The VM has stopped and cannot execute further without being reset.
pub enum Terminated {}
impl sealed::Sealed for Terminated {}
impl MarieVmState for Terminated {}

/// The result of running the VM.
#[must_use = "the outcome owns the VM; dropping it discards the machine"]
pub enum RunOutcome<IO> {
    /// The program executed a `Halt`.
    Terminated(MarieVM<IO, Terminated>),
    /// Execution stopped early but can be continued by calling `run` again.
    Suspended(MarieVM<IO, Running>, SuspendReason),
    /// Execution stopped because of an unrecoverable error.
    Faulted(MarieVM<IO, Terminated>, Fault),
}

/// The result of stepping the VM by a single instruction.
#[must_use = "the outcome owns the VM; dropping it discards the machine"]
pub enum StepOutcome<IO> {
    /// One instruction was executed.
    Stepped(MarieVM<IO, Stepping>),
    /// The instruction was an `Input` and the device is not ready.
    ///
    /// The machine is suspended part-way through the instruction, at the
    /// micro-operation that reads the device; stepping again re-polls it.
    AwaitingInput(MarieVM<IO, Stepping>),
    /// The instruction was a `Halt`.
    Terminated(MarieVM<IO, Terminated>),
    /// The instruction could not be executed.
    Faulted(MarieVM<IO, Terminated>, Fault),
}

/// The result of advancing the VM by a single micro-operation.
#[must_use = "the outcome owns the VM; dropping it discards the machine"]
pub enum MicroStepOutcome<IO> {
    /// One micro-operation was executed.
    ///
    /// Use [`MarieVM::at_instruction_boundary`](crate::MarieVM::at_instruction_boundary)
    /// to tell whether this completed an instruction.
    Stepped(MarieVM<IO, Stepping>, MicroOp),
    /// The operation reads the input device, which is not ready.
    ///
    /// The micro-program counter has not advanced, so stepping again re-polls it.
    AwaitingInput(MarieVM<IO, Stepping>),
    /// The operation was the `Halt` of a `Halt` instruction.
    Terminated(MarieVM<IO, Terminated>),
    /// The operation could not be executed.
    Faulted(MarieVM<IO, Terminated>, Fault),
}

/// Why a [`RunOutcome::Suspended`] stopped the VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendReason {
    /// A breakpoint was reached. The instruction at this address has *not* been executed.
    Breakpoint(MemoryAddress),
    /// An `Input` instruction found the device not ready.
    ///
    /// The machine is suspended part-way through the instruction, at the
    /// micro-operation that reads the device; running again re-polls it.
    AwaitingInput,
    /// The step budget passed to [`MarieVM::run_bounded`] was exhausted.
    ///
    /// [`MarieVM::run_bounded`]: crate::MarieVM::run_bounded
    StepLimit,
}

impl fmt::Display for SuspendReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SuspendReason::Breakpoint(address) => write!(f, "breakpoint at {address}"),
            SuspendReason::AwaitingInput => f.write_str("waiting for input"),
            SuspendReason::StepLimit => f.write_str("step limit reached"),
        }
    }
}

/// An unrecoverable error that stops the VM.
#[derive(Debug)]
pub enum Fault {
    /// The fetched word did not decode to a valid instruction.
    ///
    /// Only opcode `0xF` is unassigned.
    InvalidOpcode {
        /// The address the offending word was fetched from.
        address: MemoryAddress,
        /// The offending word.
        word: Value,
    },
    /// An I/O device returned an error.
    Io {
        /// The address of the instruction that performed the I/O.
        address: MemoryAddress,
        /// The opcode that performed the I/O: [`Opcode::Input`] or [`Opcode::Output`].
        opcode: Opcode,
        /// The underlying device error.
        error: IoError,
    },
}

impl Fault {
    /// The address of the instruction that faulted.
    pub fn address(&self) -> MemoryAddress {
        match self {
            Fault::InvalidOpcode { address, .. } | Fault::Io { address, .. } => *address,
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::InvalidOpcode { address, word } => write!(
                f,
                "invalid opcode 0x{:X} in word 0x{word:04X} at address {address}",
                word.to_bits() >> 12,
            ),
            Fault::Io {
                address,
                opcode,
                error,
            } => write!(f, "{opcode} at address {address} failed: {error}"),
        }
    }
}

impl std::error::Error for Fault {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Fault::Io { error, .. } => Some(error),
            Fault::InvalidOpcode { .. } => None,
        }
    }
}
