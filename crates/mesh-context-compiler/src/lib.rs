//! Bounded, versioned context retrieval. No unbounded read reaches a model call.
//!
//! **Placeholder.** This crate is scaffolded so the workspace builds, lints and
//! tests green from day one; the implementation lands under **E15 context ledger**. Run
//! See the public roadmap and issue tracker for the work that fills it.
//!
//! The dependency rules in plan §8.3 apply to this crate and are checked in CI —
//! add a dependency edge only together with its justification in the ownership map.

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-context-compiler";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-context-compiler");
    }
}
