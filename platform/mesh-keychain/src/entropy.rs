//! Where a fresh seed comes from, and what this crate is willing to claim about it.

use std::io::Read;

use mesh_crypto::CustodyError;

/// The length of an Ed25519 seed.
pub(crate) const SEED_BYTES: usize = 32;

/// The operating system's cryptographic random source, on the one family of platforms this crate
/// has a source for.
#[cfg(unix)]
const OS_RANDOM_SOURCE: &str = "/dev/urandom";

/// Fill `buffer` from the operating system's cryptographic random source.
///
/// `/dev/urandom` on Unix, and nothing anywhere else. That is a deliberate refusal rather than a
/// gap: the alternatives are a third-party crate, which ADR-0014 admits only under five conditions
/// and a named reviewer, or a hand-rolled generator, which is the one category of cryptographic
/// code this repository has already decided twice it will not write. A platform this cannot read
/// gets [`CustodyError::BackendUnavailable`] and no key, which is the correct answer — a seed from
/// a source nobody chose is worse than no seed.
///
/// `/dev/urandom` is the right device on both platforms this compiles for. On Linux it is the
/// same CSPRNG as `getrandom(2)` once the pool is initialised, and on Darwin it is a
/// Fortuna-family generator seeded by the kernel; neither blocks and neither is the
/// `/dev/random`-blocks-forever folklore, which has not been true on Linux since 5.6.
///
/// **How it knows it opened that device rather than something wearing its name.** It checks, and
/// the check is [`read_verified_source`] below. `File::open` follows symbolic links and resolves
/// the path in whatever mount namespace this process is in, so the name alone is not evidence.
///
/// # Errors
///
/// [`CustodyError::BackendUnavailable`] when the device cannot be opened, is not a character
/// device, or cannot deliver the full request. A short read is a failure, never a partially seeded
/// key. It is **not** an error for the source to be a character device other than `/dev/urandom` —
/// see [`read_verified_source`] for what that costs and why the check stops there.
pub(crate) fn fill_from_os(buffer: &mut [u8; SEED_BYTES]) -> Result<(), CustodyError> {
    #[cfg(unix)]
    {
        read_verified_source(OS_RANDOM_SOURCE, buffer)
    }
    #[cfg(not(unix))]
    {
        let _ = buffer;
        Err(CustodyError::BackendUnavailable)
    }
}

/// Open `path`, refuse it unless it is a character device, and fill `buffer` from it.
///
/// # What the check proves, and what it does not
///
/// It proves that the thing read is a **character device** — a kernel device node — rather than a
/// regular file, a directory, a pipe or a socket. It does **not** prove the device is
/// `/dev/urandom`, and the cheapest counter-example is not `mknod`: it is a symbolic link.
/// `ln -sf /dev/zero /dev/urandom` needs exactly the same privilege as planting a regular file —
/// writing one path — and `/dev/zero` **is** a character device, so this check accepts it and every
/// seed afterwards is thirty-two zero bytes. That is measured, not reasoned:
/// [`tests::the_check_accepts_any_character_device_including_one_that_returns_a_constant`] asserts
/// the acceptance, so the limit of this guard cannot be forgotten while the guard is read as a
/// control. On the host that ran it, `/dev/urandom` is character device 17:1 and `/dev/zero` is
/// 3:3; distinguishing them means pinning `rdev`, which is a per-platform number this crate has no
/// documented source for on Darwin.
///
/// What the check does remove is the **regular-file** half: replace the device node with an
/// ordinary file of constant bytes and every actor key minted afterwards is predictable to whoever
/// wrote it, and nothing today would report it — the crate's own smoke test catches a source that
/// returns the *same* bytes twice, which a file long enough to serve two distinct reads does not.
/// That is one attack shape closed and one left open, and the guard is worth exactly that much.
///
/// The syscall with no filesystem dependency to subvert is `getrandom(2)`, and reaching it needs
/// either `unsafe` or a third-party crate. `docs/threat-model.md` G27 records that nothing in this
/// repository routes a new `unsafe` block to the justification and Miri obligations two contracts
/// already state, so the first one in the crate that holds a secret is not a thing to add here.
/// This check is the mitigation available without either.
///
/// **The check is on the descriptor, not on the path.** [`std::fs::File::metadata`] is `fstat` on
/// the handle already open, so there is no window between the check and the use — a second `stat`
/// of `path` would be exactly the time-of-check/time-of-use race the attack needs.
#[cfg(unix)]
fn read_verified_source(path: &str, buffer: &mut [u8; SEED_BYTES]) -> Result<(), CustodyError> {
    use std::os::unix::fs::FileTypeExt as _;

    let mut device = std::fs::File::open(path).map_err(|_| CustodyError::BackendUnavailable)?;
    let opened = device
        .metadata()
        .map_err(|_| CustodyError::BackendUnavailable)?;
    if !opened.file_type().is_char_device() {
        // Same rule as an unreadable source, and for the same reason: a seed from a source nobody
        // chose is worse than no seed. The error deliberately carries no path — `key_isolation.rs`
        // asserts that no diagnostic this crate renders quotes one.
        return Err(CustodyError::BackendUnavailable);
    }
    device
        .read_exact(buffer)
        .map_err(|_| CustodyError::BackendUnavailable)
}

/// Overwrite a seed buffer once it has been consumed.
///
/// **This is an approximation and is documented as one.** A plain loop of stores into a buffer the
/// compiler can prove is dead is a loop the compiler may delete; the guaranteed spelling is
/// `ptr::write_volatile`, which needs `unsafe`, and this crate has none — `docs/threat-model.md`
/// G27 records that nothing in this repository routes a new `unsafe` block to the justification
/// and Miri obligations two contracts already state, so adding one here would add an unreviewed
/// block to the one crate that holds a secret.
///
/// What makes the loop survive in practice is the fence: a `SeqCst` compiler fence after the
/// stores means the optimiser may not treat them as unobservable and sink them past this point.
/// It is a strong hint rather than a guarantee, and the honest ordering of the two controls is
/// that `SigningKey: ZeroizeOnDrop` is the real one and this is defence in depth over the copy
/// that never reached it.
pub(crate) fn scrub(buffer: &mut [u8; SEED_BYTES]) {
    for byte in buffer.iter_mut() {
        *byte = 0;
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two draws differ, and neither is the all-zero buffer the failure mode looks like.
    ///
    /// A statistical smoke test, stated as one: it detects a source that returns a constant or
    /// never writes, which is what a broken wiring actually looks like. It is not evidence about
    /// the distribution, and this crate makes no claim about that — the claim is delegated to the
    /// operating system and named in the doc comment above.
    #[test]
    fn two_draws_from_the_os_differ_and_neither_is_empty() {
        let mut first = [0u8; SEED_BYTES];
        let mut second = [0u8; SEED_BYTES];
        fill_from_os(&mut first).expect("this test runs on a unix host");
        fill_from_os(&mut second).expect("this test runs on a unix host");
        assert_ne!(first, [0u8; SEED_BYTES]);
        assert_ne!(second, [0u8; SEED_BYTES]);
        assert_ne!(first, second);
    }

    #[test]
    fn scrubbing_clears_the_buffer() {
        let mut buffer = [0xabu8; SEED_BYTES];
        scrub(&mut buffer);
        assert_eq!(buffer, [0u8; SEED_BYTES]);
    }

    /// **The attack.** An adversary who can write `/dev` replaces the device node with a regular
    /// file of bytes it chose, and every key minted afterwards is predictable to it.
    ///
    /// Driven against a file standing in for the device rather than against `/dev` itself, which
    /// is why the path is an argument to the private helper: the test needs no privilege and
    /// changes nothing outside its own temporary directory.
    #[cfg(unix)]
    #[test]
    fn a_regular_file_standing_in_for_the_device_is_refused() {
        let planted = temporary_path("regular-file");
        // Long enough to serve the read, so the refusal is the device-type check and not a short
        // read — the constant is exactly what the existing smoke test above cannot see.
        std::fs::write(&planted, [0x41u8; SEED_BYTES * 4]).expect("a temporary file");

        let mut buffer = [0u8; SEED_BYTES];
        let outcome = read_verified_source(
            planted.to_str().expect("a utf-8 temporary path"),
            &mut buffer,
        );

        std::fs::remove_file(&planted).expect("the temporary file is removed");
        assert_eq!(outcome, Err(CustodyError::BackendUnavailable));
        assert_eq!(buffer, [0u8; SEED_BYTES], "a refused source filled nothing");
    }

    /// A directory is not a character device either, and neither is a path that is not there.
    #[cfg(unix)]
    #[test]
    fn nothing_that_is_not_a_character_device_is_read_from() {
        let directory = temporary_path("directory");
        std::fs::create_dir_all(&directory).expect("a temporary directory");

        let mut buffer = [0u8; SEED_BYTES];
        let outcome = read_verified_source(
            directory.to_str().expect("a utf-8 temporary path"),
            &mut buffer,
        );
        std::fs::remove_dir(&directory).expect("the temporary directory is removed");
        assert_eq!(outcome, Err(CustodyError::BackendUnavailable));

        assert_eq!(
            read_verified_source("/nonexistent/mesh-keychain/urandom", &mut buffer),
            Err(CustodyError::BackendUnavailable)
        );
    }

    /// **The limit of the check, measured rather than asserted.**
    ///
    /// `/dev/zero` is a character device, so `is_char_device` accepts it and the seed is thirty-two
    /// zero bytes. The whole attack is `ln -sf /dev/zero /dev/urandom`, which needs no privilege
    /// beyond writing the one path a regular file would have needed. This test asserts the guard
    /// **lets it through**, on the same discipline as
    /// `crates/mesh-crypto/tests/self_reported_backend.rs`: a fix that closes it — pinning `rdev`,
    /// or `getrandom(2)` once G27 routes the `unsafe` — turns this test red and forces the doc
    /// comment above to be rewritten with it, instead of leaving a guard that reads stronger than
    /// it is.
    #[cfg(unix)]
    #[test]
    fn the_check_accepts_any_character_device_including_one_that_returns_a_constant() {
        let mut buffer = [0xffu8; SEED_BYTES];
        assert_eq!(read_verified_source("/dev/zero", &mut buffer), Ok(()));
        assert_eq!(
            buffer, [0u8; SEED_BYTES],
            "the device type is all this guard checks, and a constant source passes it"
        );
    }

    /// The real device passes the same check, so the refusal above is about the device type and
    /// not about the helper refusing everything.
    #[cfg(unix)]
    #[test]
    fn the_real_source_is_a_character_device_on_this_host() {
        let mut buffer = [0u8; SEED_BYTES];
        assert_eq!(read_verified_source(OS_RANDOM_SOURCE, &mut buffer), Ok(()));
        assert_ne!(buffer, [0u8; SEED_BYTES]);
    }

    /// A unique path under the system temporary directory, named for the case that uses it.
    #[cfg(unix)]
    fn temporary_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mesh-keychain-entropy-{}-{label}",
            std::process::id()
        ))
    }
}
