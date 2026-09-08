//! The binary memory-image format.
//!
//! This is the format MARIE.js writes from its "Download .bin" menu item:
//!
//! ```js
//! const array = new ArrayBuffer(sim.memory.length * 2);
//! const view = new DataView(array);
//! for (let i = 0; i < sim.memory.length; i++) {
//!     view.setInt16(i * 2, sim.memory[i], true);
//! }
//! ```
//!
//! Three details are worth stating plainly, because two of them are easy to assume
//! wrongly:
//!
//! - **Little-endian.** `DataView.setInt16` is *big*-endian by default; the trailing
//!   `true` selects little-endian. A reader that took the default would load every
//!   word byte-swapped.
//! - **Signed 16-bit words**, matching [`Value`](crate::Value).
//! - **The whole address space**, not just the program: MARIE.js writes
//!   `sim.memory.length` words, so a full image is always [`IMAGE_BYTES`] bytes, most
//!   of them zero.
//!
//! MARIE.js can only write this format — its file picker accepts `.mas` and `.mar` —
//! so reading one back is an addition rather than a compatibility concern. The bytes
//! are its bytes either way.

use thiserror::Error;

use crate::address::{MEMORY_WORD_COUNT, MemoryImage};

/// The size of a full memory image, in bytes.
pub const IMAGE_BYTES: usize = MEMORY_WORD_COUNT as usize * 2;

/// Why a byte sequence is not a memory image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ImageError {
    /// The byte count is odd, so the last word is incomplete.
    #[error("a memory image is a whole number of 16-bit words, but this is {0} bytes")]
    OddLength(usize),
    /// There are more words than the address space holds.
    #[error("an image of {words} words does not fit in the {MEMORY_WORD_COUNT}-word address space")]
    TooLarge {
        /// How many words the input held.
        words: usize,
    },
    /// A full image was required, but the input is a different size.
    #[error("a full memory image is exactly {IMAGE_BYTES} bytes, but this is {0}")]
    NotFullImage(usize),
}

/// Encodes words as little-endian 16-bit values.
///
/// Use [`encode_image`] for a full image; this is for a bare program.
pub fn encode_words(words: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(words.len() * 2);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

/// Encodes a full memory image, exactly as MARIE.js writes it.
pub fn encode_image(image: &MemoryImage) -> Vec<u8> {
    encode_words(image)
}

/// Decodes little-endian 16-bit words.
///
/// Accepts any whole number of words up to the size of the address space, so this
/// reads both a full image and a bare program.
///
/// # Errors
///
/// Returns [`ImageError::OddLength`] if the input does not divide into words, or
/// [`ImageError::TooLarge`] if there are more words than memory holds.
pub fn decode_words(bytes: &[u8]) -> Result<Vec<i16>, ImageError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(ImageError::OddLength(bytes.len()));
    }
    let words = bytes.len() / 2;
    if words > MEMORY_WORD_COUNT as usize {
        return Err(ImageError::TooLarge { words });
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    Ok(pairs.iter().copied().map(i16::from_le_bytes).collect())
}

/// Decodes a full memory image.
///
/// # Errors
///
/// Returns [`ImageError::NotFullImage`] unless the input is exactly [`IMAGE_BYTES`]
/// bytes. Use [`decode_words`] to accept a partial one.
pub fn decode_image(bytes: &[u8]) -> Result<MemoryImage, ImageError> {
    if bytes.len() != IMAGE_BYTES {
        return Err(ImageError::NotFullImage(bytes.len()));
    }
    let words = decode_words(bytes)?;
    let mut image = [0i16; MEMORY_WORD_COUNT as usize];
    image.copy_from_slice(&words);
    Ok(image)
}

/// Returns `true` if `bytes` is the size of a full memory image.
///
/// A caller uses this to tell a whole-memory dump from a program that should be loaded
/// at an origin.
pub const fn is_full_image(bytes: &[u8]) -> bool {
    bytes.len() == IMAGE_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_little_endian() {
        // The single most important assertion in this module: `Load 004` is 0x1004,
        // which MARIE.js writes low byte first. Reading it big-endian would give
        // 0x0410 — a `Jump` to the wrong place, with no error to say so.
        assert_eq!(encode_words(&[0x1004]), vec![0x04, 0x10]);
        assert_eq!(decode_words(&[0x04, 0x10]).unwrap(), vec![0x1004]);
    }

    #[test]
    fn negative_words_round_trip() {
        let words = [-1i16, i16::MIN, i16::MAX, 0];
        let bytes = encode_words(&words);
        assert_eq!(bytes[..2], [0xFF, 0xFF]);
        assert_eq!(decode_words(&bytes).unwrap(), words);
    }

    #[test]
    fn a_full_image_is_the_whole_address_space() {
        let mut image = [0i16; MEMORY_WORD_COUNT as usize];
        image[0] = 0x7000_u16 as i16;
        image[MEMORY_WORD_COUNT as usize - 1] = 0x0042;

        let bytes = encode_image(&image);
        assert_eq!(bytes.len(), IMAGE_BYTES);
        assert!(is_full_image(&bytes));
        assert_eq!(decode_image(&bytes).unwrap(), image);
    }

    #[test]
    fn a_partial_image_decodes_as_words() {
        let bytes = encode_words(&[1, 2, 3]);
        assert!(!is_full_image(&bytes));
        assert_eq!(decode_words(&bytes).unwrap(), vec![1, 2, 3]);
        // But it is not a full image.
        assert_eq!(decode_image(&bytes), Err(ImageError::NotFullImage(6)));
    }

    #[test]
    fn malformed_input_is_rejected_rather_than_truncated() {
        assert_eq!(decode_words(&[0x00]), Err(ImageError::OddLength(1)));
        let oversized = vec![0u8; IMAGE_BYTES + 2];
        assert_eq!(
            decode_words(&oversized),
            Err(ImageError::TooLarge {
                words: MEMORY_WORD_COUNT as usize + 1
            })
        );
        assert_eq!(decode_words(&[]).unwrap(), Vec::<i16>::new());
    }
}
