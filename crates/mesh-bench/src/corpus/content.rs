//! Turning a [`FileSpec`] into bytes, in chunks, without ever holding a file.
//!
//! W5's file is a gibibyte and W2's corpus is a hundred of them, so content
//! generation is a **streaming** operation: the caller supplies a sink and the
//! generator hands it bounded chunks. Nothing here allocates in proportion to
//! the file it is generating.
//!
//! The four profiles are byte behaviour, not file format — see
//! [`ContentKind`](super::plan::ContentKind). What each one is *for*:
//!
//! | Profile | Behaviour it models | Why a benchmark cares |
//! |---|---|---|
//! | `Text` | repeated tokens, line-structured | content-defined chunking finds boundaries; deltas are small |
//! | `Csv` | fixed columns, drifting values | line-aligned edits, high column redundancy |
//! | `Binary` | stable header, incompressible body | media and PDF: a header worth deduplicating, a body that is not |
//! | `Container` | maximum entropy throughout | zip-based office formats: one byte changes everything downstream |

use super::plan::{ContentKind, FileSpec};
use super::rng::SplitMix64;

/// The chunk size content is handed to a sink in.
///
/// 64 KiB: large enough that the per-call cost disappears, small enough that a
/// gibibyte file never needs a gibibyte of memory.
pub const CHUNK_BYTES: usize = 64 * 1024;

/// The token table `Text` is built from.
///
/// Written out rather than generated so that the corpus is byte-identical on
/// every machine and every future build, which is the entire deliverable.
const WORDS: [&str; 32] = [
    "actor",
    "append",
    "buffer",
    "canonical",
    "chunk",
    "commit",
    "context",
    "digest",
    "durable",
    "engine",
    "frontier",
    "hydrate",
    "index",
    "journal",
    "lattice",
    "ledger",
    "manifest",
    "materialize",
    "mesh",
    "object",
    "operation",
    "partition",
    "protocol",
    "publish",
    "replica",
    "review",
    "session",
    "shadow",
    "state",
    "stream",
    "verify",
    "workspace",
];

/// The header every `Binary` file starts with, before its incompressible body.
const BINARY_HEADER: &[u8; 16] = b"MESH-BENCH-ASSET";

/// Generates `file`'s bytes and hands them to `sink` in chunks.
///
/// The concatenation of every chunk is exactly `file.bytes` bytes long, and is a
/// pure function of `file.kind`, `file.stream` and `file.bytes`.
pub fn generate(file: &FileSpec, sink: &mut impl FnMut(&[u8])) {
    write_stream(file.kind, file.stream, file.bytes, sink);
}

/// Generates `bytes` bytes of `kind` from `stream`.
pub fn write_stream(kind: ContentKind, stream: u64, bytes: u64, sink: &mut impl FnMut(&[u8])) {
    let mut source = SplitMix64::new(stream);
    let mut remaining = bytes;
    let mut buffer = Vec::with_capacity(CHUNK_BYTES);
    let mut written = 0_u64;

    while remaining > 0 {
        let want = usize::try_from(remaining.min(CHUNK_BYTES as u64)).unwrap_or(CHUNK_BYTES);
        buffer.clear();
        match kind {
            ContentKind::Text => fill_text(&mut source, want, &mut buffer),
            ContentKind::Csv => fill_csv(&mut source, written, want, &mut buffer),
            ContentKind::Binary => fill_binary(&mut source, written, want, &mut buffer),
            ContentKind::Container => fill_random(&mut source, want, &mut buffer),
        }
        buffer.truncate(want);
        sink(&buffer);
        written += want as u64;
        remaining -= want as u64;
    }
}

/// Collects a whole file into memory. For tests and small corpora only.
#[must_use]
pub fn to_vec(file: &FileSpec) -> Vec<u8> {
    let mut out = Vec::with_capacity(usize::try_from(file.bytes).unwrap_or(0));
    generate(file, &mut |chunk| out.extend_from_slice(chunk));
    out
}

/// Words and newlines until the buffer is long enough.
fn fill_text(source: &mut SplitMix64, want: usize, buffer: &mut Vec<u8>) {
    let mut column = 0;
    while buffer.len() < want {
        let word = WORDS[(source.next_u64() % WORDS.len() as u64) as usize];
        buffer.extend_from_slice(word.as_bytes());
        column += 1;
        if column >= 9 {
            buffer.push(b'\n');
            column = 0;
        } else {
            buffer.push(b' ');
        }
    }
}

/// Fixed-column records. The row counter comes from the byte offset so a chunk
/// boundary never restarts the numbering.
fn fill_csv(source: &mut SplitMix64, written: u64, want: usize, buffer: &mut Vec<u8>) {
    if written == 0 {
        buffer.extend_from_slice(b"row,label,value,flag\n");
    }
    let mut row = written / 32;
    while buffer.len() < want {
        let label = WORDS[(source.next_u64() % WORDS.len() as u64) as usize];
        let value = source.next_u64() % 1_000_000;
        let flag = u8::from(source.next_u64() % 2 == 0);
        buffer.extend_from_slice(format!("{row},{label},{value},{flag}\n").as_bytes());
        row += 1;
    }
}

/// A stable header, then incompressible bytes.
fn fill_binary(source: &mut SplitMix64, written: u64, want: usize, buffer: &mut Vec<u8>) {
    if written == 0 {
        buffer.extend_from_slice(BINARY_HEADER);
    }
    fill_random(source, want, buffer);
}

/// Raw generator output.
fn fill_random(source: &mut SplitMix64, want: usize, buffer: &mut Vec<u8>) {
    while buffer.len() < want {
        buffer.extend_from_slice(&source.next_u64().to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: ContentKind, bytes: u64) -> FileSpec {
        FileSpec {
            path: "sample".to_owned(),
            bytes,
            kind,
            stream: 0x5eed_1234,
        }
    }

    const KINDS: [ContentKind; 4] = [
        ContentKind::Text,
        ContentKind::Csv,
        ContentKind::Binary,
        ContentKind::Container,
    ];

    #[test]
    fn every_profile_produces_exactly_the_requested_length() {
        for kind in KINDS {
            for bytes in [0_u64, 1, 7, 4096, CHUNK_BYTES as u64 + 13] {
                let generated = to_vec(&spec(kind, bytes));
                assert_eq!(
                    generated.len() as u64,
                    bytes,
                    "{} produced the wrong length for {bytes}",
                    kind.name()
                );
            }
        }
    }

    #[test]
    fn every_profile_is_reproducible() {
        for kind in KINDS {
            let file = spec(kind, 9_000);
            assert_eq!(to_vec(&file), to_vec(&file), "{} drifted", kind.name());
        }
    }

    #[test]
    fn a_different_stream_gives_different_bytes() {
        for kind in KINDS {
            let mut left = spec(kind, 4_096);
            let mut right = spec(kind, 4_096);
            left.stream = 1;
            right.stream = 2;
            assert_ne!(
                to_vec(&left),
                to_vec(&right),
                "{} ignored its stream",
                kind.name()
            );
        }
    }

    #[test]
    fn chunks_are_bounded_and_reassemble_to_the_whole_file() {
        // What this pins is the streaming contract, not equality between two
        // chunkings: every chunk stays inside `CHUNK_BYTES`, a file larger than
        // one chunk is actually delivered in several, and their concatenation
        // is the file. A generator that buffered the whole gibibyte and handed
        // it over in one call would fail this.
        for kind in KINDS {
            let file = spec(kind, CHUNK_BYTES as u64 * 2 + 517);
            let whole = to_vec(&file);
            let mut chunked = Vec::new();
            let mut sizes = Vec::new();
            generate(&file, &mut |chunk| {
                sizes.push(chunk.len());
                chunked.extend_from_slice(chunk);
            });
            assert_eq!(whole, chunked, "{} is not chunk-stable", kind.name());
            assert!(sizes.len() >= 3, "{} was not actually chunked", kind.name());
            assert!(
                sizes.iter().all(|size| *size <= CHUNK_BYTES),
                "a chunk exceeded the bound"
            );
        }
    }

    #[test]
    fn text_is_compressible_and_containers_are_not() {
        // A crude entropy proxy — distinct byte values over a fixed window —
        // is enough to show the two profiles are not the same generator with a
        // different label, which is the only claim the table in the module
        // comment makes.
        let text = to_vec(&spec(ContentKind::Text, 8_192));
        let container = to_vec(&spec(ContentKind::Container, 8_192));
        let distinct = |bytes: &[u8]| {
            bytes
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<u8>>()
                .len()
        };
        assert!(
            distinct(&text) < 64,
            "text used {} distinct byte values",
            distinct(&text)
        );
        assert!(
            distinct(&container) > 200,
            "a container profile used only {} distinct byte values",
            distinct(&container)
        );
    }

    #[test]
    fn binary_files_start_with_their_header() {
        let binary = to_vec(&spec(ContentKind::Binary, 512));
        assert!(binary.starts_with(BINARY_HEADER));
    }

    #[test]
    fn csv_files_start_with_their_header_row() {
        let csv = to_vec(&spec(ContentKind::Csv, 512));
        assert!(csv.starts_with(b"row,label,value,flag\n"));
        assert!(csv.iter().filter(|byte| **byte == b'\n').count() > 3);
    }

    #[test]
    fn text_is_line_structured() {
        let text = to_vec(&spec(ContentKind::Text, 4_096));
        assert!(text.iter().filter(|byte| **byte == b'\n').count() > 20);
    }

    #[test]
    fn a_zero_byte_file_is_empty_rather_than_a_header() {
        for kind in KINDS {
            assert!(to_vec(&spec(kind, 0)).is_empty(), "{}", kind.name());
        }
    }
}
