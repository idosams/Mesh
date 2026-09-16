//! Build facts baked in by `build.rs`.
//!
//! Read at run time, decided at compile time — the only honest way to report a
//! build profile, since a binary cannot look up how it was compiled.

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

use crate::schema::BuildProfile;

/// The build profile of this binary, as a schema value.
pub fn build_profile() -> BuildProfile {
    BuildProfile {
        profile: PROFILE.to_owned(),
        opt_level: OPT_LEVEL.to_owned(),
        debug_info: DEBUG_INFO.to_owned(),
        rustc_version: RUSTC_VERSION.to_owned(),
        target_triple: TARGET_TRIPLE.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_profile_is_populated() {
        let build = build_profile();
        assert!(!build.profile.is_empty());
        assert!(build.rustc_version.starts_with("rustc "));
        assert!(!build.target_triple.is_empty());
        assert!(!build.opt_level.is_empty());
    }

    #[test]
    fn the_profile_comes_from_cargo() {
        // Guards the wiring itself: Cargo only ever sets `debug` or `release`
        // here, so anything else means the constant is being invented.
        assert!(
            matches!(PROFILE, "debug" | "release"),
            "unexpected profile `{PROFILE}`"
        );
    }
}
