//! MARIE VM states

mod sealed {
    pub trait Sealed {}
}

pub trait MarieVmState: sealed::Sealed {}

pub enum Halted {}
impl sealed::Sealed for Halted {}
impl MarieVmState for Halted {}

pub enum Running {}
impl sealed::Sealed for Running {}
impl MarieVmState for Running {}

pub enum Stepping {}
impl sealed::Sealed for Stepping {}
impl MarieVmState for Stepping {}
