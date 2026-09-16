//! Independent semantic statements shared with neighboring crates.
//!
//! Operation vocabulary ownership and exact type identity are tested through the direct
//! `mesh-operations` dependency in `operation_ownership.rs`. These two remaining source checks pin
//! prose-level semantic promises that are not represented by Rust type identity: causal ordering
//! in `mesh-state`, and the non-recursive meaning of a directory move in `mesh-operations`.

use std::fs;
use std::path::PathBuf;

fn source(crate_name: &str, file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", crate_name, "src", file]
        .iter()
        .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). This test pins a neighboring crate's documented semantic \
             promise; if the file moved, update this path and re-evaluate that promise.",
            path.display()
        )
    })
}

/// The causal order this crate applies is `mesh-state`'s. If that crate stops stating the rule, the
/// two are ordering the same causal set by two rules and nothing else would notice.
#[test]
fn mesh_state_still_orders_by_causal_depth_then_identifier() {
    let theirs = source("mesh-state", "advance.rs");
    assert!(
        theirs.contains("causal depth, then identifier"),
        "mesh-state no longer states the causal order rule that src/order.rs mirrors"
    );
    assert!(
        theirs.contains("Causal depth, then identifier — the crate's one ordering rule"),
        "mesh-state's ordering function moved or was renamed"
    );
    assert!(
        theirs.contains("Wall-clock time is not a tiebreaker"),
        "mesh-state no longer states that wall-clock time is not a tiebreaker"
    );
}

/// `mesh-operations` states that a directory move names the entry and never a descendant. That is
/// what makes the move rule in `src/apply.rs` one entry-map edit rather than a subtree walk.
#[test]
fn a_move_still_names_the_entry_and_never_a_descendant() {
    let theirs = source("mesh-operations", "operation.rs");
    assert!(
        theirs.contains("It does not name a descendant"),
        "mesh-operations no longer states that MoveEntry names the entry rather than a descendant"
    );
}
