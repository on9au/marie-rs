//! The label symbol table.
//!
//! Definitions come from the parser, references from the assembler. Both are recorded
//! with spans, which is what turns this into the index behind go-to-definition,
//! find-all-references, rename and an unused-label lint.
//!
//! Labels are **case-sensitive**, matching MARIE.js, which stores them in a plain
//! object keyed by the label as written. Mnemonics, by contrast, are case-insensitive.
//! Getting this backwards is a silent source of incompatibility, so the two are kept
//! deliberately far apart: mnemonic folding happens in the parser, and nothing here
//! ever changes a label's case.

use std::collections::BTreeMap;

use mrs_core::MemoryAddress;

use crate::span::Span;

/// Whether an occurrence of a label defines it or uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// The `Foo,` that introduces the label.
    Definition,
    /// An operand naming the label.
    Reference,
}

/// One label, with everywhere it appears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The label as written.
    pub name: String,
    /// The address it resolves to.
    pub address: MemoryAddress,
    /// Where it was defined.
    pub definition: Span,
    /// The zero-based line it was defined on.
    pub line: u32,
    /// Every operand that names it, in source order.
    pub references: Vec<Span>,
}

impl Symbol {
    /// Returns `true` if nothing refers to this label.
    pub fn is_unused(&self) -> bool {
        self.references.is_empty()
    }
}

/// Every label in a source file.
///
/// Iteration order is by name, so diagnostics derived from the table are deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SymbolTable {
    symbols: BTreeMap<String, Symbol>,
}

impl SymbolTable {
    /// Creates an empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a definition.
    ///
    /// Returns `Err` with the existing symbol if the label is already defined, leaving
    /// the table unchanged — the caller turns that into a duplicate-label diagnostic
    /// that can point at the first definition.
    pub fn define(
        &mut self,
        name: &str,
        address: MemoryAddress,
        definition: Span,
        line: u32,
    ) -> Result<(), &Symbol> {
        if self.symbols.contains_key(name) {
            return Err(&self.symbols[name]);
        }
        self.symbols.insert(
            name.to_owned(),
            Symbol {
                name: name.to_owned(),
                address,
                definition,
                line,
                references: Vec::new(),
            },
        );
        Ok(())
    }

    /// Records a use of `name`, returning its address.
    ///
    /// Returns `None` for an undefined label, in which case nothing is recorded.
    pub fn reference(&mut self, name: &str, span: Span) -> Option<MemoryAddress> {
        let symbol = self.symbols.get_mut(name)?;
        symbol.references.push(span);
        Some(symbol.address)
    }

    /// Looks a label up by name.
    pub fn get(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(name)
    }

    /// Returns `true` if the label is defined.
    pub fn contains(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
    }

    /// Returns the address of a label.
    pub fn address_of(&self, name: &str) -> Option<MemoryAddress> {
        self.get(name).map(|symbol| symbol.address)
    }

    /// Returns the number of labels.
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Returns `true` if no labels are defined.
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// Iterates over the labels, ordered by name.
    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }

    /// Returns the label occurring at `offset`, and whether that occurrence is its
    /// definition or a reference.
    ///
    /// This is what go-to-definition calls: resolve the caret to a symbol, then jump to
    /// [`Symbol::definition`].
    pub fn find_at(&self, offset: u32) -> Option<(&Symbol, Role)> {
        self.symbols.values().find_map(|symbol| {
            if symbol.definition.touches(offset) {
                Some((symbol, Role::Definition))
            } else if symbol.references.iter().any(|span| span.touches(offset)) {
                Some((symbol, Role::Reference))
            } else {
                None
            }
        })
    }

    /// Returns every span that a rename of the symbol at `offset` would have to touch,
    /// definition first, then references in source order.
    pub fn rename_spans(&self, offset: u32) -> Option<(&Symbol, Vec<Span>)> {
        let (symbol, _) = self.find_at(offset)?;
        let mut spans = vec![symbol.definition];
        spans.extend(symbol.references.iter().copied());
        Some((symbol, spans))
    }

    /// Iterates over labels nothing refers to.
    pub fn unused(&self) -> impl Iterator<Item = &Symbol> {
        self.iter().filter(|symbol| symbol.is_unused())
    }
}
