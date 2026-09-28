use super::*;
use std::io::Cursor;

fn encoded(frame: &RemoteFrame) -> Vec<u8> {
    let mut output = Vec::new();
    RemoteFrameWriter::new(&mut output)
        .write_frame(frame)
        .unwrap();
    output
}
fn chunk(offset: u64, bytes: Vec<u8>, final_part: bool) -> RemoteFrame {
    RemoteFrame::Chunk {
        digest: Digest32::from_bytes([7; 32]),
        offset,
        final_part,
        bytes,
    }
}

#[test]
fn wire_header_is_explicit_and_all_maximum_sized_bodies_roundtrip() {
    assert_eq!(
        encoded(&RemoteFrame::Control(b"{}".to_vec())),
        b"MSHR\x01\x01\x00\x00\x00\x02{}"
    );
    for frame in [
        RemoteFrame::Control(vec![1; MAX_CONTROL_BYTES]),
        RemoteFrame::Manifest(vec![2; MAX_MANIFEST_BYTES]),
        chunk(
            MAX_CHUNK_BYTES - MAX_PART_BYTES as u64,
            vec![3; MAX_PART_BYTES],
            true,
        ),
    ] {
        let wire = encoded(&frame);
        let mut reader = RemoteFrameReader::new(Cursor::new(&wire));
        let decoded = reader.read_frame().unwrap().unwrap();
        assert_eq!(encoded(&decoded), wire);
        assert!(reader.read_frame().unwrap().is_none());
        assert!(reader.read_frame().is_err());
    }
}

#[test]
fn hostile_header_lengths_are_rejected_before_body_read_or_allocation() {
    for (kind, len) in [
        (0, 1),
        (4, 1),
        (1, 0),
        (1, 65_537),
        (2, 1_048_577),
        (3, 41),
        (3, 65_578),
        (2, u32::MAX),
    ] {
        let mut wire = b"MSHR\x01\x01\0\0\0\0".to_vec();
        wire[5] = kind;
        wire[6..10].copy_from_slice(&len.to_be_bytes());
        wire.extend_from_slice(b"must not read this body");
        let mut reader = RemoteFrameReader::new(Cursor::new(wire));
        assert!(reader.read_frame().is_err());
        assert_eq!(reader.input.position(), HEADER_BYTES as u64);
        assert!(reader.read_frame().is_err());
        assert_eq!(reader.input.position(), HEADER_BYTES as u64);
    }
}

#[test]
fn unknown_magic_version_and_chunk_metadata_poison_without_resynchronizing() {
    let valid = encoded(&chunk(0, vec![1], false));
    for index in [0, 4, HEADER_BYTES + 40] {
        let mut wire = valid.clone();
        wire[index] = 99;
        wire.extend_from_slice(&valid);
        let mut reader = RemoteFrameReader::new(Cursor::new(wire));
        assert!(reader.read_frame().is_err());
        let consumed = reader.input.position();
        assert!(reader.read_frame().is_err());
        assert_eq!(reader.input.position(), consumed);
    }
    let mut wire = valid;
    wire[HEADER_BYTES + 32..HEADER_BYTES + 40].copy_from_slice(&u64::MAX.to_be_bytes());
    let mut reader = RemoteFrameReader::new(Cursor::new(wire));
    assert!(reader.read_frame().is_err());
    assert_eq!(
        reader.input.position(),
        (HEADER_BYTES + CHUNK_PREFIX_BYTES) as u64
    );
}

#[test]
fn every_truncation_returns_no_frame_and_is_terminal() {
    for frame in [
        RemoteFrame::Control(b"task".to_vec()),
        RemoteFrame::Manifest(b"tree".to_vec()),
        chunk(0, b"bytes".to_vec(), true),
    ] {
        let wire = encoded(&frame);
        for end in 0..wire.len() {
            let mut reader = RemoteFrameReader::new(Cursor::new(&wire[..end]));
            if end == 0 {
                assert!(reader.read_frame().unwrap().is_none());
            } else {
                assert_eq!(
                    reader.read_frame().err().unwrap().kind(),
                    io::ErrorKind::UnexpectedEof
                );
            }
            assert!(reader.read_frame().is_err());
        }
    }
}

struct Fragmented {
    bytes: Cursor<Vec<u8>>,
    interrupted: bool,
}
impl Read for Fragmented {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.interrupted = !self.interrupted;
        if self.interrupted {
            Err(io::ErrorKind::Interrupted.into())
        } else {
            let len = buf.len().min(1);
            self.bytes.read(&mut buf[..len])
        }
    }
}
#[test]
fn fragmented_interrupted_reads_preserve_multiple_frame_boundaries() {
    let mut wire = encoded(&RemoteFrame::Control(b"one".to_vec()));
    wire.extend(encoded(&RemoteFrame::Control(b"two".to_vec())));
    let mut reader = RemoteFrameReader::new(Fragmented {
        bytes: Cursor::new(wire),
        interrupted: false,
    });
    for expected in [b"one", b"two"] {
        let Some(RemoteFrame::Control(bytes)) = reader.read_frame().unwrap() else {
            panic!("control expected")
        };
        assert_eq!(bytes, expected);
    }
    assert!(reader.read_frame().unwrap().is_none());
}

struct ReadFailure {
    bytes: Cursor<Vec<u8>>,
    remaining: usize,
    kind: io::ErrorKind,
}
impl Read for ReadFailure {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(self.kind.into());
        }
        let len = buf.len().min(self.remaining);
        let got = self.bytes.read(&mut buf[..len])?;
        self.remaining -= got;
        Ok(got)
    }
}
#[test]
fn timeout_or_would_block_after_partial_input_requires_a_new_connection() {
    for kind in [io::ErrorKind::TimedOut, io::ErrorKind::WouldBlock] {
        for remaining in [1, HEADER_BYTES + 1] {
            let input = ReadFailure {
                bytes: Cursor::new(encoded(&RemoteFrame::Control(b"task".to_vec()))),
                remaining,
                kind,
            };
            let mut reader = RemoteFrameReader::new(input);
            assert_eq!(reader.read_frame().err().unwrap().kind(), kind);
            reader.input.remaining = usize::MAX;
            assert!(reader.read_frame().is_err());
        }
    }
}

struct WriteFailure {
    bytes: Vec<u8>,
    remaining: usize,
    fail_flush: bool,
}
impl Write for WriteFailure {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let len = bytes.len().min(self.remaining).min(3);
        self.bytes.extend_from_slice(&bytes[..len]);
        self.remaining -= len;
        Ok(len)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn any_partial_write_or_flush_failure_is_terminal() {
    let frame = chunk(0, b"private bytes".to_vec(), true);
    let wire = encoded(&frame);
    for remaining in 0..=wire.len() {
        let mut writer = RemoteFrameWriter::new(WriteFailure {
            bytes: vec![],
            remaining,
            fail_flush: remaining == wire.len(),
        });
        assert!(writer.write_frame(&frame).is_err());
        let written = writer.output.bytes.len();
        writer.output.remaining = usize::MAX;
        writer.output.fail_flush = false;
        assert!(writer.write_frame(&frame).is_err());
        assert_eq!(writer.output.bytes.len(), written);
    }
}
#[test]
fn invalid_outbound_frames_write_nothing_and_do_not_corrupt_the_connection() {
    let mut writer = RemoteFrameWriter::new(Vec::new());
    for frame in [
        RemoteFrame::Control(vec![]),
        RemoteFrame::Control(vec![0; MAX_CONTROL_BYTES + 1]),
        RemoteFrame::Manifest(vec![0; MAX_MANIFEST_BYTES + 1]),
        chunk(0, vec![], false),
        chunk(u64::MAX, vec![1], false),
        chunk(0, vec![1; MAX_PART_BYTES + 1], false),
    ] {
        assert!(writer.write_frame(&frame).is_err());
        assert!(writer.output.is_empty());
    }
    writer
        .write_frame(&RemoteFrame::Control(b"valid".to_vec()))
        .unwrap();
    assert!(!writer.output.is_empty());
}

#[cfg(unix)]
#[test]
fn actual_stream_disconnect_preserves_cas_offset_and_reconnect_finishes_exact_content() {
    use crate::fleet::{
        RemoteAssignment, RemoteInputChunk, RemoteInputEntry, RemoteInputManifest,
        RemoteInputReceiver,
    };
    use mesh_cas::{Blake3, Cas, ContentDigest};
    use mesh_store::RecordDigest;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let root = std::env::temp_dir().join(format!("mesh-framed-cas-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let data: Vec<u8> = (0..MAX_PART_BYTES + 37).map(|n| (n % 251) as u8).collect();
    let digest = Blake3::digest_bytes(&data);
    let manifest = RemoteInputManifest::new(
        RecordDigest::from_bytes([1; 32]),
        vec![RemoteInputEntry::File {
            path: "result.bin".into(),
            executable: false,
            digest,
            chunks: vec![RemoteInputChunk {
                digest,
                bytes: data.len() as u64,
            }],
        }],
    )
    .unwrap();
    let assignment = RemoteAssignment {
        id: "assignment".into(),
        worker_key: "ab".repeat(32),
        input: manifest.input(),
        bundle: manifest.bundle(),
        lease_sequence: 1,
        lease_until_ms: 1000,
    };
    let cas = Cas::open(&root).unwrap();
    let manifest_wire = manifest.encoded().as_bytes().to_vec();
    let part = data[..MAX_PART_BYTES].to_vec();
    let (left, right) = UnixStream::pair().unwrap();
    left.set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    right
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let sender = std::thread::spawn(move || {
        let mut writer = RemoteFrameWriter::new(left);
        writer
            .write_frame(&RemoteFrame::Manifest(manifest_wire))
            .unwrap();
        writer
            .write_frame(&RemoteFrame::Chunk {
                digest,
                offset: 0,
                final_part: false,
                bytes: part,
            })
            .unwrap();
        let partial = encoded(&RemoteFrame::Chunk {
            digest,
            offset: MAX_PART_BYTES as u64,
            final_part: true,
            bytes: vec![0; 37],
        });
        writer
            .output
            .write_all(&partial[..partial.len() - 1])
            .unwrap();
    });
    let mut reader = RemoteFrameReader::new(right);
    let Some(RemoteFrame::Manifest(raw)) = reader.read_frame().unwrap() else {
        panic!("manifest expected")
    };
    let received = RemoteInputManifest::decode(
        std::str::from_utf8(&raw).unwrap(),
        assignment.input,
        assignment.bundle,
    )
    .unwrap();
    let mut receiver = RemoteInputReceiver::new(received, &assignment, &cas).unwrap();
    let Some(RemoteFrame::Chunk {
        digest,
        offset,
        final_part,
        bytes,
    }) = reader.read_frame().unwrap()
    else {
        panic!("chunk expected")
    };
    receiver.accept(digest, offset, &bytes, final_part).unwrap();
    assert!(reader.read_frame().is_err());
    sender.join().unwrap();
    assert_eq!(
        receiver.status(digest).unwrap(),
        (MAX_PART_BYTES as u64, false)
    );
    assert!(receiver.verify_complete().is_err());
    drop(receiver);
    drop(cas);
    // Reopen durable CAS and use its confirmed offset; never reuse the poisoned stream.
    let cas = Cas::open(&root).unwrap();
    let mut receiver = RemoteInputReceiver::new(manifest, &assignment, &cas).unwrap();
    let (left, right) = UnixStream::pair().unwrap();
    left.set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    right
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut writer = RemoteFrameWriter::new(left);
    writer
        .write_frame(&RemoteFrame::Chunk {
            digest,
            offset: MAX_PART_BYTES as u64,
            final_part: true,
            bytes: data[MAX_PART_BYTES..].to_vec(),
        })
        .unwrap();
    drop(writer);
    let mut reader = RemoteFrameReader::new(right);
    let Some(RemoteFrame::Chunk {
        digest,
        offset,
        final_part,
        bytes,
    }) = reader.read_frame().unwrap()
    else {
        panic!("chunk expected")
    };
    receiver.accept(digest, offset, &bytes, final_part).unwrap();
    assert_eq!(receiver.status(digest).unwrap(), (data.len() as u64, true));
    receiver.verify_complete().unwrap();
    assert!(reader.read_frame().unwrap().is_none());
    drop(receiver);
    drop(cas);
    std::fs::remove_dir_all(root).unwrap();
}
