//! Bounded synchronous framing for native broker streams. Frames carry data, never authority.
//! The owner supplies authenticated streams and deadlines; this module opens no endpoint/process.
use super::remote_input::{MAX_CHUNK_BYTES, MAX_MANIFEST_BYTES, MAX_PART_BYTES};
use mesh_cas::Digest32;
use std::io::{self, Read, Write};

const MAGIC: &[u8; 4] = b"MSHR";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 10;
const CHUNK_PREFIX_BYTES: usize = 41;
const MAX_CONTROL_BYTES: usize = 65_536;

/// One complete frame. No Debug implementation: bodies may contain private tasks or file content.
/// These are untrusted facts until native authentication, schema and assignment checks succeed.
pub enum RemoteFrame {
    /// Bounded canonical control bytes; the selected native operation must decode its own schema.
    Control(Vec<u8>),
    /// An encoded input manifest; decode against the independently expected input/bundle.
    Manifest(Vec<u8>),
    /// One contiguous chunk part. Existing CAS admission verifies membership, offsets and digest.
    Chunk {
        /// Declared content digest, not a path.
        digest: Digest32,
        /// Offset in the declared chunk.
        offset: u64,
        /// Whether these bytes complete the declared chunk.
        final_part: bool,
        /// At most 64 KiB, nonempty.
        bytes: Vec<u8>,
    },
}
impl RemoteFrame {
    fn header(&self) -> io::Result<[u8; HEADER_BYTES]> {
        let (kind, len) = match self {
            Self::Control(bytes) => (1, bytes.len()),
            Self::Manifest(bytes) => (2, bytes.len()),
            Self::Chunk { offset, bytes, .. } => {
                chunk_range(*offset, bytes.len())?;
                (3, CHUNK_PREFIX_BYTES + bytes.len())
            }
        };
        length(kind, len)?;
        let mut header = [0; HEADER_BYTES];
        header[..4].copy_from_slice(MAGIC);
        header[4] = VERSION;
        header[5] = kind;
        header[6..].copy_from_slice(&(len as u32).to_be_bytes());
        Ok(header)
    }
}

/// Reads one bounded frame at a time without prefetch or an accumulating queue.
/// Any truncated/invalid frame or I/O error permanently poisons this reader. A clean EOF is
/// terminal too. The broker must close the connection; it must not scan for a later magic header.
pub struct RemoteFrameReader<R> {
    input: R,
    ended: bool,
}
impl<R: Read> RemoteFrameReader<R> {
    /// Wrap an already-admitted native stream. Its owner must configure timeouts/cancellation.
    pub fn new(input: R) -> Self {
        Self {
            input,
            ended: false,
        }
    }

    /// Return only a whole frame, or None for EOF exactly between frames. No partial body escapes.
    /// TimedOut/WouldBlock poison the connection: retrying cannot reinterpret consumed bytes.
    pub fn read_frame(&mut self) -> io::Result<Option<RemoteFrame>> {
        if self.ended {
            return Err(invalid("remote-frame-connection-ended"));
        }
        self.ended = true;
        let mut header = [0; HEADER_BYTES];
        loop {
            match self.input.read(&mut header[..1]) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        self.input.read_exact(&mut header[1..])?;
        if &header[..4] != MAGIC || header[4] != VERSION {
            return Err(invalid("remote-frame-version"));
        }
        let kind = header[5];
        let len = u32::from_be_bytes(header[6..].try_into().unwrap()) as usize;
        length(kind, len)?; // Before allocating or reading any body bytes.
        let frame = if kind == 3 {
            let mut prefix = [0; CHUNK_PREFIX_BYTES];
            self.input.read_exact(&mut prefix)?;
            let offset = u64::from_be_bytes(prefix[32..40].try_into().unwrap());
            chunk_range(offset, len - CHUNK_PREFIX_BYTES)?;
            let final_part = match prefix[40] {
                0 => false,
                1 => true,
                _ => return Err(invalid("remote-frame-chunk-flags")),
            };
            RemoteFrame::Chunk {
                digest: Digest32::from_bytes(prefix[..32].try_into().unwrap()),
                offset,
                final_part,
                bytes: read_body(&mut self.input, len - CHUNK_PREFIX_BYTES)?,
            }
        } else {
            let bytes = read_body(&mut self.input, len)?;
            if kind == 1 {
                RemoteFrame::Control(bytes)
            } else {
                RemoteFrame::Manifest(bytes)
            }
        };
        self.ended = false;
        Ok(Some(frame))
    }
}

/// Writes one frame synchronously, with no unbounded queue or second body encoding allocation.
/// A write/flush failure poisons this writer. Flushed bytes are not a durable remote acknowledgment.
pub struct RemoteFrameWriter<W> {
    output: W,
    failed: bool,
}
impl<W: Write> RemoteFrameWriter<W> {
    /// Wrap an already-admitted native stream. The owner supplies deadlines and bounded stderr.
    pub fn new(output: W) -> Self {
        Self {
            output,
            failed: false,
        }
    }
    /// Validate all local bounds before writing. On partial I/O failure the owner must disconnect;
    /// a new connection must reconcile durable receipts, not automatically replay launch commands.
    pub fn write_frame(&mut self, frame: &RemoteFrame) -> io::Result<()> {
        if self.failed {
            return Err(invalid("remote-frame-connection-ended"));
        }
        let header = frame.header()?;
        self.failed = true;
        self.output.write_all(&header)?;
        match frame {
            RemoteFrame::Control(bytes) | RemoteFrame::Manifest(bytes) => {
                self.output.write_all(bytes)?
            }
            RemoteFrame::Chunk {
                digest,
                offset,
                final_part,
                bytes,
            } => {
                self.output.write_all(digest.as_bytes())?;
                self.output.write_all(&offset.to_be_bytes())?;
                self.output.write_all(&[u8::from(*final_part)])?;
                self.output.write_all(bytes)?;
            }
        }
        self.output.flush()?;
        self.failed = false;
        Ok(())
    }
}
fn length(kind: u8, len: usize) -> io::Result<()> {
    let valid = match kind {
        1 => (1..=MAX_CONTROL_BYTES).contains(&len),
        2 => (1..=MAX_MANIFEST_BYTES).contains(&len),
        3 => (CHUNK_PREFIX_BYTES + 1..=CHUNK_PREFIX_BYTES + MAX_PART_BYTES).contains(&len),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("remote-frame-kind-or-length"))
    }
}
fn chunk_range(offset: u64, len: usize) -> io::Result<()> {
    if len == 0
        || len > MAX_PART_BYTES
        || offset
            .checked_add(len as u64)
            .is_none_or(|end| end > MAX_CHUNK_BYTES)
    {
        Err(invalid("remote-frame-chunk-range"))
    } else {
        Ok(())
    }
}
fn read_body(input: &mut impl Read, len: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
