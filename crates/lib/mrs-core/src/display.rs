//! The memory-mapped display.
//!
//! MARIE.js maps a 16×16 colour display onto the top of memory, `0xF00`–`0xFFF`, one
//! word per pixel in row-major order. Its own label for the panel says it exactly:
//!
//! ```text
//! 16x16 display, 0xF00-0xFFF. Pixel: R[14-10] G[9-5] B[4-0]
//! ```
//!
//! A program draws by storing to those addresses; there is no instruction involved, so
//! nothing in the instruction set mentions it. That is why the geometry and the pixel
//! format live here with the rest of the memory map rather than in the VM.
//!
//! # Channel expansion
//!
//! Each channel is five bits and widens to eight by *scaling*, not shifting:
//!
//! ```js
//! const b = Math.round(((x & 0x1f) / 31.0) * 255.0);
//! ```
//!
//! `c << 3` is the usual shortcut and is wrong here — it maps full brightness to 248
//! rather than 255, so white comes out slightly grey and no pixel ever reaches the top
//! of the range. [`Rgb555::red`] and friends reproduce the rounding exactly.

use crate::address::MemoryAddress;

/// The first address of the display.
pub const DISPLAY_ORIGIN: MemoryAddress = MemoryAddress::new(0xF00);

/// Pixels across.
pub const DISPLAY_WIDTH: usize = 16;

/// Pixels down.
pub const DISPLAY_HEIGHT: usize = 16;

/// Words the display occupies, one per pixel.
pub const DISPLAY_WORDS: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT;

// The display must sit inside memory, and must be exactly the region documented above.
const _: () = assert!(DISPLAY_ORIGIN.value() as usize + DISPLAY_WORDS == 0x1000);

/// A pixel: five bits per channel, `R[14-10] G[9-5] B[4-0]`.
///
/// Bit 15 is unused. Any 16-bit word is a valid pixel, so this cannot fail to convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Rgb555(u16);

impl Rgb555 {
    /// Black.
    pub const BLACK: Self = Self(0);

    /// White: every channel at full brightness.
    pub const WHITE: Self = Self(0x7FFF);

    /// Reads a pixel from a memory word.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Returns the raw word.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// The five-bit red channel.
    pub const fn red5(self) -> u8 {
        ((self.0 >> 10) & 0x1F) as u8
    }

    /// The five-bit green channel.
    pub const fn green5(self) -> u8 {
        ((self.0 >> 5) & 0x1F) as u8
    }

    /// The five-bit blue channel.
    pub const fn blue5(self) -> u8 {
        (self.0 & 0x1F) as u8
    }

    /// Widens a five-bit channel to eight bits.
    ///
    /// This is `round(c / 31 * 255)` in integer arithmetic, matching MARIE.js.
    const fn widen(channel: u8) -> u8 {
        ((channel as u16 * 255 + 15) / 31) as u8
    }

    /// The eight-bit red channel.
    pub const fn red(self) -> u8 {
        Self::widen(self.red5())
    }

    /// The eight-bit green channel.
    pub const fn green(self) -> u8 {
        Self::widen(self.green5())
    }

    /// The eight-bit blue channel.
    pub const fn blue(self) -> u8 {
        Self::widen(self.blue5())
    }

    /// The pixel as eight-bit `[red, green, blue]`.
    pub const fn to_rgb8(self) -> [u8; 3] {
        [self.red(), self.green(), self.blue()]
    }

    /// Builds a pixel from five-bit channels, masking anything wider.
    pub const fn from_channels(red: u8, green: u8, blue: u8) -> Self {
        Self((((red & 0x1F) as u16) << 10) | (((green & 0x1F) as u16) << 5) | (blue & 0x1F) as u16)
    }

    /// Returns `true` if the pixel is black.
    pub const fn is_black(self) -> bool {
        // Bit 15 is not part of any channel, so a word of `0x8000` is still black.
        self.0 & 0x7FFF == 0
    }
}

/// Returns `true` if `address` is part of the display.
pub const fn contains(address: MemoryAddress) -> bool {
    address.value() >= DISPLAY_ORIGIN.value()
}

/// Returns the pixel index an address maps to, row-major, or `None` if it is not part
/// of the display.
pub const fn pixel_index(address: MemoryAddress) -> Option<usize> {
    if !contains(address) {
        return None;
    }
    Some((address.value() - DISPLAY_ORIGIN.value()) as usize)
}

/// Returns the address of the pixel at `(column, row)`.
///
/// MARIE.js indexes it as `0xF00 + 16 * row + column`.
pub const fn address_of(column: usize, row: usize) -> Option<MemoryAddress> {
    if column >= DISPLAY_WIDTH || row >= DISPLAY_HEIGHT {
        return None;
    }
    Some(MemoryAddress::new(
        DISPLAY_ORIGIN.value() + (row * DISPLAY_WIDTH + column) as u16,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_are_extracted_from_the_documented_bits() {
        // R[14-10] G[9-5] B[4-0].
        let pixel = Rgb555::from_bits(0b0_11111_00000_00000);
        assert_eq!((pixel.red5(), pixel.green5(), pixel.blue5()), (31, 0, 0));
        let pixel = Rgb555::from_bits(0b0_00000_11111_00000);
        assert_eq!((pixel.red5(), pixel.green5(), pixel.blue5()), (0, 31, 0));
        let pixel = Rgb555::from_bits(0b0_00000_00000_11111);
        assert_eq!((pixel.red5(), pixel.green5(), pixel.blue5()), (0, 0, 31));
    }

    #[test]
    fn channels_widen_by_scaling_rather_than_shifting() {
        // The assertion that keeps white white: `31 << 3` would be 248.
        assert_eq!(Rgb555::WHITE.to_rgb8(), [255, 255, 255]);
        assert_eq!(Rgb555::BLACK.to_rgb8(), [0, 0, 0]);

        // Every level matches `Math.round(c / 31 * 255)`.
        for channel in 0..=31u8 {
            let expected = ((f64::from(channel) / 31.0) * 255.0).round() as u8;
            let pixel = Rgb555::from_channels(channel, channel, channel);
            assert_eq!(pixel.red(), expected, "channel {channel}");
            assert_eq!(pixel.to_rgb8(), [expected; 3]);
        }
    }

    #[test]
    fn channels_round_trip() {
        for red in 0..=31u8 {
            for green in [0u8, 7, 16, 31] {
                let pixel = Rgb555::from_channels(red, green, 31 - red);
                assert_eq!(
                    (pixel.red5(), pixel.green5(), pixel.blue5()),
                    (red, green, 31 - red)
                );
            }
        }
    }

    #[test]
    fn the_display_covers_the_top_of_memory() {
        assert_eq!(DISPLAY_ORIGIN.value(), 0xF00);
        assert_eq!(DISPLAY_WORDS, 256);
        assert!(contains(MemoryAddress::new(0xF00)));
        assert!(contains(MemoryAddress::MAX));
        assert!(!contains(MemoryAddress::new(0xEFF)));
        assert_eq!(pixel_index(MemoryAddress::new(0xEFF)), None);
        assert_eq!(pixel_index(MemoryAddress::new(0xF00)), Some(0));
        assert_eq!(pixel_index(MemoryAddress::MAX), Some(255));
    }

    #[test]
    fn addresses_are_row_major() {
        // MARIE.js: `0xF00 + 16 * row + column`.
        assert_eq!(address_of(0, 0).unwrap().value(), 0xF00);
        assert_eq!(address_of(1, 0).unwrap().value(), 0xF01);
        assert_eq!(address_of(0, 1).unwrap().value(), 0xF10);
        assert_eq!(address_of(15, 15).unwrap().value(), 0xFFF);
        assert_eq!(address_of(16, 0), None);
        assert_eq!(address_of(0, 16), None);

        // Indices and addresses agree in both directions.
        for row in 0..DISPLAY_HEIGHT {
            for column in 0..DISPLAY_WIDTH {
                let address = address_of(column, row).unwrap();
                assert_eq!(pixel_index(address), Some(row * DISPLAY_WIDTH + column));
            }
        }
    }

    #[test]
    fn the_unused_high_bit_is_ignored() {
        assert!(Rgb555::from_bits(0x8000).is_black());
        assert_eq!(Rgb555::from_bits(0xFFFF).to_rgb8(), [255, 255, 255]);
    }
}
