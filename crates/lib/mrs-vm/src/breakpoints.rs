//! Breakpoint tracking for the MARIE VM debugger.

use std::fmt;

use crate::memory::{MEMORY_WORD_COUNT, MemoryAddress};

const WORD_BITS: usize = u64::BITS as usize;
const WORDS: usize = MEMORY_WORD_COUNT as usize / WORD_BITS;

// The bitset stores exactly one bit per addressable word, with nothing left over.
const _: () = assert!((MEMORY_WORD_COUNT as usize).is_multiple_of(WORD_BITS));

/// The set of addresses the VM should suspend at before executing.
///
/// This is a fixed-size bitset covering the whole address space, so membership tests
/// on the hot path are a shift and a mask, and no allocation is involved.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BreakpointSet {
    bits: [u64; WORDS],
}

impl BreakpointSet {
    /// Creates an empty set.
    pub const fn new() -> Self {
        Self { bits: [0; WORDS] }
    }

    const fn position(address: MemoryAddress) -> (usize, u64) {
        let index = address.value() as usize;
        (index / WORD_BITS, 1u64 << (index % WORD_BITS))
    }

    /// Adds a breakpoint. Returns `true` if it was not already present.
    pub fn insert(&mut self, address: MemoryAddress) -> bool {
        let (word, mask) = Self::position(address);
        let inserted = self.bits[word] & mask == 0;
        self.bits[word] |= mask;
        inserted
    }

    /// Removes a breakpoint. Returns `true` if it was present.
    pub fn remove(&mut self, address: MemoryAddress) -> bool {
        let (word, mask) = Self::position(address);
        let removed = self.bits[word] & mask != 0;
        self.bits[word] &= !mask;
        removed
    }

    /// Returns `true` if there is a breakpoint at `address`.
    pub fn contains(&self, address: MemoryAddress) -> bool {
        let (word, mask) = Self::position(address);
        self.bits[word] & mask != 0
    }

    /// Removes every breakpoint.
    pub fn clear(&mut self) {
        self.bits = [0; WORDS];
    }

    /// Returns `true` if no breakpoints are set.
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|word| *word == 0)
    }

    /// Returns the number of breakpoints set.
    pub fn len(&self) -> usize {
        self.bits
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// Iterates over the addresses in the set, in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = MemoryAddress> + '_ {
        self.bits.iter().enumerate().flat_map(|(word, bits)| {
            let base = word * WORD_BITS;
            (0..WORD_BITS)
                .filter(move |bit| bits & (1u64 << bit) != 0)
                .map(move |bit| MemoryAddress::new((base + bit) as u16))
        })
    }
}

impl Default for BreakpointSet {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<MemoryAddress> for BreakpointSet {
    fn from_iter<I: IntoIterator<Item = MemoryAddress>>(iter: I) -> Self {
        let mut set = Self::new();
        for address in iter {
            set.insert(address);
        }
        set
    }
}

impl fmt::Debug for BreakpointSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_remove_and_membership() {
        let mut set = BreakpointSet::new();
        assert!(set.is_empty());

        assert!(set.insert(MemoryAddress::new(0)));
        assert!(!set.insert(MemoryAddress::new(0)));
        assert!(set.insert(MemoryAddress::MAX));
        assert!(set.contains(MemoryAddress::new(0)));
        assert!(set.contains(MemoryAddress::MAX));
        assert!(!set.contains(MemoryAddress::new(1)));
        assert_eq!(set.len(), 2);

        assert!(set.remove(MemoryAddress::MAX));
        assert!(!set.remove(MemoryAddress::MAX));
        assert_eq!(set.len(), 1);

        set.clear();
        assert!(set.is_empty());
    }

    #[test]
    fn iterates_in_ascending_order() {
        let addresses = [0x000, 0x03F, 0x040, 0x123, 0xFFF].map(MemoryAddress::new);
        let set: BreakpointSet = addresses.into_iter().collect();
        assert_eq!(set.iter().collect::<Vec<_>>(), addresses);
    }
}
