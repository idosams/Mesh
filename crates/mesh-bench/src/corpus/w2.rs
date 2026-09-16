//! W2 — large monorepo.
//!
//! ```text
//! 1,000,000 files
//! 100 GB logical size
//! Only 5% accessed
//! ```
//!
//! The workload the virtual working copy exists for. The point is the ratio: a
//! hundred gigabytes on paper, five on the wire, and the whole question is
//! whether a checkout has to pay for the ninety-five it never opens.
//!
//! **The accessed subset is exactly 5%, not 5% on average.** Drawing per file
//! with probability 0.05 gives a different subset size every seed, and a
//! hydration benchmark whose input size wobbles is a hydration benchmark with a
//! confound in it. [`select_exactly`](super::tree::select_exactly) — Algorithm
//! S — gives exactly `files / 20`, spread across the tree, in constant memory.
//!
//! This is the one workload no test materialises. A hundred gigabytes and a
//! million inodes is a deliberate act on a machine chosen for it, not something
//! `cargo nextest` does on the way past; `benchmarks/workloads/README.md`
//! records what it costs and what was actually measured here.

use super::plan::{ContentKind, FileSpec, Item};
use super::rng::stream;
use super::shape::{ShapeFact, ShapeReport};
use super::tree::{band_of, path_for, size_ladder, ExactSelector, SizeBand};
use super::{Generator, Scale, Tally, WorkloadId};
use crate::json::{Json, JsonObject};

/// The accessed share, in parts per thousand. Plan §12.2: 5%.
const ACCESS_PERMILLE: u64 = 50;

/// The file mix of a monorepo: overwhelmingly source, with vendored archives and
/// build fixtures carrying most of the bytes.
const BANDS: [SizeBand; 6] = [
    SizeBand {
        share_permille: 500,
        min_bytes: 256,
        max_bytes: 32_768,
        kind: ContentKind::Text,
        extension: "rs",
    },
    SizeBand {
        share_permille: 250,
        min_bytes: 256,
        max_bytes: 32_768,
        kind: ContentKind::Text,
        extension: "java",
    },
    SizeBand {
        share_permille: 120,
        min_bytes: 128,
        max_bytes: 8_192,
        kind: ContentKind::Text,
        extension: "bzl",
    },
    SizeBand {
        share_permille: 70,
        min_bytes: 1_024,
        max_bytes: 262_144,
        kind: ContentKind::Csv,
        extension: "csv",
    },
    SizeBand {
        share_permille: 40,
        min_bytes: 65_536,
        max_bytes: 8_388_608,
        kind: ContentKind::Binary,
        extension: "bin",
    },
    SizeBand {
        share_permille: 20,
        min_bytes: 1_048_576,
        max_bytes: 67_108_864,
        kind: ContentKind::Container,
        extension: "zip",
    },
];

/// W2's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LargeMonorepoParameters {
    /// How many files the tree holds.
    pub files: usize,
    /// The tree's total logical size, in bytes.
    pub target_bytes: u64,
    /// The accessed share, in parts per thousand.
    pub access_permille: u64,
}

impl LargeMonorepoParameters {
    /// The parameters plan §12.2 states, and the two reductions of them.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            // Plan §12.2: 1,000,000 files, 100 GB, 5% accessed.
            Scale::Full => LargeMonorepoParameters {
                files: 1_000_000,
                target_bytes: 100_000_000_000,
                access_permille: ACCESS_PERMILLE,
            },
            Scale::Reduced => LargeMonorepoParameters {
                files: 20_000,
                target_bytes: 2_000_000_000,
                access_permille: ACCESS_PERMILLE,
            },
            // 500 rather than 512: the file count is kept divisible by 20 at
            // every scale so that "5% accessed" is exactly 5% rather than
            // whatever integer division leaves. 512 gives 25 of 512, which is
            // 4.88%, and a sparsity benchmark should not have to explain that.
            Scale::Smoke => LargeMonorepoParameters {
                files: 500,
                target_bytes: 4_000_000,
                access_permille: ACCESS_PERMILLE,
            },
        }
    }

    /// How many files the workload reads.
    #[must_use]
    pub const fn accessed_files(self) -> u64 {
        (self.files as u64 * self.access_permille) / 1000
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("files", Json::Uint(self.files as u64))
            .with("target_bytes", Json::Uint(self.target_bytes))
            .with("access_permille", Json::Uint(self.access_permille))
            .with("accessed_files", Json::Uint(self.accessed_files()))
    }
}

/// The W2 generator.
#[derive(Clone, Debug)]
pub struct LargeMonorepo {
    seed: u64,
    scale: Scale,
    parameters: LargeMonorepoParameters,
    sizes: Vec<u64>,
}

impl LargeMonorepo {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub fn new(seed: u64, scale: Scale) -> Self {
        let parameters = LargeMonorepoParameters::for_scale(scale);
        let sizes = size_ladder(
            seed,
            "w2/size",
            parameters.files,
            &BANDS,
            parameters.target_bytes,
        );
        LargeMonorepo {
            seed,
            scale,
            parameters,
            sizes,
        }
    }

    fn file(&self, index: usize) -> FileSpec {
        let band = &BANDS[band_of(index, self.parameters.files, &BANDS)];
        FileSpec {
            path: path_for("monorepo", index, band.extension),
            bytes: self.sizes[index],
            kind: band.kind,
            stream: stream(self.seed, "w2/content", index as u64),
        }
    }

    /// The accessed subset, as an iterator of exactly `accessed_files` paths.
    fn accesses(&self) -> impl Iterator<Item = Item> + '_ {
        let mut selector = ExactSelector::new(
            self.seed,
            "w2/access",
            self.parameters.files as u64,
            self.parameters.accessed_files(),
        );
        // `take` is consulted for every index, selected or not — that is the
        // algorithm, not a filter over a precomputed set.
        (0..self.parameters.files)
            .filter(move |_| selector.take())
            .map(move |index| Item::Access {
                path: self.file(index).path,
            })
    }
}

impl Generator for LargeMonorepo {
    fn id(&self) -> WorkloadId {
        WorkloadId::W2
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
        let files = (0..self.parameters.files).map(move |index| Item::File(self.file(index)));
        Box::new(files.chain(self.accesses()))
    }

    fn shape(&self) -> ShapeReport {
        let tally = Tally::of(self.items());
        ShapeReport::new()
            .with(ShapeFact::count(
                "file_count",
                self.parameters.files as u64,
                tally.files,
            ))
            .with(ShapeFact::bytes(
                "logical_bytes",
                self.parameters.target_bytes,
                tally.logical_bytes,
            ))
            .with(ShapeFact::count(
                "accessed_files",
                self.parameters.accessed_files(),
                tally.accesses,
            ))
            .with(ShapeFact::ratio(
                "accessed_ratio",
                self.parameters.access_permille as f64 / 1000.0,
                if tally.files == 0 {
                    0.0
                } else {
                    tally.accesses as f64 / tally.files as f64
                },
                0.0005,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CANONICAL_SEED;

    #[test]
    fn the_full_scale_is_the_plan_figure() {
        let parameters = LargeMonorepoParameters::for_scale(Scale::Full);
        assert_eq!(parameters.files, 1_000_000, "plan 12.2 W2: 1,000,000 files");
        assert_eq!(
            parameters.target_bytes, 100_000_000_000,
            "plan 12.2 W2: 100 GB"
        );
        assert_eq!(parameters.accessed_files(), 50_000, "plan 12.2 W2: 5%");
    }

    #[test]
    fn the_bands_are_a_complete_partition() {
        assert_eq!(
            BANDS.iter().map(|band| band.share_permille).sum::<u32>(),
            1000
        );
    }

    #[test]
    fn every_scale_keeps_the_file_count_divisible_by_the_access_share() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let parameters = LargeMonorepoParameters::for_scale(scale);
            assert_eq!(
                parameters.files % 20,
                0,
                "{}: {} files makes 5% inexact",
                scale.name(),
                parameters.files
            );
        }
    }

    #[test]
    fn exactly_five_percent_is_accessed_at_every_scale() {
        for scale in [Scale::Reduced, Scale::Smoke] {
            let generator = LargeMonorepo::new(CANONICAL_SEED, scale);
            let tally = Tally::of(generator.items());
            assert_eq!(
                tally.accesses * 20,
                tally.files,
                "{}: {} accessed of {}",
                scale.name(),
                tally.accesses,
                tally.files
            );
        }
    }

    #[test]
    fn accessed_paths_are_paths_that_exist() {
        let generator = LargeMonorepo::new(CANONICAL_SEED, Scale::Smoke);
        let files: std::collections::BTreeSet<String> = generator
            .items()
            .filter_map(|item| item.as_file().map(|file| file.path.clone()))
            .collect();
        let accessed: Vec<String> = generator
            .items()
            .filter_map(|item| match item {
                Item::Access { path } => Some(path),
                _ => None,
            })
            .collect();
        assert!(!accessed.is_empty());
        for path in accessed {
            assert!(
                files.contains(&path),
                "{path} is accessed but not generated"
            );
        }
    }

    #[test]
    fn the_accessed_subset_is_spread_across_the_tree() {
        let generator = LargeMonorepo::new(CANONICAL_SEED, Scale::Reduced);
        let accessed: Vec<String> = generator
            .items()
            .filter_map(|item| match item {
                Item::Access { path } => Some(path),
                _ => None,
            })
            .collect();
        let directories: std::collections::BTreeSet<&str> = accessed
            .iter()
            .filter_map(|path| path.rsplit_once('/').map(|(directory, _)| directory))
            .collect();
        assert!(
            directories.len() > 50,
            "5% of 20,000 files landed in only {} directories",
            directories.len()
        );
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = LargeMonorepo::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = LargeMonorepo::new(CANONICAL_SEED, Scale::Smoke);
        let right = LargeMonorepo::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }
}
