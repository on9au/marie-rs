//! The mapping between memory addresses and source lines.
//!
//! A debugger needs both directions: given the program counter, highlight a line;
//! given a line the user clicked in the gutter, work out where to set a breakpoint.
//! MARIE.js keeps only the address-to-line direction, so the reverse index here is an
//! addition rather than a compatibility concern.

use std::collections::BTreeMap;

use mrs_core::MemoryAddress;

/// A two-way index between addresses and zero-based source lines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceMap {
    by_address: BTreeMap<u16, u32>,
    by_line: BTreeMap<u32, u16>,
}

impl SourceMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `address` was assembled from `line`.
    pub fn insert(&mut self, address: MemoryAddress, line: u32) {
        self.by_address.insert(address.value(), line);
        self.by_line.insert(line, address.value());
    }

    /// Returns the zero-based line that produced `address`.
    pub fn line_for(&self, address: MemoryAddress) -> Option<u32> {
        self.by_address.get(&address.value()).copied()
    }

    /// Returns the address assembled from `line`.
    pub fn address_for(&self, line: u32) -> Option<MemoryAddress> {
        self.by_line.get(&line).copied().map(MemoryAddress::new)
    }

    /// Returns the address of the first mapped line at or after `line`.
    ///
    /// This is what a debugger wants when the user sets a breakpoint on a blank or
    /// comment line: slide down to the next line that actually produced a word.
    pub fn address_at_or_after(&self, line: u32) -> Option<MemoryAddress> {
        self.by_line
            .range(line..)
            .next()
            .map(|(_, address)| MemoryAddress::new(*address))
    }

    /// Returns the number of mapped addresses.
    pub fn len(&self) -> usize {
        self.by_address.len()
    }

    /// Returns `true` if nothing is mapped.
    pub fn is_empty(&self) -> bool {
        self.by_address.is_empty()
    }

    /// Iterates over `(address, line)` pairs in address order.
    pub fn iter(&self) -> impl Iterator<Item = (MemoryAddress, u32)> + '_ {
        self.by_address
            .iter()
            .map(|(address, line)| (MemoryAddress::new(*address), *line))
    }
}
