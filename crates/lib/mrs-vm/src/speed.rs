//! The execution-speed levels of the MARIE.js slider.
//!
//! A ten-position slider that paces a running program, from one register transfer per
//! second up to unlimited. It exists so a class can watch the fetch-decode-execute
//! cycle happen; at the slow end you see `MAR <- PC`, then `MBR <- M[MAR]`, one at a
//! time.
//!
//! # What a "step" is here
//!
//! Micro-operations, not instructions. MARIE.js paces `doRunStep`, which performs a
//! micro-step and skips its two marker actions:
//!
//! ```js
//! // When running continuously, skip over some virtual actions so that the clock
//! // pulses are more regular
//! if (action?.type === 'decode' || action?.type == 'step-end') continue;
//! ```
//!
//! That leaves `5 + execute` paced units per instruction, which is exactly the number
//! of [`MicroOp`](crate::microcode::MicroOp)s this crate executes per instruction — its
//! `Decode` sits where MARIE.js has its `step` marker. So a level here advances the
//! machine at the same rate as the same level on the website.
//!
//! # No waiting happens here
//!
//! A [`Speed`] only *describes* the pacing: how many micro-steps to run, and how long
//! to pause afterwards. It never sleeps. Sleeping is the driver's business, and the
//! right way to wait differs — a terminal blocks the thread, a browser uses
//! `setTimeout` — so a library that picked one would be unusable from the other.
//!
//! ```
//! use mrs_vm::speed::Speed;
//!
//! let speed = Speed::new(0).unwrap();
//! assert_eq!(speed.micro_steps(), Some(1));
//! assert_eq!(speed.delay().as_millis(), 1000);
//!
//! // The top of the slider runs flat out.
//! assert!(Speed::FASTEST.is_unlimited());
//! assert_eq!(Speed::FASTEST.delay().as_millis(), 0);
//! ```

use std::fmt;
use std::time::Duration;

/// One position of the speed slider.
///
/// Levels run from `0` (slowest) to `9` (unlimited).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Speed(u8);

/// `(micro-steps per batch, delay in milliseconds)` for each slider position.
///
/// Copied from MARIE.js's `speeds` table. Note that levels 3 and 4 are **not** in
/// speed order there — 3 delays 10 ms and 4 delays 30 ms, so level 4 is slower than
/// level 3. That looks like a transposition in the original, but it is what the
/// website does, and matching it is the point of this table; see
/// [`Speed::is_monotonic`].
const LEVELS: [(Option<u64>, u64); 10] = [
    (Some(1), 1000),
    (Some(1), 500),
    (Some(1), 250),
    (Some(1), 10),
    (Some(1), 30),
    (Some(1), 0),
    (Some(10), 0),
    (Some(50), 0),
    (Some(100), 0),
    (None, 0),
];

impl Speed {
    /// The number of slider positions.
    pub const LEVELS: u8 = LEVELS.len() as u8;

    /// The slowest level: one micro-operation per second.
    pub const SLOWEST: Self = Self(0);

    /// The fastest level: unlimited.
    pub const FASTEST: Self = Self(Self::LEVELS - 1);

    /// The level MARIE.js starts at, which is unlimited.
    pub const DEFAULT: Self = Self::FASTEST;

    /// How long a batch may run before it is cut short regardless of the level.
    ///
    /// MARIE.js checks `performance.now() - start < 20` inside the batch loop so the
    /// page keeps responding. A driver should apply the same cap, or a fast level with
    /// a large batch will stall whatever is drawing.
    pub const BATCH_TIME_LIMIT: Duration = Duration::from_millis(20);

    /// Creates a speed from a slider position, or `None` if it is out of range.
    pub const fn new(level: u8) -> Option<Self> {
        if level < Self::LEVELS {
            Some(Self(level))
        } else {
            None
        }
    }

    /// Creates a speed, clamping to the ends of the slider.
    pub const fn clamped(level: u8) -> Self {
        if level < Self::LEVELS {
            Self(level)
        } else {
            Self::FASTEST
        }
    }

    /// The slider position.
    pub const fn level(self) -> u8 {
        self.0
    }

    /// How many micro-operations to run before pausing, or `None` for unlimited.
    pub const fn micro_steps(self) -> Option<u64> {
        LEVELS[self.0 as usize].0
    }

    /// How long to wait after a batch.
    pub const fn delay(self) -> Duration {
        Duration::from_millis(LEVELS[self.0 as usize].1)
    }

    /// Returns `true` if this level runs without a step budget.
    pub const fn is_unlimited(self) -> bool {
        self.micro_steps().is_none()
    }

    /// Returns `true` if this level pauses between batches.
    pub const fn is_delayed(self) -> bool {
        LEVELS[self.0 as usize].1 > 0
    }

    /// The next level up, or `None` at the top.
    pub const fn faster(self) -> Option<Self> {
        Self::new(self.0 + 1)
    }

    /// The next level down, or `None` at the bottom.
    pub const fn slower(self) -> Option<Self> {
        if self.0 == 0 {
            None
        } else {
            Self::new(self.0 - 1)
        }
    }

    /// Every level, slowest first.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..Self::LEVELS).map(Self)
    }

    /// Returns `false` if the level table is not in increasing order of speed.
    ///
    /// It is not: MARIE.js's levels 3 and 4 are transposed. Exposed so a caller that
    /// would rather present a sensible slider than a faithful one can tell.
    pub fn is_monotonic() -> bool {
        LEVELS.windows(2).all(|pair| {
            let (left, right) = (pair[0], pair[1]);
            // Faster means a larger batch, or the same batch with less delay.
            match (left.0, right.0) {
                (Some(a), Some(b)) if a == b => right.1 <= left.1,
                (Some(a), Some(b)) => b >= a,
                (Some(_), None) => true,
                (None, _) => false,
            }
        })
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for Speed {
    /// Describes the level in the terms a user would set it by.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.micro_steps(), self.delay().as_millis()) {
            (None, _) => write!(f, "{}: unlimited", self.0),
            (Some(steps), 0) => {
                let plural = if steps == 1 { "" } else { "s" };
                write!(f, "{}: {steps} step{plural} per batch, no delay", self.0)
            }
            (Some(steps), delay) => {
                let plural = if steps == 1 { "" } else { "s" };
                write!(f, "{}: {steps} step{plural} every {delay} ms", self.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_marie_js() {
        let expected: [(Option<u64>, u128); 10] = [
            (Some(1), 1000),
            (Some(1), 500),
            (Some(1), 250),
            (Some(1), 10),
            (Some(1), 30),
            (Some(1), 0),
            (Some(10), 0),
            (Some(50), 0),
            (Some(100), 0),
            (None, 0),
        ];
        for (level, (steps, delay)) in expected.into_iter().enumerate() {
            let speed = Speed::new(level as u8).expect("a level");
            assert_eq!(speed.micro_steps(), steps, "level {level}");
            assert_eq!(speed.delay().as_millis(), delay, "level {level}");
        }
    }

    #[test]
    fn the_default_is_unlimited_like_the_website() {
        // MARIE.js stores `speed: 9` in its default settings.
        assert_eq!(Speed::default(), Speed::FASTEST);
        assert_eq!(Speed::DEFAULT.level(), 9);
        assert!(Speed::DEFAULT.is_unlimited());
        assert!(!Speed::DEFAULT.is_delayed());
    }

    #[test]
    fn levels_three_and_four_are_transposed_in_the_original() {
        // Documented rather than silently corrected: this is what the website does.
        let three = Speed::new(3).unwrap();
        let four = Speed::new(4).unwrap();
        assert!(
            four.delay() > three.delay(),
            "level 4 really is slower than level 3"
        );
        assert!(!Speed::is_monotonic(), "and so the table is not monotonic");
    }

    #[test]
    fn levels_are_bounded_and_navigable() {
        assert_eq!(Speed::new(10), None);
        assert_eq!(Speed::clamped(200), Speed::FASTEST);
        assert_eq!(Speed::SLOWEST.slower(), None);
        assert_eq!(Speed::FASTEST.faster(), None);
        assert_eq!(Speed::SLOWEST.faster(), Speed::new(1));
        assert_eq!(Speed::FASTEST.slower(), Speed::new(8));
        assert_eq!(Speed::all().count(), 10);
    }

    #[test]
    fn levels_describe_themselves() {
        assert_eq!(Speed::SLOWEST.to_string(), "0: 1 step every 1000 ms");
        assert_eq!(
            Speed::new(5).unwrap().to_string(),
            "5: 1 step per batch, no delay"
        );
        assert_eq!(
            Speed::new(6).unwrap().to_string(),
            "6: 10 steps per batch, no delay"
        );
        assert_eq!(Speed::FASTEST.to_string(), "9: unlimited");
    }
}
