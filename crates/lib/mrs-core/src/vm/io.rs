//! MARIE VM IO module

/// Contract for each IO device to implement
pub trait MarieVmIODevice {
    /// Reads/inputs a 16-bit value from the device
    fn read(&self) -> i16;

    /// Writes/outputs a 16-bit value to the device
    fn write(&self, value: i16);
}
