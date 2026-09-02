//! MARIE VM IO module

use std::task::Poll;

#[derive(Debug)]
pub enum IoError {
    Eof,
    Io(std::io::Error),
    Parse(String),
}

/// Contract for each IO device to implement
pub trait MarieVmIODevice {
    /// Polls the input for a 16-bit value from the device
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>>;

    /// outputs a 16-bit value to the device
    fn output(&mut self, value: i16) -> Result<(), IoError>;
}

/// Real device stdin/stdout
pub struct StdinIo;

impl MarieVmIODevice for StdinIo {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        use std::io::{BufRead, Write};
        print!("input> ");
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        Poll::Ready(match std::io::stdin().lock().read_line(&mut line) {
            Err(e) => Err(IoError::Io(e)),
            Ok(0) => Err(IoError::Eof),
            Ok(_) => parse_word(line.trim()),
        })
    }

    fn output(&mut self, value: i16) -> Result<(), IoError> {
        println!("{value}");
        Ok(())
    }
}

fn parse_word(s: &str) -> Result<i16, IoError> {
    s.parse::<i16>().map_err(|e| IoError::Parse(e.to_string()))
}

/// Scripted device: deterministic tests.
#[derive(Default)]
pub struct VecIo {
    pub inputs: std::collections::VecDeque<i16>,
    pub outputs: Vec<i16>,
}

impl MarieVmIODevice for VecIo {
    fn poll_input(&mut self) -> Poll<Result<i16, IoError>> {
        Poll::Ready(self.inputs.pop_front().ok_or(IoError::Eof))
    }
    fn output(&mut self, value: i16) -> Result<(), IoError> {
        self.outputs.push(value);
        Ok(())
    }
}

/// Decorator: forces N `Pending` polls before delegating.
pub struct Flaky<D> {
    inner: D,
    stall: u8,
    remaining: u8,
}

impl<D> Flaky<D> {
    pub fn new(inner: D, stall: u8) -> Self {
        Self {
            inner,
            stall,
            remaining: stall,
        }
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
}
