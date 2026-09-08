//! Graceful Ctrl-C handling.
//!
//! Without a handler, Ctrl-C kills the process where it stands. That is fine for a
//! one-shot command and poor for a running machine: the terminal is left mid-frame
//! when the display is drawing, and nothing says how far the program got.
//!
//! So the signal sets a flag, the run loops check it between batches, and the machine
//! stops the way it would at a breakpoint — reporting where it stopped.
//!
//! # Arming
//!
//! What Ctrl-C should mean depends on what the program is doing. While the machine is
//! running it means *stop the machine*; sitting at the debugger prompt it means *quit*,
//! because there is nothing to interrupt and a flag nobody reads would look like the
//! key did nothing. So the handler is armed only around a run, and a press outside one
//! exits immediately — as does a second press during a run, so a wedged loop can never
//! trap the terminal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The exit status a shell reports for a process ended by SIGINT.
const INTERRUPTED: i32 = 130;

/// The longest a sleep will go without noticing a Ctrl-C.
///
/// The slowest speed level pauses for a whole second between steps, and waiting that
/// long to react would feel like the key had been ignored.
const SLEEP_SLICE: Duration = Duration::from_millis(20);

/// A Ctrl-C handler and the flag it sets.
#[derive(Clone)]
pub struct Interrupt {
    stop: Arc<AtomicBool>,
    armed: Arc<AtomicBool>,
}

impl Interrupt {
    /// Installs the handler, armed: Ctrl-C asks the machine to stop.
    pub fn install() -> Self {
        Self::new(true)
    }

    /// Installs the handler, disarmed: Ctrl-C exits until [`Interrupt::arm`] is called.
    ///
    /// This is what the debugger wants, so that Ctrl-C at its prompt quits rather than
    /// setting a flag nothing is watching.
    pub fn install_disarmed() -> Self {
        Self::new(false)
    }

    fn new(armed: bool) -> Self {
        let interrupt = Self {
            stop: Arc::new(AtomicBool::new(false)),
            armed: Arc::new(AtomicBool::new(armed)),
        };
        let stop = Arc::clone(&interrupt.stop);
        let armed = Arc::clone(&interrupt.armed);
        // A failure here means a handler is already installed. Losing Ctrl-C handling
        // is a far better outcome than refusing to run the program.
        let _ = ctrlc::set_handler(move || {
            // Nothing to interrupt, or already asked once: go now.
            if !armed.load(Ordering::SeqCst) || stop.swap(true, Ordering::SeqCst) {
                std::process::exit(INTERRUPTED);
            }
        });
        interrupt
    }

    /// Arms the handler until the returned guard is dropped, clearing any stale flag.
    #[must_use = "the handler is disarmed again when the guard is dropped"]
    pub fn arm(&self) -> Armed<'_> {
        self.stop.store(false, Ordering::SeqCst);
        self.armed.store(true, Ordering::SeqCst);
        Armed(self)
    }

    /// Returns `true` once Ctrl-C has been pressed.
    pub fn requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Sleeps for `duration`, returning early if Ctrl-C is pressed.
    ///
    /// Returns `true` if it was interrupted.
    pub fn sleep(&self, duration: Duration) -> bool {
        let deadline = Instant::now() + duration;
        loop {
            if self.requested() {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            std::thread::sleep(remaining.min(SLEEP_SLICE));
        }
    }
}

/// Disarms the handler when dropped.
pub struct Armed<'a>(&'a Interrupt);

impl Armed<'_> {
    /// Returns `true` once Ctrl-C has been pressed.
    pub fn requested(&self) -> bool {
        self.0.requested()
    }
}

impl Drop for Armed<'_> {
    fn drop(&mut self) {
        self.0.armed.store(false, Ordering::SeqCst);
        self.0.stop.store(false, Ordering::SeqCst);
    }
}

/// Reports a run stopped by Ctrl-C and exits.
///
/// Exits with [`INTERRUPTED`] rather than returning an error, because a script that
/// checks the status should be able to tell a deliberate stop from a program that
/// failed. It is worded as what it is: a stop, not a fault.
pub fn stopped(location: impl std::fmt::Display, instructions: u64) -> ! {
    let report = miette::miette!(
        help = "the machine stopped where it was; nothing was lost",
        "interrupted at {location} after {instructions} instruction{}",
        if instructions == 1 { "" } else { "s" }
    );
    eprintln!("{report:?}");
    std::process::exit(INTERRUPTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An interrupt with no handler installed, so tests can drive the flag directly.
    fn detached() -> Interrupt {
        Interrupt {
            stop: Arc::new(AtomicBool::new(false)),
            armed: Arc::new(AtomicBool::new(true)),
        }
    }

    #[test]
    fn an_unset_flag_sleeps_for_the_whole_duration() {
        let interrupt = detached();
        assert!(!interrupt.requested());
        let started = Instant::now();
        assert!(!interrupt.sleep(Duration::from_millis(60)));
        assert!(started.elapsed() >= Duration::from_millis(50));
    }

    #[test]
    fn a_set_flag_cuts_a_long_sleep_short() {
        let interrupt = detached();
        interrupt.stop.store(true, Ordering::SeqCst);
        let started = Instant::now();
        // A whole second is the slowest speed level's delay.
        assert!(interrupt.sleep(Duration::from_secs(1)));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn a_flag_set_partway_through_a_sleep_is_noticed_promptly() {
        // The case that matters: Ctrl-C during the slowest level's one-second pause.
        let interrupt = detached();
        let flag = Arc::clone(&interrupt.stop);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            flag.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        assert!(interrupt.sleep(Duration::from_secs(5)));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn arming_clears_a_stale_flag_and_disarming_restores_it() {
        let interrupt = detached();
        interrupt.stop.store(true, Ordering::SeqCst);
        {
            let armed = interrupt.arm();
            assert!(!armed.requested(), "a stale press is not carried forward");
            assert!(interrupt.armed.load(Ordering::SeqCst));
        }
        assert!(!interrupt.armed.load(Ordering::SeqCst), "disarmed on drop");
        assert!(!interrupt.requested());
    }
}
