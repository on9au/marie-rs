//! MARIE VM IO module

use std::collections::VecDeque;
use std::fmt;
use std::task::Poll;

use mrs_core::literal::{ParseWordError, parse_prefixed_word};

/// An error raised by an I/O device.
#[derive(Debug)]
pub enum IoError {
    /// The input stream is exhausted; no further values can be read.
    Eof,
    /// The underlying stream failed.
    Io(std::io::Error),
    /// A value was read but could not be interpreted as a 16-bit word.
    Parse(String),
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::Eof => f.write_str("end of input"),
            IoError::Io(error) => write!(f, "I/O error: {error}"),
            IoError::Parse(message) => write!(f, "could not parse input: {message}"),
        }
    }
}

impl std::error::Error for IoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IoError::Io(error) => Some(error),
            IoError::Eof | IoError::Parse(_) => None,
        }
    }
}

impl From<std::io::Error> for IoError {
    fn from(error: std::io::Error) -> Self {
        IoError::Io(error)
    }
}

impl From<ParseWordError> for IoError {
    fn from(error: ParseWordError) -> Self {
        IoError::Parse(error.to_string())
    }
}

/// Contract for each IO device to implement
pub trait MarieVmIODevice {
    /// Polls the input for a 16-bit value from the device
    ///
    /// Returning [`Poll::Pending`] suspends the VM part-way through the `Input`
    /// instruction, at the micro-operation that reads the device. The device is
    /// polled again when the VM is resumed, so a poll that returns `Pending` must not
    /// consume a value or have any other side effect.
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>>;

    /// outputs a 16-bit value to the device
    fn output(&mut self, value: i16) -> Result<(), IoError>;

    /// Pushes `value` back so that the next [`MarieVmIODevice::poll_input`] returns it.
    ///
    /// This is what lets the debugger step backwards over an `Input` instruction.
    /// Returning `false` — the default — means the device cannot rewind, and
    /// [`step_back`](crate::MarieVM::step_back) reports
    /// [`StepBackError::IrreversibleInput`](crate::history::StepBackError::IrreversibleInput)
    /// rather than silently losing the value.
    ///
    /// Implementations must not perform any other side effect when returning `false`.
    fn unread_input(&mut self, value: i16) -> bool {
        let _ = value;
        false
    }

    /// Retracts the most recent output, which is guaranteed to have been `value`.
    ///
    /// Returning `false` — the default — means the device cannot rewind; see
    /// [`MarieVmIODevice::unread_input`].
    fn unwrite_output(&mut self, value: i16) -> bool {
        let _ = value;
        false
    }
}

impl<D: MarieVmIODevice + ?Sized> MarieVmIODevice for &mut D {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        (**self).poll_input()
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        (**self).output(value)
    }

    fn unread_input(&mut self, value: i16) -> bool {
        (**self).unread_input(value)
    }

    fn unwrite_output(&mut self, value: i16) -> bool {
        (**self).unwrite_output(value)
    }
}

impl<D: MarieVmIODevice + ?Sized> MarieVmIODevice for Box<D> {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        (**self).poll_input()
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        (**self).output(value)
    }

    fn unread_input(&mut self, value: i16) -> bool {
        (**self).unread_input(value)
    }

    fn unwrite_output(&mut self, value: i16) -> bool {
        (**self).unwrite_output(value)
    }
}

/// Real device stdin/stdout
///
/// Neither direction can be rewound, so a debugger stepping backwards over `Input` or
/// `Output` on this device will report the operation as irreversible.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdinIo;

impl MarieVmIODevice for StdinIo {
    /// Prompts on stdout and blocks until a word is read, so this never returns
    /// [`Poll::Pending`]. Blank lines and unparseable input re-prompt rather than
    /// faulting the VM, matching the MARIE.js input dialog.
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        use std::io::{BufRead, Write};

        let stdin = std::io::stdin();
        loop {
            print!("input> ");
            let _ = std::io::stdout().flush();

            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Err(e) => return Poll::Ready(Err(IoError::Io(e))),
                Ok(0) => return Poll::Ready(Err(IoError::Eof)),
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match parse_prefixed_word(trimmed) {
                        Ok(word) => return Poll::Ready(Ok(word.value())),
                        Err(e) => eprintln!("{e}"),
                    }
                }
            }
        }
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        println!("{value}");
        Ok(())
    }
}

/// Scripted device: deterministic tests.
///
/// Both directions are rewindable, so this device supports stepping backwards.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VecIo {
    /// Values that will be returned by successive calls to `poll_input`.
    pub inputs: VecDeque<i16>,
    /// Values that have been written by `output`, in order.
    pub outputs: Vec<i16>,
}

impl VecIo {
    /// Creates a device that will yield `inputs`, in order.
    pub fn new<I: IntoIterator<Item = i16>>(inputs: I) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            outputs: Vec::new(),
        }
    }

    /// Removes and returns everything written so far.
    pub fn take_outputs(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.outputs)
    }
}

impl MarieVmIODevice for VecIo {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        Poll::Ready(self.inputs.pop_front().ok_or(IoError::Eof))
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        self.outputs.push(value);
        Ok(())
    }

    fn unread_input(&mut self, value: i16) -> bool {
        self.inputs.push_front(value);
        true
    }

    fn unwrite_output(&mut self, value: i16) -> bool {
        // Only retract if the tail really is the value being undone; otherwise
        // something else has written to this device and rewinding would corrupt it.
        if self.outputs.last() == Some(&value) {
            self.outputs.pop();
            true
        } else {
            false
        }
    }
}

/// Decorator: forces N `Pending` polls before delegating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flaky<D> {
    inner: D,
    stall: u8,
    remaining: u8,
}

impl<D> Flaky<D> {
    /// Wraps `inner` so that every input poll stalls `stall` times before it is delegated.
    pub fn new(inner: D, stall: u8) -> Self {
        Self {
            inner,
            stall,
            remaining: stall,
        }
    }

    /// Returns a reference to the wrapped device.
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// Returns a mutable reference to the wrapped device.
    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.inner
    }

    /// Unwraps this decorator, returning the wrapped device.
    pub fn into_inner(self) -> D {
        self.inner
    }
}

impl<D: MarieVmIODevice> MarieVmIODevice for Flaky<D> {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        if self.remaining > 0 {
            self.remaining -= 1;
            return Poll::Pending;
        }
        self.remaining = self.stall;
        self.inner.poll_input()
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        self.inner.output(value)
    }

    fn unread_input(&mut self, value: i16) -> bool {
        // Restore the stall counter too, so replaying re-stalls the same way.
        if self.inner.unread_input(value) {
            self.remaining = self.stall;
            true
        } else {
            false
        }
    }

    fn unwrite_output(&mut self, value: i16) -> bool {
        self.inner.unwrite_output(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flaky_stalls_before_each_delegated_poll() {
        let mut device = Flaky::new(VecIo::new([7]), 2);
        assert!(device.poll_input().is_pending());
        assert!(device.poll_input().is_pending());
        assert!(matches!(device.poll_input(), Poll::Ready(Ok(7))));
        assert_eq!(device.into_inner().inputs.len(), 0);
    }

    #[test]
    fn vec_io_rewinds_both_directions() {
        let mut device = VecIo::new([1, 2]);
        assert!(matches!(device.poll_input(), Poll::Ready(Ok(1))));
        assert!(device.unread_input(1));
        assert!(matches!(device.poll_input(), Poll::Ready(Ok(1))));

        device.output(9).unwrap();
        assert!(device.unwrite_output(9));
        assert!(device.outputs.is_empty());
        // Refuses to retract something it did not just write.
        assert!(!device.unwrite_output(9));
    }

    #[test]
    fn stdin_io_reports_itself_as_irreversible() {
        let mut device = StdinIo;
        assert!(!device.unread_input(1));
        assert!(!device.unwrite_output(1));
    }
}
