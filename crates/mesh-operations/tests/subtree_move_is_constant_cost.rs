//! Moving a directory costs one operation and a fixed number of bytes, whatever is under it.
//!
//! # Why this is a vocabulary test and not a performance test
//!
//! Plan §11 publishes a budget: **one million descendants moved in under 100 ms, under 10 KiB of
//! metadata for a subtree move.** A vocabulary that spelled a move as "unlink every descendant and
//! relink it" fails that budget by construction, and no amount of implementation work downstream
//! recovers it. So the property has to be true here, in the shape of the operation, before anyone
//! writes a materializer.
//!
//! What is measured below, at four subtree sizes up to a million nodes:
//!
//! * the move is **one** operation, and its canonical encoding is the **same length** at every
//!   size — so the metadata cost is constant, not merely sublinear;
//! * that length is far under 10 KiB;
//! * applying it to a directory model touches a **fixed number** of directory entries, again at
//!   every size;
//! * every descendant is still reachable afterwards, at its new path — the move really moved the
//!   subtree rather than merely not mentioning it.
//!
//! # What is NOT established here, stated plainly
//!
//! The model below is a test fixture, not `mesh-materializer`. It shows that the *operation* does
//! not require touching descendants; it does not show that the materializer will not. The 100 ms
//! wall-clock half of the budget belongs to the materializer and the benchmark harness, and the
//! timing assertion here is a floor rather than the measurement — it says the metadata plane is
//! nowhere near the budget, not that the whole path meets it.

use std::time::Instant;

use mesh_operations::{encode_canonical, NormalizedName, ObjectId, Operation, OperationKind};

/// The published metadata budget for a subtree move.
const METADATA_BUDGET_BYTES: usize = 10 * 1024;

/// The published wall-clock budget, used here only as a very loose ceiling on the metadata plane.
const WALL_CLOCK_CEILING_MILLIS: u128 = 100;

/// A directory model: for each directory, the names it binds and the child object each name binds
/// to. This is the shape the vocabulary assumes — a directory version binds a name to a **child
/// object identifier** — and the reason a subtree move never has to name a descendant.
struct DirectoryModel {
    /// `children[directory] = [(name, child)]`.
    children: Vec<Vec<(String, u32)>>,
    /// How many directory entries have been inserted or removed since the model was built.
    entries_touched: usize,
}

impl DirectoryModel {
    /// A model whose root holds `source` and `destination`, with `descendants` nodes hanging off a
    /// subtree root bound into `source` under the name `subtree`.
    ///
    /// The subtree is a wide-then-deep shape rather than a chain, so the descendant count is real
    /// rather than a depth in disguise.
    fn with_subtree(descendants: usize) -> Self {
        // 0 root · 1 source · 2 destination · 3 subtree root, then the descendants.
        let mut children: Vec<Vec<(String, u32)>> = vec![Vec::new(); 4 + descendants];
        children[0].push(("source".to_owned(), 1));
        children[0].push(("destination".to_owned(), 2));
        children[1].push(("subtree".to_owned(), 3));

        for index in 0..descendants {
            let node = 4 + index as u32;
            // A branching factor of sixteen: node n's parent is the subtree root for the first
            // sixteen, then an earlier descendant, so the tree is genuinely nested.
            let parent = if index < 16 {
                3
            } else {
                4 + (index as u32 / 16)
            };
            children[parent as usize].push((format!("n{index}"), node));
        }
        Self {
            children,
            entries_touched: 0,
        }
    }

    /// Apply one operation, counting the directory entries it touches.
    ///
    /// Only the members a subtree move uses are implemented; anything else is a test-fixture gap
    /// and says so rather than silently doing nothing.
    fn apply(&mut self, operation: &Operation, directories: &[(u32, ObjectId)]) {
        let index_of = |object: ObjectId| {
            directories
                .iter()
                .find(|(_, id)| *id == object)
                .map(|(index, _)| *index)
                .expect("the fixture names every directory it moves between")
        };
        match operation {
            Operation::MoveEntry {
                from_directory_id,
                from_name,
                to_directory_id,
                to_name,
                ..
            } => {
                let from = index_of(*from_directory_id) as usize;
                let to = index_of(*to_directory_id) as usize;
                let position = self.children[from]
                    .iter()
                    .position(|(name, _)| name == from_name.as_str())
                    .expect("the entry being moved exists");
                let (_, child) = self.children[from].remove(position);
                self.entries_touched += 1;
                self.children[to].push((to_name.as_str().to_owned(), child));
                self.entries_touched += 1;
            }
            other => panic!("the fixture does not model {:?}", other.kind()),
        }
    }

    /// How many nodes are reachable beneath `directory`, following name bindings.
    fn reachable_below(&self, directory: u32) -> usize {
        let mut stack = vec![directory];
        let mut seen = 0usize;
        while let Some(current) = stack.pop() {
            for (_, child) in &self.children[current as usize] {
                seen += 1;
                stack.push(*child);
            }
        }
        seen
    }

    /// The path of `target`, by walking down from the root. Used on a single node, because
    /// building every path is the linear work this operation exists to avoid.
    fn path_of(&self, target: u32) -> Option<String> {
        let mut stack = vec![(0u32, String::new())];
        while let Some((current, prefix)) = stack.pop() {
            for (name, child) in &self.children[current as usize] {
                let path = format!("{prefix}/{name}");
                if *child == target {
                    return Some(path);
                }
                stack.push((*child, path));
            }
        }
        None
    }
}

fn move_operation() -> Vec<Operation> {
    Operation::move_subtree(
        ObjectId::from_bytes([1; 16]),
        NormalizedName::new("subtree").unwrap(),
        ObjectId::from_bytes([2; 16]),
        NormalizedName::new("subtree").unwrap(),
        ObjectId::from_bytes([3; 16]),
    )
}

#[test]
fn a_subtree_move_is_one_operation_of_constant_size_at_every_subtree_size() {
    let sizes = [0usize, 1, 1_000, 1_000_000];
    let mut encodings = Vec::new();
    let mut touched = Vec::new();

    for size in sizes {
        let mut model = DirectoryModel::with_subtree(size);
        // `source` is directory 1 and `destination` is directory 2 in the fixture.
        let directories = [
            (1u32, ObjectId::from_bytes([1; 16])),
            (2u32, ObjectId::from_bytes([2; 16])),
        ];

        let operations = move_operation();
        assert_eq!(
            operations.len(),
            1,
            "a move is one operation at size {size}"
        );
        assert_eq!(operations[0].kind(), OperationKind::MoveEntry);

        let bytes = encode_canonical(&operations[0]);
        assert!(
            bytes.len() < METADATA_BUDGET_BYTES,
            "a subtree move encoded to {} bytes at size {size}, over the {METADATA_BUDGET_BYTES}-byte budget",
            bytes.len()
        );
        encodings.push(bytes.len());

        let started = Instant::now();
        model.apply(&operations[0], &directories);
        let elapsed = started.elapsed().as_millis();
        assert!(
            elapsed < WALL_CLOCK_CEILING_MILLIS,
            "moving a subtree of {size} took {elapsed} ms on the metadata plane"
        );

        touched.push(model.entries_touched);

        // Everything that was under the subtree is still under it, at its new home.
        assert_eq!(
            model.reachable_below(2),
            size + 1,
            "the destination does not hold the subtree and its {size} descendants"
        );
        assert_eq!(
            model.reachable_below(1),
            0,
            "the source still holds something after the move"
        );
    }

    assert!(
        encodings.windows(2).all(|pair| pair[0] == pair[1]),
        "the encoded size of a subtree move varies with the subtree: {encodings:?}"
    );
    assert!(
        touched.windows(2).all(|pair| pair[0] == pair[1]),
        "the number of directory entries touched varies with the subtree: {touched:?}"
    );
    assert_eq!(
        touched[0], 2,
        "a move touches exactly two directory entries: one unbound, one bound"
    );
}

#[test]
fn every_descendant_path_moves_without_the_descendant_being_named() {
    let descendants = 10_000usize;
    let mut model = DirectoryModel::with_subtree(descendants);
    let directories = [
        (1u32, ObjectId::from_bytes([1; 16])),
        (2u32, ObjectId::from_bytes([2; 16])),
    ];

    // A node deep inside the subtree, before the move.
    let deep = 4 + descendants as u32 - 1;
    let before = model
        .path_of(deep)
        .expect("the node is reachable before the move");
    assert!(before.starts_with("/source/subtree/"), "{before}");

    let operations = move_operation();
    // The operation names five things, and not one of them is a descendant.
    let Operation::MoveEntry { object_id, .. } = &operations[0] else {
        panic!("move_subtree produced something other than a MoveEntry");
    };
    assert_eq!(*object_id, ObjectId::from_bytes([3; 16]));

    model.apply(&operations[0], &directories);

    let after = model
        .path_of(deep)
        .expect("the node is reachable after the move");
    assert!(after.starts_with("/destination/subtree/"), "{after}");
    assert_eq!(
        after.strip_prefix("/destination").unwrap(),
        before.strip_prefix("/source").unwrap(),
        "the path below the moved root changed"
    );
    assert_eq!(model.entries_touched, 2);
}

/// The control. If the vocabulary had required naming every descendant, the encoding would grow
/// with the subtree and the budget would be unreachable — this shows what that would have cost, so
/// the constant result above is read as a design choice rather than as an accident.
#[test]
fn naming_every_descendant_would_have_blown_the_budget() {
    let descendants = 1_000usize;
    let per_entry = encode_canonical(&Operation::UnlinkDirectoryEntry {
        directory_id: ObjectId::from_bytes([1; 16]),
        name: NormalizedName::new("n0").unwrap(),
        object_id: ObjectId::from_bytes([3; 16]),
    })
    .len();
    let naive = per_entry * descendants * 2;
    assert!(
        naive > METADATA_BUDGET_BYTES,
        "the per-descendant spelling would have cost {naive} bytes for {descendants} descendants, \
         which was expected to exceed the {METADATA_BUDGET_BYTES}-byte budget"
    );
    assert!(encode_canonical(&move_operation()[0]).len() < per_entry * 4);
}
