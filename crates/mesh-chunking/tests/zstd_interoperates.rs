//! The frames this crate writes, decoded by the reference Zstandard implementation.
//!
//! # Why this test exists and what it is worth
//!
//! `Compression::Zstd` claims to emit *valid Zstandard frames*. Round-tripping through this
//! crate's own decoder cannot check that claim: two halves of the same misunderstanding agree
//! perfectly. The only witness that settles it is a decoder nobody here wrote.
//!
//! So when the `zstd` command-line tool is on `PATH`, this test writes every frame shape the
//! encoder can produce — empty, one byte, a run that becomes an `RLE_Block`, incompressible data
//! that becomes `Raw_Block`s, and content past the 128 KiB block ceiling that becomes several — and
//! checks that `zstd -d` returns the original bytes.
//!
//! # When the tool is absent
//!
//! The test **skips**, and says so. That is a real hole and it is stated rather than hidden: on a
//! machine without `zstd` this file proves nothing, and the claim rests on the run recorded in the
//! pull request. Making it a hard failure would make the suite depend on a tool the repository does
//! not vendor and does not install, which trades a known gap for an unrelated one.

use std::io::Write;
use std::process::{Command, Stdio};

use mesh_chunking::testing::{pseudorandom_bytes, source_like_bytes};
use mesh_chunking::Compression;

/// Whether the reference tool is available.
fn zstd_available() -> bool {
    Command::new("zstd")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// `zstd -d` over `frame`, or the reason it could not run.
fn reference_decode(frame: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = Command::new("zstd")
        .args(["-d", "-c", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot spawn zstd: {error}"))?;

    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(frame)
        .map_err(|error| format!("cannot write the frame to zstd: {error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("zstd did not finish: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "zstd exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

#[test]
fn the_reference_zstandard_decoder_reads_every_frame_this_crate_writes() {
    if !zstd_available() {
        eprintln!(
            "SKIPPED: the `zstd` command-line tool is not on PATH, so nothing independent checked \
             the frames this crate writes. Install it, or read the run pasted in the pull request \
             that added this file."
        );
        return;
    }

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("one byte", vec![7]),
        ("a short run", vec![0xEE; 64]),
        ("a run past the block ceiling", vec![0xAB; 300_000]),
        ("incompressible", pseudorandom_bytes(300_000, 301)),
        ("source-like text", source_like_bytes(50_000, 302)),
        ("exactly one block", pseudorandom_bytes(128 * 1024, 303)),
        (
            "one byte past a block",
            pseudorandom_bytes(128 * 1024 + 1, 304),
        ),
    ];

    for (name, data) in cases {
        let frame = Compression::Zstd.compress(&data);
        let decoded = reference_decode(&frame).unwrap_or_else(|error| {
            panic!("the reference decoder rejected the `{name}` frame: {error}")
        });
        assert_eq!(
            decoded,
            data,
            "the reference decoder returned {} bytes for the `{name}` frame and the input was {}",
            decoded.len(),
            data.len()
        );
    }
}

#[test]
fn this_crate_refuses_a_frame_the_reference_encoder_produced() {
    // The other direction, and the honest one: `zstd -19` produces entropy-coded blocks, this
    // crate cannot read them, and it must say so by name rather than return wrong bytes. A decoder
    // that silently mis-decodes is worse than one that refuses.
    if !zstd_available() {
        eprintln!("SKIPPED: the `zstd` command-line tool is not on PATH.");
        return;
    }

    let data = source_like_bytes(200_000, 305);
    let mut child = Command::new("zstd")
        .args(["-19", "-c", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("zstd is available");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&data)
        .expect("the input is written");
    let output = child.wait_with_output().expect("zstd finishes");
    assert!(output.status.success(), "zstd -19 failed");

    let error = Compression::Zstd
        .decompress(&output.stdout)
        .expect_err("a real Zstandard frame must not decode here");
    assert!(
        error.to_string().contains("entropy-coded"),
        "the refusal must name the reason; it said: {error}"
    );
}
