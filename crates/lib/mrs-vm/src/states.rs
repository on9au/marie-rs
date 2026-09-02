//! MARIE VM states

use crate::MarieVM;

mod sealed {
    pub trait Sealed {}
}

pub trait MarieVmState: sealed::Sealed {}

pub enum Ready {}
impl sealed::Sealed for Ready {}
impl MarieVmState for Ready {}

pub enum Running {}
impl sealed::Sealed for Running {}
impl MarieVmState for Running {}

pub enum Stepping {}
impl sealed::Sealed for Stepping {}
impl MarieVmState for Stepping {}

pub enum Terminated {}
impl sealed::Sealed for Terminated {}
impl MarieVmState for Terminated {}

pub enum RunOutcome<IO> {
    Terminated(MarieVM<IO, Terminated>),
    Suspended(MarieVM<IO, Running>, SuspendReason),
    Faulted(MarieVM<IO, Terminated>, Fault),
}

pub enum StepOutcome<IO> {
    Stepped(MarieVM<IO, Stepping>),
    Terminated(MarieVM<IO, Terminated>),
    Faulted(MarieVM<IO, Terminated>, Fault),
}

pub enum SuspendReason {
    Breakpoint,
    StepComplete,
}

pub enum Fault {
    InvalidOpcode,
}
