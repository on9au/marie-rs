//! MARIE VM states

pub trait MarieVmState {}

pub struct Halted;
impl MarieVmState for Halted {}
pub struct Running;
impl MarieVmState for Running {}
pub struct Stepping;
impl MarieVmState for Stepping {}
