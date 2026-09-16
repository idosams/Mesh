//! A test double for the digest seam, and named as one.
//!
//! `mesh-materializer` ships no [`StateDigest`] implementation on purpose — `src/encode.rs` states
//! why, and `mesh-state` states the same thing about head identifiers at more length. The tests
//! still need *a* digest in order to exercise [`mesh_materializer::WorkspaceState::state_hash`] at
//! all, so this is one.
//!
//! **It is FNV-1a widened to 256 bits and it is not collision-resistant.** No assertion in this
//! crate's tests depends on it being: every equality claim about two materializations is made on
//! the canonical *bytes*, which is a stronger comparison than any hash, and this type is used only
//! to check that the hash is a function of those bytes. Reaching for it outside `tests/` would be a
//! protocol identity computed with a non-cryptographic digest, which is the failure the missing
//! default exists to prevent.

use mesh_materializer::{StateDigest, StateHash};

/// FNV-1a over 128 bits, folded into a 256-bit output. A test double.
pub struct TestDigest(u128);

impl StateDigest for TestDigest {
    fn start() -> Self {
        Self(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d)
    }

    fn absorb(&mut self, bytes: &[u8]) {
        let mut state = self.0;
        for byte in bytes {
            state ^= u128::from(*byte);
            state = state.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
        self.0 = state;
    }

    fn finish(self) -> StateHash {
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(&self.0.to_be_bytes());
        bytes[16..].copy_from_slice(&self.0.rotate_left(37).to_be_bytes());
        StateHash::from_bytes(bytes)
    }
}

/// The same digest taken over a buffer, so a test can check that streaming and buffering agree.
pub fn digest_of(bytes: &[u8]) -> StateHash {
    let mut digest = TestDigest::start();
    digest.absorb(bytes);
    digest.finish()
}
