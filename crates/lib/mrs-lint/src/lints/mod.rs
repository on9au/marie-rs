//! The lints this crate ships.
//!
//! Grouped by what they are about: [`flow`] needs the control-flow graph, [`hazards`]
//! covers constructs that assemble cleanly and then misbehave, and [`style`] is about
//! how the source reads.

pub mod flow;
pub mod hazards;
pub mod style;
