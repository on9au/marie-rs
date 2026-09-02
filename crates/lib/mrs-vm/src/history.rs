//! The undo journal that backs stepping backwards.
//!
//! Each executed [`MicroOp`] has at most one effect on the machine, so recording the
//! value it overwrote is enough to reverse it. Entries also carry the control state
//! from before the operation, which restores the micro-program counter and the
//! decoded instruction without needing a second kind of record.
//!
//! Recording is off by default: a journal is only useful to a debugger, and a
//! free-running program would otherwise grow one forever. Turn it on with
//! [`MarieVM::set_history_limit`](crate::MarieVM::set_history_limit).

use std::collections::VecDeque;
use std::fmt;

use mrs_core::{Instruction, MemoryAddress};

use crate::microcode::MicroOp;
use crate::registers::Register;

/// The change one micro-operation made to the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// The operation changed nothing observable.
    None,
    /// A register was overwritten.
    Register {
        /// The register that was written.
        register: Register,
        /// Its value beforehand.
        previous: u16,
    },
    /// A memory word was overwritten.
    Memory {
        /// The address that was written.
        address: MemoryAddress,
        /// Its contents beforehand.
        previous: i16,
    },
    /// A value was consumed from the input device and latched into `IN`.
    InputRead {
        /// The previous contents of `IN`.
        previous: u16,
        /// The value taken from the device.
        value: i16,
    },
    /// A value was written to the output device.
    OutputWritten {
        /// The value written.
        value: i16,
    },
}

impl Effect {
    /// Returns `true` if reversing this effect requires the I/O device's cooperation.
    pub const fn touches_io(self) -> bool {
        matches!(
            self,
            Effect::InputRead { .. } | Effect::OutputWritten { .. }
        )
    }
}

/// Control-unit state, snapshotted before each micro-operation.
///
/// These fields are small and interdependent, so they are saved wholesale rather than
/// as deltas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Control {
    pub(crate) micro_pc: u8,
    pub(crate) instruction_address: MemoryAddress,
    pub(crate) decoded: Option<Instruction>,
    pub(crate) comparison: bool,
}

/// One reversible micro-operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub(crate) control: Control,
    micro_op: MicroOp,
    effect: Effect,
}

impl Entry {
    pub(crate) const fn new(control: Control, micro_op: MicroOp, effect: Effect) -> Self {
        Self {
            control,
            micro_op,
            effect,
        }
    }

    /// The operation that was executed.
    pub const fn micro_op(&self) -> MicroOp {
        self.micro_op
    }

    /// What it changed.
    pub const fn effect(&self) -> Effect {
        self.effect
    }

    /// The address of the instruction this operation belonged to.
    pub const fn instruction_address(&self) -> MemoryAddress {
        self.control.instruction_address
    }

    /// Returns `true` if this operation began a new instruction.
    pub const fn starts_instruction(&self) -> bool {
        self.control.micro_pc == 0
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.control.instruction_address, self.micro_op)
    }
}

/// A bounded journal of executed micro-operations, most recent last.
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: VecDeque<Entry>,
    limit: usize,
}

impl History {
    /// Creates a journal that retains at most `limit` operations.
    ///
    /// A limit of zero disables recording entirely.
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            limit,
        }
    }

    /// Returns how many operations the journal retains.
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Sets how many operations the journal retains, discarding the oldest if the new
    /// limit is smaller. A limit of zero disables recording and clears the journal.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        while self.entries.len() > limit {
            self.entries.pop_front();
        }
        if limit == 0 {
            // Release the buffer rather than hold an empty allocation forever.
            self.entries = VecDeque::new();
        }
    }

    /// Returns `true` if operations are being recorded.
    pub const fn is_enabled(&self) -> bool {
        self.limit > 0
    }

    /// Returns the number of operations that can currently be undone.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if nothing can be undone.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Discards every recorded operation.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Iterates over the recorded operations, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Entry> + ExactSizeIterator {
        self.entries.iter()
    }

    /// Returns the most recently recorded operation.
    pub fn last(&self) -> Option<&Entry> {
        self.entries.back()
    }

    /// Records an operation, evicting the oldest if the journal is full.
    pub(crate) fn record(&mut self, entry: Entry) {
        if self.limit == 0 {
            return;
        }
        if self.entries.len() == self.limit {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Removes and returns the most recent operation.
    pub(crate) fn pop(&mut self) -> Option<Entry> {
        self.entries.pop_back()
    }
}

/// Why the machine could not be stepped backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepBackError {
    /// There is nothing recorded to undo.
    ///
    /// Either the journal is disabled, the machine has not executed anything since it
    /// was enabled, or the operation has been evicted by the journal's limit.
    NoHistory,
    /// The operation consumed a value the input device cannot give back.
    IrreversibleInput,
    /// The operation emitted output the device cannot retract.
    IrreversibleOutput,
}

impl fmt::Display for StepBackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepBackError::NoHistory => f.write_str("no recorded operation to undo"),
            StepBackError::IrreversibleInput => {
                f.write_str("the input device cannot un-read a consumed value")
            }
            StepBackError::IrreversibleOutput => {
                f.write_str("the output device cannot retract a written value")
            }
        }
    }
}

impl std::error::Error for StepBackError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(micro_pc: u8) -> Entry {
        Entry::new(
            Control {
                micro_pc,
                instruction_address: MemoryAddress::ZERO,
                decoded: None,
                comparison: false,
            },
            MicroOp::IncrementPc,
            Effect::None,
        )
    }

    #[test]
    fn recording_is_off_until_a_limit_is_set() {
        let mut history = History::default();
        assert!(!history.is_enabled());
        history.record(entry(0));
        assert!(history.is_empty());
    }

    #[test]
    fn the_oldest_entries_are_evicted_when_full() {
        let mut history = History::new(2);
        for micro_pc in 0..4 {
            history.record(entry(micro_pc));
        }
        assert_eq!(history.len(), 2);
        let retained: Vec<_> = history.iter().map(|e| e.control.micro_pc).collect();
        assert_eq!(retained, vec![2, 3]);
    }

    #[test]
    fn shrinking_the_limit_drops_the_oldest() {
        let mut history = History::new(4);
        for micro_pc in 0..4 {
            history.record(entry(micro_pc));
        }
        history.set_limit(2);
        let retained: Vec<_> = history.iter().map(|e| e.control.micro_pc).collect();
        assert_eq!(retained, vec![2, 3]);

        history.set_limit(0);
        assert!(history.is_empty());
        assert!(!history.is_enabled());
    }
}
