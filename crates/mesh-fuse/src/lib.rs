//! The Linux FUSE adapter — plan §7.2, task `01KZC2ZTN3YXE6NM270T001RAK`.
//!
//! # One sentence this crate is built around
//!
//! **A FUSE backend is a decision layer the kernel drives, and the decisions are separable from
//! the driving.** [`FuseAdapter`] is the decision layer: it implements
//! [`mesh_materializer::WorkspaceAdapter`] in full and is graded by
//! [`mesh_materializer::run_conformance`] through the same entry point every other backend goes
//! through. [`kernel`] is the shape of the driving: the request stream, the reply cache a retry
//! needs, and the transfer loop a short write needs.
//!
//! # What this crate is not, stated first because it is what a reader most needs to know
//!
//! **There is no mount here.** No `/dev/fuse`, no `libfuse`, no `fuser`. Mounting needs the
//! `fuser` crate, and `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`
//! narrows the lockfile fence to admit **an audited cryptographic dependency** and nothing else. A
//! filesystem dependency carrying a C library is outside that admission, so taking it is an
//! escalation rather than a decision a lane makes on its own. The case for it is written out in
//! this crate's `Cargo.toml` and filed as `01KZHAPMMK7YGF9AJ8QGPV3EP7` — filed rather than assumed,
//! which is the whole point of an escalation.
//!
//! Three of the five acceptance criteria of `01KZC2ZTN3YXE6NM270T001RAK` therefore do not close
//! here, and the PR body names them one by one rather than leaving a reader to infer it from an
//! absence.
//!
//! # What it is
//!
//! | Module | What it decides |
//! |---|---|
//! | [`tree`] | An object is an inode number and a generation, because that is the pair the kernel caches and the pair a daemon has to be able to invalidate. One lock per actor's state. |
//! | [`view`] | A mountpoint. Two mounts of one actor are two views onto one tree; handles are per view. |
//! | [`adapter`] | Seventeen capabilities, including exact file length and the one the folder-watching fallback honestly refuses: a FUSE session is handed `open`, `flush`, `fsync` and `release` by the kernel, so [`mesh_materializer::AdapterCapability::ObserveDurableBoundary`] is a claim here rather than a guess. |
//! | [`errno`] | The Linux error-number table `tests/compatibility/adapter/v0/README.md` §5 leaves to each backend. |
//! | [`kernel`] | The retry. A re-sent request is answered from the reply cache and never applied twice; a short transfer is completed by re-issuing at the advanced offset. |
//!
//! # The conformance grade, as a number
//!
//! 131 cases: **70 pass, 0 fail, 61 unsupported** — conformant. `tests/conformance.rs` asserts the
//! whole tally family by family rather than printing it, because a report nobody compares against
//! anything can quietly become "everything unsupported" and still read green.
//!
//! Against the folder-watching fallback's 68 pass / 1 fail / 62 unsupported on APFS, the two
//! differences are the whole of what changes when a backend is handed the kernel's own events:
//! `BND` moves from `unsupported` to two passes, and `ORD` loses the case-folding failure that
//! belonged to the volume rather than to the backend.
//!
//! # Nothing here reads a clock, a file, a socket or the environment
//!
//! Ordering at this seam is [`mesh_materializer::EventSequence`], minted per view.
//! `tests/no_ambient_io.rs` is what says so.

#![forbid(unsafe_code)]

pub mod adapter;
pub mod errno;
pub mod kernel;
pub mod tree;
pub mod view;

pub use crate::adapter::{FuseAdapter, ADAPTER_NAME, DECLARED};
pub use crate::errno::{errno_of, Errno};
pub use crate::kernel::{read_through, write_through, KernelOp, Reply, RequestGate, Transfer};
pub use crate::tree::{Tree, DEFAULT_MAX_WRITE};
pub use crate::view::FuseView;

/// The crate's name, so the placeholder's one verifiable behaviour survives its replacement.
pub const CRATE_NAME: &str = "mesh-fuse";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-fuse");
    }

    #[test]
    fn the_adapter_name_is_the_one_the_report_carries() {
        assert_eq!(ADAPTER_NAME, "mesh-fuse/1");
    }
}
