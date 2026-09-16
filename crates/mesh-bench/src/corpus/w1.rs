//! W1 — small codebase.
//!
//! ```text
//! 20,000 files
//! 1.5 GB
//! Mostly text
//! Normal package install/build/test
//! ```
//!
//! The everyday workload: the repository a person actually works in. It is the
//! baseline for lookup, open, create, rename and directory enumeration, and the
//! one where a filesystem adapter's fixed overhead is most visible, because
//! nothing here is big enough to hide it.
//!
//! The three activities are emitted as items rather than executed. This module
//! generates data; running `install`, `build` and `test` against it is the
//! runner's job, and putting a subprocess in a data generator would make the
//! corpus depend on a toolchain.

use super::plan::{ContentKind, FileSpec, Item};
use super::rng::stream;
use super::shape::{ShapeFact, ShapeReport};
use super::tree::{band_of, path_for, size_ladder, SizeBand};
use super::{Generator, Scale, Tally, WorkloadId};
use crate::json::{Json, JsonObject};

/// The activities a runner replays against the corpus, in order.
pub const ACTIVITIES: [&str; 3] = ["install", "build", "test"];

/// The file mix. Shares are parts per thousand and sum to 1000.
///
/// Chosen to look like a working repository rather than to be pretty: source in
/// three languages, documentation, configuration, fixtures, and a small tail of
/// committed assets that is where most of the bytes actually live.
const BANDS: [SizeBand; 7] = [
    SizeBand {
        share_permille: 400,
        min_bytes: 512,
        max_bytes: 16_384,
        kind: ContentKind::Text,
        extension: "rs",
    },
    SizeBand {
        share_permille: 250,
        min_bytes: 512,
        max_bytes: 16_384,
        kind: ContentKind::Text,
        extension: "ts",
    },
    SizeBand {
        share_permille: 150,
        min_bytes: 512,
        max_bytes: 8_192,
        kind: ContentKind::Text,
        extension: "py",
    },
    SizeBand {
        share_permille: 100,
        min_bytes: 256,
        max_bytes: 8_192,
        kind: ContentKind::Text,
        extension: "md",
    },
    SizeBand {
        share_permille: 50,
        min_bytes: 128,
        max_bytes: 65_536,
        kind: ContentKind::Text,
        extension: "json",
    },
    SizeBand {
        share_permille: 30,
        min_bytes: 1_024,
        max_bytes: 131_072,
        kind: ContentKind::Csv,
        extension: "csv",
    },
    SizeBand {
        share_permille: 20,
        min_bytes: 4_096,
        max_bytes: 1_048_576,
        kind: ContentKind::Binary,
        extension: "png",
    },
];

/// W1's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmallCodebaseParameters {
    /// How many files the corpus holds.
    pub files: usize,
    /// The corpus's total logical size, in bytes.
    pub target_bytes: u64,
}

impl SmallCodebaseParameters {
    /// The parameters plan §12.2 states, and the two reductions of them.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            // Plan §12.2: 20,000 files, 1.5 GB.
            Scale::Full => SmallCodebaseParameters {
                files: 20_000,
                target_bytes: 1_500_000_000,
            },
            Scale::Reduced => SmallCodebaseParameters {
                files: 2_000,
                target_bytes: 150_000_000,
            },
            Scale::Smoke => SmallCodebaseParameters {
                files: 64,
                target_bytes: 1_000_000,
            },
        }
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("files", Json::Uint(self.files as u64))
            .with("target_bytes", Json::Uint(self.target_bytes))
            .with("activities", Json::Uint(ACTIVITIES.len() as u64))
    }
}

/// The W1 generator.
#[derive(Clone, Debug)]
pub struct SmallCodebase {
    seed: u64,
    scale: Scale,
    parameters: SmallCodebaseParameters,
    sizes: Vec<u64>,
}

impl SmallCodebase {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub fn new(seed: u64, scale: Scale) -> Self {
        let parameters = SmallCodebaseParameters::for_scale(scale);
        let sizes = size_ladder(
            seed,
            "w1/size",
            parameters.files,
            &BANDS,
            parameters.target_bytes,
        );
        SmallCodebase {
            seed,
            scale,
            parameters,
            sizes,
        }
    }

    fn file(&self, index: usize) -> FileSpec {
        let band = &BANDS[band_of(index, self.parameters.files, &BANDS)];
        FileSpec {
            path: path_for("workspace", index, band.extension),
            bytes: self.sizes[index],
            kind: band.kind,
            stream: stream(self.seed, "w1/content", index as u64),
        }
    }
}

impl Generator for SmallCodebase {
    fn id(&self) -> WorkloadId {
        WorkloadId::W1
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
        let activities = ACTIVITIES.iter().map(|name| Item::Activity {
            name: (*name).to_owned(),
        });
        Box::new(files.chain(activities))
    }

    fn shape(&self) -> ShapeReport {
        let tally = Tally::of(self.items());
        // "Mostly text" is realised as the band table: every band except the
        // committed-asset tail carries a text profile, which is 980 of 1000.
        let stated_text_ratio = f64::from(
            BANDS
                .iter()
                .filter(|band| band.kind.is_text())
                .map(|band| band.share_permille)
                .sum::<u32>(),
        ) / 1000.0;
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
            .with(ShapeFact::ratio(
                "text_file_ratio",
                stated_text_ratio,
                tally.text_ratio(),
                0.01,
            ))
            .with(ShapeFact::count(
                "activity_count",
                ACTIVITIES.len() as u64,
                tally.activities,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CANONICAL_SEED;

    #[test]
    fn the_full_scale_is_the_plan_figure() {
        let parameters = SmallCodebaseParameters::for_scale(Scale::Full);
        assert_eq!(parameters.files, 20_000, "plan 12.2 W1: 20,000 files");
        assert_eq!(
            parameters.target_bytes, 1_500_000_000,
            "plan 12.2 W1: 1.5 GB"
        );
    }

    #[test]
    fn the_bands_are_a_complete_partition() {
        assert_eq!(
            BANDS.iter().map(|band| band.share_permille).sum::<u32>(),
            1000
        );
    }

    #[test]
    fn the_corpus_totals_exactly_its_stated_size() {
        let generator = SmallCodebase::new(CANONICAL_SEED, Scale::Reduced);
        let tally = Tally::of(generator.items());
        assert_eq!(tally.logical_bytes, 150_000_000);
        assert_eq!(tally.files, 2_000);
    }

    #[test]
    fn the_corpus_is_mostly_text() {
        let generator = SmallCodebase::new(CANONICAL_SEED, Scale::Reduced);
        assert!(Tally::of(generator.items()).text_ratio() > 0.9);
    }

    #[test]
    fn the_three_activities_are_emitted_in_order() {
        let generator = SmallCodebase::new(CANONICAL_SEED, Scale::Smoke);
        let names: Vec<String> = generator
            .items()
            .filter_map(|item| match item {
                Item::Activity { name } => Some(name),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["install", "build", "test"]);
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = SmallCodebase::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn source_files_carry_source_extensions() {
        let generator = SmallCodebase::new(CANONICAL_SEED, Scale::Smoke);
        let first = generator.items().next().expect("a file");
        let path = &first.as_file().expect("the first item is a file").path;
        assert!(path.ends_with(".rs"), "{path}");
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = SmallCodebase::new(CANONICAL_SEED, Scale::Smoke);
        let right = SmallCodebase::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }
}
