//! W4 — mixed business workspace.
//!
//! ```text
//! 5,000 files
//! DOCX, XLSX, PPTX, PDF, images, CSV, text
//! 20 GB
//! ```
//!
//! The workload that is not a codebase. It exists because Mesh's claims are
//! about *workspaces*, and a workspace full of office documents behaves nothing
//! like a source tree under chunking, delta and deduplication.
//!
//! # These are byte profiles, not documents
//!
//! A file this module writes with a `.docx` name is **not a valid DOCX**. It is
//! high-entropy bytes with the property that matters to a storage benchmark: a
//! zip-based office format is a deflate stream, so a one-byte edit near the
//! front changes essentially everything after it, and a chunker gets no reuse
//! from it. `.pdf` and image extensions get a stable header and an
//! incompressible body — a container worth deduplicating over a payload that is
//! not.
//!
//! Anything that needs to *parse* these files needs real ones, and this
//! generator will mislead it. The seven categories are recorded in the shape
//! report so a consumer can see exactly what it is getting.

use super::plan::{ContentKind, FileSpec, Item};
use super::rng::stream;
use super::shape::{ShapeFact, ShapeReport};
use super::tree::{band_of, path_for, size_ladder, SizeBand};
use super::{Generator, Scale, Tally, WorkloadId};
use crate::json::{Json, JsonObject};
use std::collections::BTreeMap;

/// The seven categories plan §12.2 lists, and the extensions that carry them.
///
/// `images` is two extensions; every other category is one. The mapping is
/// written out so the shape report can prove all seven are present rather than
/// counting extensions and hoping.
const CATEGORIES: [(&str, &[&str]); 7] = [
    ("docx", &["docx"]),
    ("xlsx", &["xlsx"]),
    ("pptx", &["pptx"]),
    ("pdf", &["pdf"]),
    ("images", &["png", "jpg"]),
    ("csv", &["csv"]),
    ("text", &["txt"]),
];

/// The format mix. Shares are parts per thousand and sum to 1000.
const BANDS: [SizeBand; 8] = [
    SizeBand {
        share_permille: 200,
        min_bytes: 32_768,
        max_bytes: 8_388_608,
        kind: ContentKind::Container,
        extension: "docx",
    },
    SizeBand {
        share_permille: 150,
        min_bytes: 16_384,
        max_bytes: 33_554_432,
        kind: ContentKind::Container,
        extension: "xlsx",
    },
    SizeBand {
        share_permille: 100,
        min_bytes: 262_144,
        max_bytes: 67_108_864,
        kind: ContentKind::Container,
        extension: "pptx",
    },
    SizeBand {
        share_permille: 200,
        min_bytes: 65_536,
        max_bytes: 16_777_216,
        kind: ContentKind::Binary,
        extension: "pdf",
    },
    SizeBand {
        share_permille: 120,
        min_bytes: 32_768,
        max_bytes: 8_388_608,
        kind: ContentKind::Binary,
        extension: "png",
    },
    SizeBand {
        share_permille: 80,
        min_bytes: 65_536,
        max_bytes: 12_582_912,
        kind: ContentKind::Binary,
        extension: "jpg",
    },
    SizeBand {
        share_permille: 100,
        min_bytes: 4_096,
        max_bytes: 4_194_304,
        kind: ContentKind::Csv,
        extension: "csv",
    },
    SizeBand {
        share_permille: 50,
        min_bytes: 1_024,
        max_bytes: 262_144,
        kind: ContentKind::Text,
        extension: "txt",
    },
];

/// W4's stated shape at one scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusinessWorkspaceParameters {
    /// How many files the workspace holds.
    pub files: usize,
    /// The workspace's total logical size, in bytes.
    pub target_bytes: u64,
}

impl BusinessWorkspaceParameters {
    /// The parameters plan §12.2 states, and the two reductions of them.
    #[must_use]
    pub const fn for_scale(scale: Scale) -> Self {
        match scale {
            // Plan §12.2: 5,000 files, 20 GB.
            Scale::Full => BusinessWorkspaceParameters {
                files: 5_000,
                target_bytes: 20_000_000_000,
            },
            Scale::Reduced => BusinessWorkspaceParameters {
                files: 1_000,
                target_bytes: 2_000_000_000,
            },
            Scale::Smoke => BusinessWorkspaceParameters {
                files: 80,
                target_bytes: 4_000_000,
            },
        }
    }

    /// The parameter object recorded in descriptors and result rows.
    #[must_use]
    pub fn to_json(self) -> JsonObject {
        JsonObject::new()
            .with("files", Json::Uint(self.files as u64))
            .with("target_bytes", Json::Uint(self.target_bytes))
            .with("categories", Json::Uint(CATEGORIES.len() as u64))
    }
}

/// The W4 generator.
#[derive(Clone, Debug)]
pub struct BusinessWorkspace {
    seed: u64,
    scale: Scale,
    parameters: BusinessWorkspaceParameters,
    sizes: Vec<u64>,
}

impl BusinessWorkspace {
    /// Builds the generator for a seed and a scale.
    #[must_use]
    pub fn new(seed: u64, scale: Scale) -> Self {
        let parameters = BusinessWorkspaceParameters::for_scale(scale);
        let sizes = size_ladder(
            seed,
            "w4/size",
            parameters.files,
            &BANDS,
            parameters.target_bytes,
        );
        BusinessWorkspace {
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
            stream: stream(self.seed, "w4/content", index as u64),
        }
    }

    /// How many files each of the seven categories holds.
    fn per_category(&self) -> BTreeMap<&'static str, u64> {
        let mut counts: BTreeMap<&'static str, u64> = CATEGORIES
            .iter()
            .map(|(category, _)| (*category, 0))
            .collect();
        for item in self.items() {
            let Some(file) = item.as_file() else { continue };
            let extension = file.path.rsplit_once('.').map(|(_, tail)| tail);
            for (category, extensions) in CATEGORIES {
                if extension.is_some_and(|found| extensions.contains(&found)) {
                    *counts.entry(category).or_default() += 1;
                }
            }
        }
        counts
    }
}

impl Generator for BusinessWorkspace {
    fn id(&self) -> WorkloadId {
        WorkloadId::W4
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
        Box::new((0..self.parameters.files).map(move |index| Item::File(self.file(index))))
    }

    fn shape(&self) -> ShapeReport {
        let tally = Tally::of(self.items());
        let counts = self.per_category();
        let mut report = ShapeReport::new()
            .with(ShapeFact::count(
                "file_count",
                self.parameters.files as u64,
                tally.files,
            ))
            .with(ShapeFact::bytes(
                "logical_bytes",
                self.parameters.target_bytes,
                tally.logical_bytes,
            ));
        for (category, _) in CATEGORIES {
            report = report.with(ShapeFact::present(
                format!("has_{category}"),
                counts.get(category).copied().unwrap_or(0) > 0,
            ));
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::CANONICAL_SEED;

    #[test]
    fn the_full_scale_is_the_plan_figure() {
        let parameters = BusinessWorkspaceParameters::for_scale(Scale::Full);
        assert_eq!(parameters.files, 5_000, "plan 12.2 W4: 5,000 files");
        assert_eq!(
            parameters.target_bytes, 20_000_000_000,
            "plan 12.2 W4: 20 GB"
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
    fn every_band_extension_belongs_to_a_category() {
        for band in BANDS {
            assert!(
                CATEGORIES
                    .iter()
                    .any(|(_, extensions)| extensions.contains(&band.extension)),
                "{} belongs to no category",
                band.extension
            );
        }
    }

    #[test]
    fn all_seven_categories_are_present_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let generator = BusinessWorkspace::new(CANONICAL_SEED, scale);
            let counts = generator.per_category();
            for (category, _) in CATEGORIES {
                assert!(
                    counts.get(category).copied().unwrap_or(0) > 0,
                    "{} has no {category} files",
                    scale.name()
                );
            }
        }
    }

    #[test]
    fn office_containers_carry_the_container_profile() {
        let generator = BusinessWorkspace::new(CANONICAL_SEED, Scale::Smoke);
        for item in generator.items() {
            let Some(file) = item.as_file() else { continue };
            if file.path.ends_with(".docx") || file.path.ends_with(".xlsx") {
                assert_eq!(
                    file.kind,
                    ContentKind::Container,
                    "{} is not incompressible",
                    file.path
                );
            }
        }
    }

    #[test]
    fn the_workspace_totals_exactly_its_stated_size() {
        let generator = BusinessWorkspace::new(CANONICAL_SEED, Scale::Reduced);
        let tally = Tally::of(generator.items());
        assert_eq!(tally.logical_bytes, 2_000_000_000);
        assert_eq!(tally.files, 1_000);
    }

    #[test]
    fn a_business_workspace_is_mostly_not_text() {
        let generator = BusinessWorkspace::new(CANONICAL_SEED, Scale::Reduced);
        assert!(Tally::of(generator.items()).text_ratio() < 0.2);
    }

    #[test]
    fn the_shape_report_holds_at_every_scale() {
        for scale in [Scale::Full, Scale::Reduced, Scale::Smoke] {
            let report = BusinessWorkspace::new(CANONICAL_SEED, scale).shape();
            assert!(report.holds(), "{} {report:?}", scale.name());
        }
    }

    #[test]
    fn the_generator_is_reproducible() {
        let left = BusinessWorkspace::new(CANONICAL_SEED, Scale::Smoke);
        let right = BusinessWorkspace::new(CANONICAL_SEED, Scale::Smoke);
        assert_eq!(left.plan_digest(), right.plan_digest());
        assert_eq!(left.content_digest(), right.content_digest());
    }
}
