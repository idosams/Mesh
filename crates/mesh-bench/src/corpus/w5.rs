//! W5 — large binary.
//!
//! ```text
//! 1 GiB file
//! 1 KiB modification
//! Middle insertion
//! Append
//! Random 4 KiB rewrites
//! ```
//!
//! The content-defined-chunking workload. Every one of the four edit shapes is
//! there because it breaks a different naive design:
//!
//! * **The 1 KiB overwrite** is the cheap case. Fixed-size blocks handle it.
//! * **The middle insertion** is the case fixed-size blocks fail: everything
//!   after the splice shifts, so a block-aligned differ re-uploads the second
//!   half of a gibibyte to move it by a kibibyte. Content-defined chunking is
//!   supposed to resynchronise within one chunk.
//! * **The append** is the case a naive rolling hash re-scans the whole file for.
//! * **The random 4 KiB rewrites** are the scattered case, where per-edit
//!   overhead is what dominates and one big diff is not available.
//!
//! The edits are emitted as a description, in order, at fixed offsets derived
//! from the seed. They are not applied here — applying them is what a benchmark
//! *measures*, and a generator that applied them would be timing itself.

use super::plan::{ContentKind, EditOp, FileSpec, Item};
use super::rng::{stream, SplitMix64};
use super::shape::{ShapeFact, ShapeReport};
use super::{Generator, Scale, WorkloadId};
use crate::json::{Json, JsonObject};

/// The path of the file every edit applies to.
pub const TARGET_PATH: &str = "binary/large.bin";

/// W5's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LargeBinaryParameters {
    /// The base file's size, in bytes.
    pub base_bytes: u64,
    /// The size of the overwrite, the insertion and the append.
    pub modification_bytes: u64,
    /// The size of each scattered rewrite.
    pub rewrite_bytes: u64,
    /// How many scattered rewrites there are.
    pub rewrite_count: u64,
}

impl LargeBinaryParameters {
    /// The parameters plan §12.2 states, and the two reductions of them.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            // Plan §12.2: a 1 GiB file, a 1 KiB modification, random 4 KiB rewrites.
            Scale::Full => LargeBinaryParameters {
                base_bytes: 1_073_741_824,
                modification_bytes: 1_024,
                rewrite_bytes: 4_096,
                rewrite_count: 64,
            },
            Scale::Reduced => LargeBinaryParameters {
                base_bytes: 67_108_864,
                modification_bytes: 1_024,
                rewrite_bytes: 4_096,
                rewrite_count: 16,
            },
            Scale::Smoke => LargeBinaryParameters {
                base_bytes: 1_048_576,
                modification_bytes: 1_024,
                rewrite_bytes: 4_096,
                rewrite_count: 4,
            },
        }
    }

    /// The offset the middle insertion splices at: the exact midpoint.
    #[must_use]
    pub const fn middle_offset(self) -> u64 {
        self.base_bytes / 2
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("base_bytes", Json::Uint(self.base_bytes))
            .with("modification_bytes", Json::Uint(self.modification_bytes))
            .with("rewrite_bytes", Json::Uint(self.rewrite_bytes))
            .with("rewrite_count", Json::Uint(self.rewrite_count))
            .with("middle_offset", Json::Uint(self.middle_offset()))
    }
}

/// The W5 generator.
#[derive(Clone, Debug)]
pub struct LargeBinary {
    seed: u64,
    scale: Scale,
    parameters: LargeBinaryParameters,
}

impl LargeBinary {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub const fn new(seed: u64, scale: Scale) -> Self {
        LargeBinary {
            seed,
            scale,
            parameters: LargeBinaryParameters::for_scale(scale),
        }
    }

    /// The base file. One file: this workload is about what happens *to* it.
    fn base(&self) -> FileSpec {
        FileSpec {
            path: TARGET_PATH.to_owned(),
            bytes: self.parameters.base_bytes,
            kind: ContentKind::Binary,
            stream: stream(self.seed, "w5/base", 0),
        }
    }

    fn edit(&self, ordinal: u64, op: EditOp) -> Item {
        Item::Edit {
            path: TARGET_PATH.to_owned(),
            op,
            stream: stream(self.seed, "w5/edit", ordinal),
        }
    }

    /// The four edit shapes, in the order plan §12.2 lists them.
    fn edits(&self) -> Vec<Item> {
        let parameters = self.parameters;
        let mut source = SplitMix64::derived(self.seed, "w5/offset", 0);
        let last_safe_offset = parameters
            .base_bytes
            .saturating_sub(parameters.modification_bytes.max(parameters.rewrite_bytes));

        let mut edits = vec![
            // The 1 KiB modification, somewhere the middle insertion is not.
            self.edit(
                0,
                EditOp::Overwrite {
                    offset: source.in_range(0, last_safe_offset),
                    length: parameters.modification_bytes,
                },
            ),
            // The middle insertion, at the exact midpoint so a reader can check it.
            self.edit(
                1,
                EditOp::Insert {
                    offset: parameters.middle_offset(),
                    length: parameters.modification_bytes,
                },
            ),
            self.edit(
                2,
                EditOp::Append {
                    length: parameters.modification_bytes,
                },
            ),
        ];
        for index in 0..parameters.rewrite_count {
            edits.push(self.edit(
                3 + index,
                EditOp::Overwrite {
                    offset: source.in_range(0, last_safe_offset),
                    length: parameters.rewrite_bytes,
                },
            ));
        }
        edits
    }
}

impl Generator for LargeBinary {
    fn id(&self) -> WorkloadId {
        WorkloadId::W5
    }

    fn scale(&self) -> Scale {
        self.scale
    }

    fn seed(&self) -> u64 {
        self.seed
    }

    fn parameters(&self) -> JsonObject {
        self.parameters.to_json()
    }

    fn items(&self) -> Box<dyn Iterator<Item = Item> + '_> {
        Box::new(std::iter::once(Item::File(self.base())).chain(self.edits()))
    }

    fn shape(&self) -> ShapeReport {
        let parameters = self.parameters;
        let edits: Vec<EditOp> = self
            .items()
            .filter_map(|item| match item {
                Item::Edit { op, .. } => Some(op),
                _ => None,
            })
            .collect();

        let has_modification = edits.iter().any(|op| {
            matches!(op, EditOp::Overwrite { length, .. } if *length == parameters.modification_bytes)
        });
        let has_middle_insertion = edits.iter().any(|op| {
            matches!(op, EditOp::Insert { offset, length }
                if *offset == parameters.middle_offset() && *length == parameters.modification_bytes)
        });
        let has_append = edits.iter().any(|op| matches!(op, EditOp::Append { .. }));
        let rewrites = edits
            .iter()
            .filter(|op| {
                matches!(op, EditOp::Overwrite { length, .. } if *length == parameters.rewrite_bytes)
            })
            .count() as u64;
        let inside = edits.iter().all(|op| match op {
            EditOp::Overwrite { offset, length } => offset + length <= parameters.base_bytes,
            EditOp::Insert { offset, .. } => *offset <= parameters.base_bytes,
            EditOp::Append { .. } => true,
        });

        ShapeReport::new()
            .with(ShapeFact::bytes(
                "base_bytes",
                parameters.base_bytes,
                self.base().bytes,
            ))
            .with(ShapeFact::present(
                "has_modification_of_stated_size",
                has_modification,
            ))
            .with(ShapeFact::present(
                "has_middle_insertion",
                has_middle_insertion,
            ))
            .with(ShapeFact::present("has_append", has_append))
            .with(ShapeFact::count(
                "random_rewrite_count",
                parameters.rewrite_count,
                rewrites,
            ))
            .with(ShapeFact::bytes(
                "random_rewrite_bytes",
                parameters.rewrite_bytes,
                parameters.rewrite_bytes,
            ))
            .with(ShapeFact::present(
                "every_edit_lands_inside_the_file",
                inside,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CANONICAL_SEED;

    #[test]
    fn the_full_scale_is_the_plan_figure() {
        let parameters = LargeBinaryParameters::for_scale(Scale::Full);
        assert_eq!(parameters.base_bytes, 1_073_741_824, "plan 12.2 W5: 1 GiB");
        assert_eq!(parameters.modification_bytes, 1_024, "plan 12.2 W5: 1 KiB");
        assert_eq!(
            parameters.rewrite_bytes, 4_096,
            "plan 12.2 W5: 4 KiB rewrites"
        );
    }

    #[test]
    fn the_workload_is_one_file() {
        let generator = LargeBinary::new(CANONICAL_SEED, Scale::Smoke);
        let files: Vec<String> = generator
            .items()
            .filter_map(|item| item.as_file().map(|file| file.path.clone()))
            .collect();
        assert_eq!(files, vec![TARGET_PATH.to_owned()]);
    }

    #[test]
    fn all_four_edit_shapes_are_present() {
        let report = LargeBinary::new(CANONICAL_SEED, Scale::Full).shape();
        for fact in [
            "has_modification_of_stated_size",
            "has_middle_insertion",
            "has_append",
        ] {
            assert!(
                report.fact(fact).is_some_and(super::ShapeFact::holds),
                "{fact} is missing"
            );
        }
        assert_eq!(
            report
                .fact("random_rewrite_count")
                .map(|fact| fact.observed),
            Some(64.0)
        );
    }

    #[test]
    fn the_insertion_is_exactly_in_the_middle() {
        let generator = LargeBinary::new(CANONICAL_SEED, Scale::Full);
        let insertion = generator
            .items()
            .find_map(|item| match item {
                Item::Edit {
                    op: EditOp::Insert { offset, length },
                    ..
                } => Some((offset, length)),
                _ => None,
            })
            .expect("an insertion");
        assert_eq!(insertion.0, 1_073_741_824 / 2);
        assert_eq!(insertion.1, 1_024);
    }

    #[test]
    fn every_edit_lands_inside_the_file() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let parameters = LargeBinaryParameters::for_scale(scale);
            for item in LargeBinary::new(CANONICAL_SEED, scale).items() {
                if let Item::Edit {
                    op: EditOp::Overwrite { offset, length },
                    ..
                } = item
                {
                    assert!(
                        offset + length <= parameters.base_bytes,
                        "{}: {offset}+{length} exceeds {}",
                        scale.name(),
                        parameters.base_bytes
                    );
                }
            }
        }
    }

    #[test]
    fn the_scattered_rewrites_are_scattered() {
        let generator = LargeBinary::new(CANONICAL_SEED, Scale::Full);
        let offsets: Vec<u64> = generator
            .items()
            .filter_map(|item| match item {
                Item::Edit {
                    op:
                        EditOp::Overwrite {
                            offset,
                            length: 4_096,
                        },
                    ..
                } => Some(offset),
                _ => None,
            })
            .collect();
        assert_eq!(offsets.len(), 64);
        let unique: std::collections::BTreeSet<u64> = offsets.iter().copied().collect();
        assert_eq!(unique.len(), offsets.len(), "two rewrites share an offset");
        let span = unique.last().copied().unwrap_or(0) - unique.first().copied().unwrap_or(0);
        assert!(
            span > 1_073_741_824 / 2,
            "64 rewrites span only {span} bytes of a gibibyte"
        );
    }

    #[test]
    fn every_edit_carries_its_own_content_stream() {
        let generator = LargeBinary::new(CANONICAL_SEED, Scale::Smoke);
        let streams: Vec<u64> = generator
            .items()
            .filter_map(|item| match item {
                Item::Edit { stream, .. } => Some(stream),
                _ => None,
            })
            .collect();
        let unique: std::collections::BTreeSet<u64> = streams.iter().copied().collect();
        assert_eq!(
            unique.len(),
            streams.len(),
            "two edits write the same bytes"
        );
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = LargeBinary::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = LargeBinary::new(CANONICAL_SEED, Scale::Smoke);
        let right = LargeBinary::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }
}
