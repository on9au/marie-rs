//! The parsed form of a MARIE source file.
//!
//! The tree is deliberately lossless about position and generous about brokenness: an
//! [`Item`] is produced even when its mnemonic is unknown or its operand is missing, so
//! a language server can still offer hover, completion and go-to-definition on a file
//! that does not assemble. Semantic checking happens later, in
//! [`crate::assembler`].

use mrs_core::{Directive, MemoryAddress, Opcode};

use crate::span::Span;

/// A label definition, without its trailing comma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The label as written. Labels are case-sensitive, so this preserves case.
    pub name: String,
    /// Where the label was written.
    pub span: Span,
}

/// What a mnemonic turned out to name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MnemonicKind {
    /// A CPU instruction.
    Opcode(Opcode),
    /// An assembler directive.
    Directive(Directive),
    /// Neither. Reported as an error, but kept in the tree so tooling can still see it.
    Unknown,
}

/// The operation named by a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mnemonic {
    /// The mnemonic as written.
    pub text: String,
    /// The mnemonic lowercased, which is how MARIE.js looks it up.
    pub lowercase: String,
    /// What it names.
    pub kind: MnemonicKind,
    /// Where it was written.
    pub span: Span,
}

impl Mnemonic {
    /// Returns the opcode this mnemonic names, if it names one.
    pub fn opcode(&self) -> Option<Opcode> {
        match self.kind {
            MnemonicKind::Opcode(opcode) => Some(opcode),
            _ => None,
        }
    }

    /// Returns the directive this mnemonic names, if it names one.
    pub fn directive(&self) -> Option<Directive> {
        match self.kind {
            MnemonicKind::Directive(directive) => Some(directive),
            _ => None,
        }
    }

    /// Returns the documentation string for whatever this names.
    ///
    /// This is what a language server shows on hover.
    pub fn description(&self) -> Option<&'static str> {
        match self.kind {
            MnemonicKind::Opcode(opcode) => Some(opcode.description()),
            MnemonicKind::Directive(directive) => Some(directive.description()),
            MnemonicKind::Unknown => None,
        }
    }
}

/// The operand of a statement, before it is known to be a literal or a label.
///
/// Which one it is depends on the mnemonic, so the distinction is drawn during
/// assembly rather than parsing. See [`Operand::looks_like_address_literal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operand {
    /// The operand as written.
    pub text: String,
    /// Where it was written.
    pub span: Span,
}

impl Operand {
    /// Returns `true` if this operand is an address literal rather than a label
    /// reference.
    ///
    /// MARIE.js decides with `/^\d[0-9a-fA-F]*$/`: an operand that *starts with a
    /// decimal digit* is a hexadecimal address, and anything else is a label. So
    /// `Load 1A` is address `0x1A` while `Load A1` is a reference to a label named
    /// `A1` — a distinction that is easy to miss and easy to get wrong.
    pub fn looks_like_address_literal(&self) -> bool {
        let mut chars = self.text.chars();
        chars.next().is_some_and(|c| c.is_ascii_digit()) && chars.all(|c| c.is_ascii_hexdigit())
    }
}

/// One statement: an optional label, a mnemonic, and an optional operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The label defined on this line, if any.
    pub label: Option<Label>,
    /// The operation.
    pub mnemonic: Mnemonic,
    /// The operand, if the line has one.
    pub operand: Option<Operand>,
    /// The trailing comment, if any.
    pub comment: Option<Span>,
    /// The address this item assembles to.
    pub address: MemoryAddress,
    /// The zero-based line it was written on.
    pub line: u32,
    /// The whole statement, excluding the comment.
    pub span: Span,
}

/// An `ORG` directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    /// The address assembly starts at.
    pub address: MemoryAddress,
    /// The three hex digits.
    pub digits: Span,
    /// The whole directive.
    pub span: Span,
    /// The zero-based line it was written on.
    pub line: u32,
}

/// An `END` directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    /// The whole directive.
    pub span: Span,
    /// The zero-based line it was written on.
    pub line: u32,
}

/// A parsed source file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    /// The `ORG` directive, if the file has a valid one.
    pub origin: Option<Origin>,
    /// The statements, in source order, each already assigned its address.
    pub items: Vec<Item>,
    /// The `END` directive that stopped parsing, if there was one.
    pub end: Option<End>,
    /// The text after `END`, which the assembler never looked at.
    ///
    /// Retained so an editor can grey it out and a lint can mention it.
    pub trailing: Option<Span>,
}

impl Program {
    /// Returns the address the program is assembled at.
    pub fn origin_address(&self) -> MemoryAddress {
        self.origin.map_or(MemoryAddress::ZERO, |o| o.address)
    }

    /// Returns the item whose span contains `offset`.
    ///
    /// This is the entry point for hover, go-to-definition and signature help.
    pub fn item_at(&self, offset: u32) -> Option<&Item> {
        self.items.iter().find(|item| item.span.touches(offset))
    }

    /// Returns the item assembled at `address`.
    pub fn item_at_address(&self, address: MemoryAddress) -> Option<&Item> {
        self.items.iter().find(|item| item.address == address)
    }

    /// Returns the items defined on `line`, which is at most one.
    pub fn item_on_line(&self, line: u32) -> Option<&Item> {
        self.items.iter().find(|item| item.line == line)
    }
}
