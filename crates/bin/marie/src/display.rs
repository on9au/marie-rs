//! Rendering the memory-mapped display in a terminal.
//!
//! Each pixel is drawn as two full blocks, because a terminal cell is about twice as
//! tall as it is wide: two cells across by one down is square. That is also what makes
//! a 16×16 display come out 32 columns wide.

use std::io::Write;

use mrs_core::display::DISPLAY_HEIGHT;
use mrs_vm::display::Display;

/// The number of terminal lines [`render`] writes.
pub const LINES: usize = DISPLAY_HEIGHT;

/// Renders the display as truecolor ANSI, one line per row.
pub fn render(display: &Display<'_>) -> String {
    let mut out = String::with_capacity(LINES * 16 * 24);
    for row in display.rows() {
        for pixel in row {
            let [red, green, blue] = pixel.to_rgb8();
            out.push_str(&format!("\x1b[38;2;{red};{green};{blue}m\u{2588}\u{2588}"));
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

/// Draws the display, moving the cursor back over the previous frame.
///
/// Repainting in place rather than scrolling is what makes an animation readable; the
/// first frame has nothing to move back over.
pub fn draw(display: &Display<'_>, first: &mut bool) {
    let mut stdout = std::io::stdout().lock();
    if *first {
        *first = false;
    } else {
        // Move up over the frame just drawn.
        let _ = write!(stdout, "\x1b[{LINES}A");
    }
    let _ = stdout.write_all(render(display).as_bytes());
    let _ = stdout.flush();
}
