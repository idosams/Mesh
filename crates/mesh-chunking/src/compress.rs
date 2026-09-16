//! The compression seam, and a Zstandard frame writer that is honest about its own limits.
//!
//! # Read this before believing anything about compression ratios
//!
//! Plan §6.2 says "Zstandard compression". This module produces **valid Zstandard frames** —
//! `zstd -d` decodes them, and `tests/zstd_interoperates.rs` proves that against the reference
//! command-line tool when it is installed — but it emits only the two block types that carry no
//! entropy coding:
//!
//! * `Raw_Block`, which stores the bytes verbatim, and
//! * `RLE_Block`, which stores one byte and a repeat count.
//!
//! **There is no Huffman coding, no FSE, and no match finder.** On a run of identical bytes the
//! output is three orders of magnitude smaller than the input; on anything else it is the input
//! plus about twelve bytes. A real Zstandard encoder is a separate, substantial piece of work and
//! it is not in this crate.
//!
//! This is written at the top of the file, in the loudest place available, because a module named
//! `compress` that silently does nothing is the exact shape of a false efficiency claim, and plan
//! §2.10 forbids one. What this module *does* buy is real:
//!
//! * the **seam** — every byte that reaches a store goes through [`Compression`], so replacing
//!   this implementation with a real encoder is a new enum variant and no call-site change;
//! * the **container** — chunks on disk are already in the format the plan commits to, so a later
//!   encoder upgrade is forward-compatible rather than a migration. A frame written today by
//!   [`Compression::Zstd`] and one written tomorrow by a full encoder are both read by the same
//!   decoder.
//!
//! # What the decoder accepts
//!
//! [`Compression::decompress`] reads `Raw_Block` and `RLE_Block` and **rejects** `Compressed_Block`
//! with [`CompressionError::EntropyCodedBlock`]. It is not a general Zstandard decoder and does not
//! pretend to be: a frame from `zstd -19` fails with a named error rather than with wrong bytes.

use core::fmt;

/// Zstandard's frame magic number.
const MAGIC: u32 = 0xFD2F_B528;

/// Frame_Header_Descriptor: an 8-byte Frame_Content_Size, no single-segment, no checksum, no
/// dictionary.
const FRAME_HEADER_DESCRIPTOR: u8 = 0b1100_0000;

/// Window_Descriptor for a 128 KiB window: exponent 7 (`windowLog` = 10 + 7), mantissa 0.
const WINDOW_DESCRIPTOR: u8 = 7 << 3;

/// The block ceiling the window above implies, and Zstandard's own absolute ceiling.
const BLOCK_MAXIMUM: usize = 128 * 1024;

/// `Raw_Block`.
const BLOCK_TYPE_RAW: u32 = 0;

/// `RLE_Block`.
const BLOCK_TYPE_RLE: u32 = 1;

/// `Compressed_Block` — never written here, and rejected on read.
const BLOCK_TYPE_COMPRESSED: u32 = 2;

/// `Reserved`, which the specification says must not appear.
const BLOCK_TYPE_RESERVED: u32 = 3;

/// How a chunk's bytes are framed on their way to storage.
///
/// The default is [`Compression::Stored`], deliberately: a codec that cannot compress should not be
/// switched on by default just because it is spelled the same as one that can. Callers who want
/// the Zstandard container — for forward compatibility, or because their content is run-heavy —
/// ask for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compression {
    /// The bytes, unframed. Zero overhead, zero reduction.
    #[default]
    Stored,
    /// A Zstandard frame built from `Raw_Block` and `RLE_Block`. See this module's header for
    /// exactly what that does and does not compress.
    Zstd,
}

impl Compression {
    /// Frame `input`.
    #[must_use]
    pub fn compress(self, input: &[u8]) -> Vec<u8> {
        match self {
            Self::Stored => input.to_vec(),
            Self::Zstd => zstd_frame(input),
        }
    }

    /// Recover the bytes `compress` was given.
    ///
    /// # Errors
    ///
    /// [`CompressionError`] when the input is not a frame this module can read — a wrong magic
    /// number, a truncated header or block, a reserved block type, or an entropy-coded block,
    /// which this module deliberately does not implement.
    pub fn decompress(self, input: &[u8]) -> Result<Vec<u8>, CompressionError> {
        match self {
            Self::Stored => Ok(input.to_vec()),
            Self::Zstd => zstd_decode(input),
        }
    }
}

/// The bytes of a Zstandard frame carrying `input`.
fn zstd_frame(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 32);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.push(FRAME_HEADER_DESCRIPTOR);
    out.push(WINDOW_DESCRIPTOR);
    out.extend_from_slice(&(input.len() as u64).to_le_bytes());

    if input.is_empty() {
        // A frame must carry at least one block, and a zero-length `Raw_Block` is the only way to
        // say "no content" without lying about the content size.
        push_block_header(&mut out, 0, BLOCK_TYPE_RAW, true);
        return out;
    }

    let mut offset = 0;
    while offset < input.len() {
        let end = (offset + BLOCK_MAXIMUM).min(input.len());
        let block = &input[offset..end];
        let last = end == input.len();

        let first = block[0];
        if block.len() > 1 && block.iter().all(|byte| *byte == first) {
            push_block_header(&mut out, block.len() as u32, BLOCK_TYPE_RLE, last);
            out.push(first);
        } else {
            push_block_header(&mut out, block.len() as u32, BLOCK_TYPE_RAW, last);
            out.extend_from_slice(block);
        }
        offset = end;
    }
    out
}

/// The 3-byte little-endian block header: last-block bit, 2-bit type, 21-bit size.
fn push_block_header(out: &mut Vec<u8>, size: u32, block_type: u32, last: bool) {
    let header = u32::from(last) | (block_type << 1) | (size << 3);
    out.extend_from_slice(&header.to_le_bytes()[..3]);
}

/// Decode a frame this module could have written.
fn zstd_decode(input: &[u8]) -> Result<Vec<u8>, CompressionError> {
    let mut cursor = 0usize;

    let magic_bytes: [u8; 4] = input
        .get(..4)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(CompressionError::Truncated { at: 0 })?;
    let magic = u32::from_le_bytes(magic_bytes);
    if magic != MAGIC {
        return Err(CompressionError::NotAZstandardFrame { found: magic });
    }
    cursor += 4;

    let descriptor = *input
        .get(cursor)
        .ok_or(CompressionError::Truncated { at: cursor })?;
    cursor += 1;

    let content_size_flag = descriptor >> 6;
    let single_segment = (descriptor >> 5) & 1 == 1;
    let checksum = (descriptor >> 2) & 1 == 1;
    let dictionary_id_flag = descriptor & 0b11;
    if (descriptor >> 3) & 1 == 1 {
        return Err(CompressionError::ReservedBitSet);
    }

    if !single_segment {
        // The window descriptor. Nothing this module emits back-references, so its value does not
        // change the decode; it is consumed so the cursor stays right.
        cursor += 1;
    }

    cursor += match dictionary_id_flag {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };

    let content_size_bytes = match content_size_flag {
        0 => usize::from(single_segment),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let declared = read_content_size(input, cursor, content_size_bytes)?;
    cursor += content_size_bytes;

    let mut out = Vec::new();
    loop {
        let header_bytes = input
            .get(cursor..cursor + 3)
            .ok_or(CompressionError::Truncated { at: cursor })?;
        let header = u32::from(header_bytes[0])
            | (u32::from(header_bytes[1]) << 8)
            | (u32::from(header_bytes[2]) << 16);
        cursor += 3;

        let last = header & 1 == 1;
        let block_type = (header >> 1) & 0b11;
        let size = (header >> 3) as usize;

        match block_type {
            BLOCK_TYPE_RAW => {
                let block = input
                    .get(cursor..cursor + size)
                    .ok_or(CompressionError::Truncated { at: cursor })?;
                out.extend_from_slice(block);
                cursor += size;
            }
            BLOCK_TYPE_RLE => {
                let byte = *input
                    .get(cursor)
                    .ok_or(CompressionError::Truncated { at: cursor })?;
                out.extend(std::iter::repeat_n(byte, size));
                cursor += 1;
            }
            BLOCK_TYPE_COMPRESSED => return Err(CompressionError::EntropyCodedBlock),
            BLOCK_TYPE_RESERVED => return Err(CompressionError::ReservedBlockType),
            // Unreachable: `block_type` is two bits.
            _ => return Err(CompressionError::ReservedBlockType),
        }

        if last {
            break;
        }
    }

    if checksum {
        cursor += 4;
        if cursor > input.len() {
            return Err(CompressionError::Truncated { at: input.len() });
        }
    }

    if let Some(declared) = declared {
        if declared != out.len() as u64 {
            return Err(CompressionError::ContentSizeDisagrees {
                declared,
                decoded: out.len() as u64,
            });
        }
    }

    Ok(out)
}

/// The `Frame_Content_Size` field, if present, with the specification's 256 offset on the 2-byte
/// form applied.
fn read_content_size(
    input: &[u8],
    at: usize,
    width: usize,
) -> Result<Option<u64>, CompressionError> {
    if width == 0 {
        return Ok(None);
    }
    let bytes = input
        .get(at..at + width)
        .ok_or(CompressionError::Truncated { at })?;
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= u64::from(*byte) << (8 * index);
    }
    if width == 2 {
        value += 256;
    }
    Ok(Some(value))
}

/// Why a frame could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionError {
    /// The magic number was not Zstandard's.
    NotAZstandardFrame {
        /// The four bytes found where the magic number should be, little-endian.
        found: u32,
    },
    /// The input ended inside a header or a block.
    Truncated {
        /// The byte offset the decoder had reached.
        at: usize,
    },
    /// The frame header's reserved bit was set, which the specification forbids.
    ReservedBitSet,
    /// The frame used the reserved block type.
    ReservedBlockType,
    /// The frame carried an entropy-coded block, which this module does not implement.
    EntropyCodedBlock,
    /// The frame's declared content size did not match what decoding produced.
    ContentSizeDisagrees {
        /// What the frame header said.
        declared: u64,
        /// What the blocks actually produced.
        decoded: u64,
    },
}

impl fmt::Display for CompressionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAZstandardFrame { found } => write!(
                formatter,
                "expected the Zstandard magic number {MAGIC:#010X}, found {found:#010X}"
            ),
            Self::Truncated { at } => {
                write!(
                    formatter,
                    "the frame ends inside a header or block at byte {at}"
                )
            }
            Self::ReservedBitSet => {
                formatter.write_str("the frame header's reserved bit is set, which is not a frame")
            }
            Self::ReservedBlockType => {
                formatter.write_str("the frame uses the reserved block type, which is not a frame")
            }
            Self::EntropyCodedBlock => formatter.write_str(
                "the frame carries an entropy-coded block; this crate writes only raw and \
                 run-length blocks and cannot decode Huffman or FSE, so a frame from a full \
                 Zstandard encoder needs one",
            ),
            Self::ContentSizeDisagrees { declared, decoded } => write!(
                formatter,
                "the frame declares {declared} bytes of content and decoded to {decoded}"
            ),
        }
    }
}

impl std::error::Error for CompressionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{pseudorandom_bytes, source_like_bytes};

    #[test]
    fn stored_is_the_identity() {
        let data = pseudorandom_bytes(5000, 21);
        assert_eq!(Compression::Stored.compress(&data), data);
        assert_eq!(Compression::Stored.decompress(&data), Ok(data));
    }

    #[test]
    fn stored_is_the_default() {
        assert_eq!(Compression::default(), Compression::Stored);
    }

    #[test]
    fn a_zstd_frame_round_trips() {
        for length in [
            0usize,
            1,
            2,
            3,
            1000,
            BLOCK_MAXIMUM - 1,
            BLOCK_MAXIMUM,
            BLOCK_MAXIMUM + 1,
            300_000,
        ] {
            let data = pseudorandom_bytes(length, 22);
            let frame = Compression::Zstd.compress(&data);
            assert_eq!(
                Compression::Zstd.decompress(&frame),
                Ok(data),
                "a {length}-byte input did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_zstd_frame_starts_with_the_magic_number() {
        let frame = Compression::Zstd.compress(b"content");
        assert_eq!(&frame[..4], &MAGIC.to_le_bytes());
    }

    #[test]
    fn a_run_becomes_a_run_length_block() {
        let run = vec![0xABu8; 100_000];
        let frame = Compression::Zstd.compress(&run);
        assert!(
            frame.len() < 32,
            "a 100 KiB run framed to {} bytes; the run-length block did not fire",
            frame.len()
        );
        assert_eq!(Compression::Zstd.decompress(&frame), Ok(run));
    }

    #[test]
    fn incompressible_content_costs_a_bounded_header() {
        // The honest statement of what this codec does to content it cannot help: it must never
        // grow the payload by more than the framing, and the framing must be small and bounded.
        let data = pseudorandom_bytes(100_000, 23);
        let frame = Compression::Zstd.compress(&data);
        let overhead = frame.len() - data.len();
        assert_eq!(
            overhead,
            14 + 3,
            "14 bytes of frame header and one 3-byte block header"
        );
    }

    #[test]
    fn source_like_content_is_not_claimed_to_shrink() {
        // Recorded as a test rather than as a comment: this codec does nothing for text. If a
        // future change makes it shrink text, this test fails and the module header — which says
        // it cannot — has to be corrected in the same commit.
        let data = source_like_bytes(50_000, 24);
        let frame = Compression::Zstd.compress(&data);
        assert!(frame.len() >= data.len());
    }

    #[test]
    fn a_wrong_magic_number_is_named() {
        let error = Compression::Zstd
            .decompress(b"not a frame at all")
            .unwrap_err();
        assert!(matches!(error, CompressionError::NotAZstandardFrame { .. }));
        assert!(error.to_string().contains("magic"));
    }

    #[test]
    fn a_truncated_frame_is_named() {
        let frame = Compression::Zstd.compress(&pseudorandom_bytes(5000, 25));
        for cut in [0usize, 3, 6, 13, 17, 2000] {
            let error = Compression::Zstd.decompress(&frame[..cut]).unwrap_err();
            assert!(
                matches!(
                    error,
                    CompressionError::Truncated { .. }
                        | CompressionError::NotAZstandardFrame { .. }
                ),
                "a frame cut at {cut} produced {error}"
            );
        }
    }

    #[test]
    fn an_entropy_coded_block_is_refused_by_name() {
        let mut frame = Compression::Zstd.compress(b"whatever");
        // Rewrite the single block header as a last `Compressed_Block`.
        let header = 1u32 | (BLOCK_TYPE_COMPRESSED << 1) | (8u32 << 3);
        frame[14..17].copy_from_slice(&header.to_le_bytes()[..3]);
        assert_eq!(
            Compression::Zstd.decompress(&frame),
            Err(CompressionError::EntropyCodedBlock)
        );
    }

    #[test]
    fn a_frame_whose_declared_size_lies_is_refused() {
        let mut frame = Compression::Zstd.compress(b"12345678");
        frame[6..14].copy_from_slice(&99u64.to_le_bytes());
        assert_eq!(
            Compression::Zstd.decompress(&frame),
            Err(CompressionError::ContentSizeDisagrees {
                declared: 99,
                decoded: 8
            })
        );
    }
}
