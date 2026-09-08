//! A control-flow graph over the assembled words.
//!
//! The assembler can only see one line at a time, so the mistakes it cannot catch are
//! the ones that span the whole program: execution running off the end, falling out of
//! code and into a `DEC` that then decodes as some unrelated instruction, or a
//! subroutine whose return slot holds code that the call is about to overwrite. All of
//! those need to know where control can actually go, which is what this builds.
//!
//! # What it is honest about
//!
//! MARIE can jump indirectly (`JumpI`) and can rewrite its own instructions (`StoreI`,
//! or a `Store` aimed at code), so a purely static graph is an *under*-approximation of
//! where control may go. That asymmetry decides which lints are safe to report:
//!
//! - "this word **is** reachable" is found by following a concrete path, so extra edges
//!   the graph missed cannot make it wrong. Lints built on it do not produce false
//!   positives from indirect flow.
//! - "this word is **not** reachable" is only sound when there are no indirect
//!   transfers at all, so [`Cfg::exact`] reports whether that held, and the
//!   unreachable-code lint stays quiet when it did not.

use mrs_asm::Assembly;
use mrs_asm::ast::{Item, MnemonicKind};
use mrs_core::{Directive, MemoryAddress, Opcode};

/// What a word holds, according to the source line that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An instruction.
    Code,
    /// A literal emitted by `DEC`, `OCT`, `HEX` or `ADR`.
    Data,
}

/// Where control can go from a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// To another word of this program, by index.
    Word(usize),
    /// To an address outside the assembled program.
    Outside(MemoryAddress),
    /// Past the last assembled word.
    OffEnd,
    /// Somewhere only known at run time.
    Unknown,
}

/// The control-flow graph of an assembled program.
#[derive(Debug, Clone)]
pub struct Cfg {
    kinds: Vec<Kind>,
    reachable: Vec<bool>,
    exact: bool,
    origin: MemoryAddress,
}

impl Cfg {
    /// Builds the graph for `assembly`.
    ///
    /// Only meaningful for a program that assembled cleanly; a failed item emits a zero
    /// word, and following those would be reading tea leaves.
    pub fn new(assembly: &Assembly) -> Self {
        let items = &assembly.program.items;
        let kinds: Vec<Kind> = items.iter().map(classify).collect();
        // An indirect jump or an indirect store means the graph cannot see every edge.
        let exact = !items
            .iter()
            .any(|item| matches!(item.mnemonic.opcode(), Some(Opcode::JumpI | Opcode::StoreI)));

        let mut cfg = Self {
            reachable: vec![false; items.len()],
            kinds,
            exact,
            origin: assembly.origin,
        };
        cfg.mark_reachable(assembly);
        cfg
    }

    /// Walks the graph from the entry point, marking what it can reach.
    fn mark_reachable(&mut self, assembly: &Assembly) {
        // Execution begins at the origin, which is the first assembled word.
        let mut stack = if self.reachable.is_empty() {
            Vec::new()
        } else {
            vec![0usize]
        };
        while let Some(index) = stack.pop() {
            if self.reachable[index] {
                continue;
            }
            self.reachable[index] = true;
            // Data is reported where it is reached rather than decoded and followed:
            // whatever a `DEC` happens to decode as is noise, not intent.
            if self.kinds[index] == Kind::Data {
                continue;
            }
            for edge in self.edges(assembly, index) {
                if let Edge::Word(next) = edge {
                    stack.push(next);
                }
            }
        }
    }

    /// Returns where control can go from the word at `index`.
    pub fn edges(&self, assembly: &Assembly, index: usize) -> Vec<Edge> {
        let Some(item) = assembly.program.items.get(index) else {
            return Vec::new();
        };
        if self.kinds[index] == Kind::Data {
            return Vec::new();
        }
        let Some(opcode) = effective_opcode(item) else {
            return Vec::new();
        };
        let operand = || {
            assembly
                .words
                .get(index)
                .map(|word| MemoryAddress::new_masked(*word as u16))
        };

        match opcode {
            Opcode::Halt => Vec::new(),
            Opcode::Jump => vec![self.target(operand())],
            // The return address is written to X, and execution resumes at X + 1.
            Opcode::JnS => vec![self.target(operand().map(|a| a.wrapping_add(1)))],
            Opcode::JumpI => vec![Edge::Unknown],
            // A skip either runs the next word or the one after it.
            Opcode::SkipCond => vec![self.step(index, 1), self.step(index, 2)],
            _ => vec![self.step(index, 1)],
        }
    }

    /// The edge reached by advancing `offset` words from `index`.
    fn step(&self, index: usize, offset: usize) -> Edge {
        match index.checked_add(offset) {
            Some(next) if next < self.kinds.len() => Edge::Word(next),
            _ => Edge::OffEnd,
        }
    }

    /// The edge for a transfer to an absolute address.
    fn target(&self, address: Option<MemoryAddress>) -> Edge {
        let Some(address) = address else {
            return Edge::Unknown;
        };
        match self.index_of(address) {
            Some(index) => Edge::Word(index),
            None => Edge::Outside(address),
        }
    }

    /// Converts an address into a word index, if it lies inside the program.
    pub fn index_of(&self, address: MemoryAddress) -> Option<usize> {
        let offset = address.index().checked_sub(self.origin.index())?;
        (offset < self.kinds.len()).then_some(offset)
    }

    /// Returns what the word at `index` holds.
    pub fn kind(&self, index: usize) -> Option<Kind> {
        self.kinds.get(index).copied()
    }

    /// Returns `true` if control can reach the word at `index`.
    pub fn is_reachable(&self, index: usize) -> bool {
        self.reachable.get(index).copied().unwrap_or(false)
    }

    /// Returns `true` if the program has no indirect control transfers, so the graph
    /// sees every edge and "unreachable" can be stated with confidence.
    pub fn exact(&self) -> bool {
        self.exact
    }

    /// The number of words in the graph.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// Returns `true` if the program has no words.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Iterates over the indices of reachable words.
    pub fn reachable_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.reachable
            .iter()
            .enumerate()
            .filter_map(|(index, hit)| hit.then_some(index))
    }
}

/// Classifies a word as code or data from the directive that emitted it.
fn classify(item: &Item) -> Kind {
    match item.mnemonic.kind {
        MnemonicKind::Directive(
            Directive::Dec | Directive::Oct | Directive::Hex | Directive::Adr,
        ) => Kind::Data,
        _ => Kind::Code,
    }
}

/// The opcode a word executes as, resolving the `Clear` alias.
pub fn effective_opcode(item: &Item) -> Option<Opcode> {
    match item.mnemonic.kind {
        MnemonicKind::Opcode(opcode) => Some(opcode),
        MnemonicKind::Directive(Directive::Clear) => Some(Opcode::LoadImmi),
        _ => None,
    }
}
