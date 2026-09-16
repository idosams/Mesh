//! Writing a described corpus to a real directory.
//!
//! Materialisation is separate from generation on purpose. A generator produces
//! a description; a benchmark that needs bytes on a filesystem calls this, and
//! most consumers — the digests, the shape checks, the manifest — never do.
//! That is what keeps a hundred-gigabyte workload verifiable on a laptop.
//!
//! Three rules this module keeps:
//!
//! * **Only files are written.** Edits, faults, accesses, changes and
//!   checkpoints are the *activity* a benchmark performs against the corpus. A
//!   generator that applied its own edits would be timing itself, and a corpus
//!   that arrived pre-edited would have no before-state to measure against.
//! * **Every path is re-checked here.** The generators only ever emit relative,
//!   traversal-free paths — and this function refuses anything else anyway,
//!   because a corpus root is often a scratch directory and a path bug that
//!   escapes it is a path bug that deletes something else.
//! * **The root must be empty.** Writing a corpus over an existing one gives a
//!   tree that matches neither digest, and a benchmark reading it would be
//!   measuring a corpus nobody can reproduce.

use super::content;
use super::plan::Item;
use super::Generator;
use crate::json::{Json, JsonObject};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Component, Path, PathBuf};

/// What materialising a corpus cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterializeReport {
    /// How many files were written.
    pub files: u64,
    /// How many bytes were written.
    pub bytes: u64,
    /// How many directories were created.
    pub directories: u64,
}

impl MaterializeReport {
    /// The report as JSON.
    #[must_use]
    pub fn to_json(self) -> Json {
        Json::Object(
            JsonObject::new()
                .with("files", Json::Uint(self.files))
                .with("bytes", Json::Uint(self.bytes))
                .with("directories", Json::Uint(self.directories)),
        )
    }
}

/// Why a corpus could not be written.
#[derive(Debug)]
pub enum MaterializeError {
    /// The destination already holds something.
    RootNotEmpty(PathBuf),
    /// A generated path was absolute, empty, or tried to leave the root.
    UnsafePath(String),
    /// The filesystem refused.
    Io(io::Error),
}

impl fmt::Display for MaterializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaterializeError::RootNotEmpty(root) => write!(
                formatter,
                "{} is not empty; a corpus written over another corpus matches neither digest",
                root.display()
            ),
            MaterializeError::UnsafePath(path) => {
                write!(
                    formatter,
                    "refusing to write `{path}`: it escapes the corpus root"
                )
            }
            MaterializeError::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for MaterializeError {}

impl From<io::Error> for MaterializeError {
    fn from(error: io::Error) -> Self {
        MaterializeError::Io(error)
    }
}

/// Writes `generator`'s files under `root`.
///
/// `root` is created if it does not exist and must be empty if it does.
///
/// # Errors
///
/// Returns [`MaterializeError`] when the root is occupied, a path is unsafe, or
/// the filesystem refuses a write.
pub fn materialize(
    generator: &dyn Generator,
    root: &Path,
) -> Result<MaterializeReport, MaterializeError> {
    prepare_root(root)?;
    let mut report = MaterializeReport::default();
    let mut created: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();

    for item in generator.items() {
        let Item::File(file) = item else { continue };
        let relative = safe_relative(&file.path)?;
        let absolute = root.join(&relative);
        if let Some(parent) = absolute.parent() {
            if created.insert(parent.to_path_buf()) {
                fs::create_dir_all(parent)?;
                report.directories += 1;
            }
        }
        let handle = File::create(&absolute)?;
        let mut writer = BufWriter::with_capacity(content::CHUNK_BYTES, handle);
        let mut failure: Option<io::Error> = None;
        content::generate(&file, &mut |chunk| {
            if failure.is_none() {
                if let Err(error) = writer.write_all(chunk) {
                    failure = Some(error);
                }
            }
        });
        if let Some(error) = failure {
            return Err(MaterializeError::Io(error));
        }
        writer.flush()?;
        report.files += 1;
        report.bytes += file.bytes;
    }
    Ok(report)
}

/// Creates the root, or checks that an existing one is empty.
fn prepare_root(root: &Path) -> Result<(), MaterializeError> {
    if root.exists() {
        if root.read_dir()?.next().is_some() {
            return Err(MaterializeError::RootNotEmpty(root.to_path_buf()));
        }
        return Ok(());
    }
    fs::create_dir_all(root)?;
    Ok(())
}

/// Accepts a relative, traversal-free path and rejects everything else.
fn safe_relative(path: &str) -> Result<PathBuf, MaterializeError> {
    if path.is_empty() {
        return Err(MaterializeError::UnsafePath(path.to_owned()));
    }
    let candidate = PathBuf::from(path);
    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(MaterializeError::UnsafePath(path.to_owned())),
        }
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{build, Scale, WorkloadId, CANONICAL_SEED};
    use crate::testing::TempDir;

    #[test]
    fn a_smoke_corpus_lands_on_disk_with_the_right_sizes() {
        let directory = TempDir::new("corpus-materialize");
        let root = directory.path().join("w1");
        let generator = build(WorkloadId::W1, Scale::Smoke, CANONICAL_SEED);
        let report = materialize(generator.as_ref(), &root).expect("writes");
        assert_eq!(report.files, 64);
        assert_eq!(report.bytes, 1_000_000);

        for item in generator.items() {
            let Some(file) = item.as_file() else { continue };
            let written = fs::metadata(root.join(&file.path)).expect("the file exists");
            assert_eq!(
                written.len(),
                file.bytes,
                "{} has the wrong size",
                file.path
            );
        }
    }

    #[test]
    fn the_bytes_on_disk_are_the_bytes_the_generator_described() {
        let directory = TempDir::new("corpus-materialize-bytes");
        let root = directory.path().join("w6");
        let generator = build(WorkloadId::W6, Scale::Smoke, CANONICAL_SEED);
        materialize(generator.as_ref(), &root).expect("writes");
        for item in generator.items() {
            let Some(file) = item.as_file() else { continue };
            let on_disk = fs::read(root.join(&file.path)).expect("readable");
            assert_eq!(on_disk, content::to_vec(file), "{} drifted", file.path);
        }
    }

    #[test]
    fn two_materialisations_of_one_seed_are_byte_identical() {
        let directory = TempDir::new("corpus-materialize-twice");
        let generator = build(WorkloadId::W4, Scale::Smoke, CANONICAL_SEED);
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        materialize(generator.as_ref(), &first).expect("writes");
        materialize(generator.as_ref(), &second).expect("writes");
        for item in generator.items() {
            let Some(file) = item.as_file() else { continue };
            assert_eq!(
                fs::read(first.join(&file.path)).expect("readable"),
                fs::read(second.join(&file.path)).expect("readable"),
                "{} differs between two runs",
                file.path
            );
        }
    }

    #[test]
    fn only_files_are_written() {
        // W5 is one file and sixty-seven edits; the edits are activity, not data.
        let directory = TempDir::new("corpus-materialize-edits");
        let root = directory.path().join("w5");
        let generator = build(WorkloadId::W5, Scale::Smoke, CANONICAL_SEED);
        let report = materialize(generator.as_ref(), &root).expect("writes");
        assert_eq!(report.files, 1);
        assert_eq!(report.bytes, 1_048_576);
    }

    #[test]
    fn an_occupied_root_is_refused() {
        let directory = TempDir::new("corpus-materialize-occupied");
        let root = directory.path().join("w1");
        let generator = build(WorkloadId::W1, Scale::Smoke, CANONICAL_SEED);
        materialize(generator.as_ref(), &root).expect("writes");
        let error = materialize(generator.as_ref(), &root).expect_err("the second write refuses");
        assert!(
            matches!(error, MaterializeError::RootNotEmpty(_)),
            "{error}"
        );
        assert!(error.to_string().contains("not empty"));
    }

    #[test]
    fn an_empty_existing_root_is_accepted() {
        let directory = TempDir::new("corpus-materialize-empty");
        let root = directory.path().join("w1");
        fs::create_dir_all(&root).expect("creatable");
        let generator = build(WorkloadId::W1, Scale::Smoke, CANONICAL_SEED);
        assert!(materialize(generator.as_ref(), &root).is_ok());
    }

    #[test]
    fn escaping_paths_are_refused() {
        for path in ["../escape", "/absolute", "", "a/../../b"] {
            assert!(
                safe_relative(path).is_err(),
                "`{path}` should not be writable"
            );
        }
    }

    #[test]
    fn ordinary_paths_are_accepted() {
        assert_eq!(
            safe_relative("workspace/d01/f0000001.rs").expect("safe"),
            PathBuf::from("workspace/d01/f0000001.rs")
        );
    }

    #[test]
    fn the_report_serialises() {
        let json = MaterializeReport {
            files: 2,
            bytes: 3,
            directories: 1,
        }
        .to_json();
        let object = json.as_object().expect("an object");
        assert_eq!(object.get("files").and_then(Json::as_u64), Some(2));
        assert_eq!(object.get("bytes").and_then(Json::as_u64), Some(3));
    }
}
