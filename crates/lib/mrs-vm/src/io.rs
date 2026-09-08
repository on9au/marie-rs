//! MARIE VM IO module

use std::collections::VecDeque;
use std::task::Poll;

use thiserror::Error;

use mrs_core::display::Rgb555;
use mrs_core::literal::{ParseWordError, parse_prefixed_word};

/// An error raised by an I/O device.
#[derive(Debug, Error)]
pub enum IoError {
    /// The input stream is exhausted; no further values can be read.
    #[error("end of input")]
    Eof,
    /// The underlying stream failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// A value was read but could not be interpreted as a 16-bit word.
    #[error("could not parse input: {0}")]
    Parse(String),
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

    /// Notifies the device that a pixel of the memory-mapped display changed.
    ///
    /// The display is written with ordinary `Store` instructions rather than by an I/O
    /// instruction, so nothing would otherwise tell a frontend that the picture moved.
    /// This hook is what lets one draw as the program runs instead of polling
    /// [`MarieVM::display`](crate::MarieVM::display) on a timer, and it is where a web
    /// frontend hangs its own rendering.
    ///
    /// `index` is row-major, `0..256`. Only called when the pixel actually changes, and
    /// also called when [`step_back`](crate::MarieVM::step_back) restores a pixel, so a
    /// frontend stays correct while rewinding. The default is to ignore it.
    fn display_write(&mut self, index: usize, pixel: Rgb555) {
        let _ = (index, pixel);
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

    fn display_write(&mut self, index: usize, pixel: Rgb555) {
        (**self).display_write(index, pixel);
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

    fn display_write(&mut self, index: usize, pixel: Rgb555) {
        (**self).display_write(index, pixel);
    }
}

/// How a line of typed input becomes machine words.
///
/// MARIE.js's "Inputs" panel offers the same choice: a value at a time, or a string
/// spent one code unit at a time. The distinction only exists because one typed line
/// can feed more than one `Input` instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// One value per line: decimal, or `0x`, `0o` and `0b` for another base.
    #[default]
    Word,
    /// A line of text, read one UTF-16 code unit per `Input`.
    ///
    /// MARIE.js names this mode UTF-16BE; a word holds a whole code unit, so the byte
    /// order never actually shows. A character outside the Basic Multilingual Plane —
    /// an emoji, say — is a surrogate pair and so spends two `Input` instructions,
    /// which is UTF-16 working rather than a quirk: a code point does not always fit
    /// in sixteen bits.
    Utf16,
}

impl InputMode {
    /// Decodes one line of input, appending the words it yields to `queue`.
    ///
    /// A line may yield no words at all — a blank one in [`InputMode::Word`], an empty
    /// one in [`InputMode::Utf16`] — which is what lets a device re-prompt instead of
    /// faulting the VM.
    ///
    /// # Errors
    ///
    /// Returns [`ParseWordError`] if [`InputMode::Word`] was given something that is
    /// not a literal. Decoding text cannot fail.
    pub fn decode(self, line: &str, queue: &mut VecDeque<i16>) -> Result<(), ParseWordError> {
        match self {
            InputMode::Word => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    queue.push_back(parse_prefixed_word(trimmed)?.value());
                }
            }
            InputMode::Utf16 => {
                // Only the line ending is stripped. Spaces within the line are part of
                // the string being fed — `123 + 456 =` is read a character at a time,
                // separators included — so trimming them would change the program's
                // input.
                let text = line.strip_suffix('\n').unwrap_or(line);
                let text = text.strip_suffix('\r').unwrap_or(text);
                // A code unit is a bit pattern, so the top half of the range wraps to
                // the negative words rather than being out of range, exactly as
                // `HEX FFFF` assembles to `-1`.
                queue.extend(text.encode_utf16().map(|unit| unit as i16));
            }
        }
        Ok(())
    }
}

/// Prompts on stdout and reads the next word from stdin, refilling `queue` as needed.
///
/// `queue` holds what an earlier line yielded and has not been read yet; a line is only
/// asked for once it is empty, which is what makes one typed string feed many `Input`
/// instructions in [`InputMode::Utf16`]. Blank lines and unparseable input re-prompt
/// rather than failing, matching the MARIE.js input dialog.
///
/// This blocks, so it belongs to a terminal frontend; a device that must not block owns
/// its queue and fills it from wherever its input really comes from.
///
/// # Errors
///
/// Returns [`IoError::Eof`] at end of input and [`IoError::Io`] if stdin fails.
pub fn prompt_stdin(mode: InputMode, queue: &mut VecDeque<i16>) -> Result<i16, IoError> {
    use std::io::{BufRead, Write};

    if let Some(value) = queue.pop_front() {
        return Ok(value);
    }

    let stdin = std::io::stdin();
    loop {
        print!("input> ");
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Err(e) => return Err(IoError::Io(e)),
            Ok(0) => return Err(IoError::Eof),
            Ok(_) => match mode.decode(&line, queue) {
                Ok(()) => {
                    if let Some(value) = queue.pop_front() {
                        return Ok(value);
                    }
                }
                Err(e) => eprintln!("{e}"),
            },
        }
    }
}

/// Real device stdin/stdout
///
/// Neither direction can be rewound, so a debugger stepping backwards over `Input` or
/// `Output` on this device will report the operation as irreversible.
#[derive(Debug, Clone, Default)]
pub struct StdinIo {
    /// How a typed line is decoded.
    mode: InputMode,
    /// Words the last line yielded that have not been read yet. Never more than one
    /// in [`InputMode::Word`].
    pending: VecDeque<i16>,
}

impl StdinIo {
    /// Creates a device that reads one word per line.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a device that reads lines in `mode`.
    pub fn with_mode(mode: InputMode) -> Self {
        Self {
            mode,
            pending: VecDeque::new(),
        }
    }

    /// Returns the mode typed lines are read in.
    pub fn mode(&self) -> InputMode {
        self.mode
    }
}

impl MarieVmIODevice for StdinIo {
    /// Prompts on stdout and blocks until a word is read, so this never returns
    /// [`Poll::Pending`]. In [`InputMode::Utf16`] a line is only asked for once the
    /// previous one has been spent, so one string feeds many `Input` instructions.
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        Poll::Ready(prompt_stdin(self.mode, &mut self.pending))
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
        let mut device = StdinIo::new();
        assert!(!device.unread_input(1));
        assert!(!device.unwrite_output(1));
    }

    /// Decodes `line` and returns the words it yields.
    fn decode(mode: InputMode, line: &str) -> Result<Vec<i16>, ParseWordError> {
        let mut queue = VecDeque::new();
        mode.decode(line, &mut queue)?;
        Ok(queue.into())
    }

    #[test]
    fn a_word_line_yields_exactly_one_value() {
        assert_eq!(decode(InputMode::Word, "17\n").unwrap(), [17]);
        assert_eq!(decode(InputMode::Word, "  -7  \n").unwrap(), [-7]);
        assert_eq!(decode(InputMode::Word, "0x1f\n").unwrap(), [31]);
        // A blank line yields nothing, so the device re-prompts rather than faulting.
        assert_eq!(decode(InputMode::Word, "   \n").unwrap(), []);
        assert!(decode(InputMode::Word, "twelve\n").is_err());
    }

    #[test]
    fn a_utf16_line_yields_one_word_per_code_unit() {
        // The spaces are part of the string: an expression is read separators and all.
        assert_eq!(
            decode(InputMode::Utf16, "1 + 2\n").unwrap(),
            ['1' as i16, ' ' as i16, '+' as i16, ' ' as i16, '2' as i16]
        );
        // Line endings are not, in either flavour.
        assert_eq!(decode(InputMode::Utf16, "=\r\n").unwrap(), ['=' as i16]);
        assert_eq!(decode(InputMode::Utf16, "\n").unwrap(), []);
    }

    #[test]
    fn a_utf16_line_spends_a_surrogate_pair_on_one_character() {
        // U+1F600 is outside the BMP, so it is two code units and two `Input`s.
        assert_eq!(
            decode(InputMode::Utf16, "\u{1f600}\n").unwrap(),
            [0xD83Du16 as i16, 0xDE00u16 as i16]
        );
        // The top half of the range is a bit pattern, not an out-of-range value.
        assert_eq!(decode(InputMode::Utf16, "\u{ffff}").unwrap(), [-1]);
    }

    #[test]
    fn a_utf16_queue_is_drained_before_another_line_is_read() {
        let mut device = StdinIo::with_mode(InputMode::Utf16);
        assert_eq!(device.mode(), InputMode::Utf16);
        // `prompt_stdin` only touches the terminal once the queue is empty, so a
        // pre-filled queue exercises the draining without one.
        device.pending.extend([b'h' as i16, b'i' as i16]);
        assert!(matches!(device.poll_input(), Poll::Ready(Ok(0x68))));
        assert!(matches!(device.poll_input(), Poll::Ready(Ok(0x69))));
        assert!(device.pending.is_empty());
    }
}
