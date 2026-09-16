//! Explicit, user-owned export of immutable artifact versions for full native inspection.
//!
//! The renderer remains a bounded navigation aid. When a person needs layout, notes, charts,
//! formulas, annotations, or another native feature, Mesh may place exact reviewed bytes in a
//! folder they selected. This module never opens document content, overwrites a path, or deletes
//! an earlier export. Directory descriptors keep a same-user destination replacement from
//! redirecting writes after preflight.

use std::ffi::{CStr, CString, OsStr};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::fd::{AsRawFd as _, FromRawFd as _, RawFd};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use mesh_daemon::ipc::Json;

const SCHEMA: &str = "mesh-review-inspection/v1";
const MAX_COPIES: usize = 2;
const MAX_EXPORT_ATTEMPTS: usize = 100;

#[cfg(target_os = "macos")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0010_0000 | 0x0000_0100;
#[cfg(target_os = "linux")]
const OPEN_DIRECTORY_FLAGS: i32 = 0x0001_0000 | 0x0002_0000;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const OPEN_DIRECTORY_FLAGS: i32 = 0;

#[cfg(target_os = "macos")]
const CREATE_FILE_FLAGS: i32 = 0x0000_0001 | 0x0000_0200 | 0x0000_0800 | 0x0000_0100;
#[cfg(target_os = "linux")]
const CREATE_FILE_FLAGS: i32 = 0x0000_0001 | 0x0000_0040 | 0x0000_0080 | 0x0002_0000;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const CREATE_FILE_FLAGS: i32 = 0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReviewInspectionCopy<'a> {
    pub(crate) side: &'static str,
    pub(crate) reviewed_path: &'a str,
    pub(crate) extension: &'static str,
    pub(crate) version: &'a str,
    pub(crate) digest: &'a str,
    pub(crate) bytes: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReviewInspectionFile {
    pub(crate) side: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) version: String,
    pub(crate) digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReviewInspectionExport {
    pub(crate) directory: PathBuf,
    pub(crate) files: Vec<ReviewInspectionFile>,
}

#[derive(Debug)]
pub(crate) enum ReviewInspectionError {
    Invalid(&'static str),
    Io(&'static str, std::io::Error),
}

impl ReviewInspectionError {
    fn io(context: &'static str, error: std::io::Error) -> Self {
        Self::Io(context, error)
    }
}

impl fmt::Display for ReviewInspectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "review-copy export refused: {reason}"),
            Self::Io(context, error) => {
                write!(formatter, "review-copy export could not {context}: {error}")
            }
        }
    }
}

fn canonical_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn component(value: &OsStr) -> Result<CString, ReviewInspectionError> {
    if value.is_empty() || value.as_bytes().contains(&b'/') {
        return Err(ReviewInspectionError::Invalid(
            "an export name was not one ordinary path component",
        ));
    }
    CString::new(value.as_bytes()).map_err(|_| {
        ReviewInspectionError::Invalid("an export name contained an unsupported character")
    })
}

#[allow(unsafe_code)]
fn mkdirat(directory: RawFd, name: &CStr, mode: u32) -> std::io::Result<()> {
    unsafe extern "C" {
        fn mkdirat(directory: i32, path: *const std::ffi::c_char, mode: u32) -> i32;
    }
    // SAFETY: `name` is a live C string and `directory` is a retained directory descriptor.
    if unsafe { mkdirat(directory, name.as_ptr(), mode) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[allow(unsafe_code, clashing_extern_declarations)]
fn openat(directory: RawFd, name: &CStr, flags: i32, mode: i32) -> std::io::Result<File> {
    unsafe extern "C" {
        #[link_name = "openat"]
        fn openat_with_mode(directory: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    }
    // SAFETY: `name` is a live C string and a successful call transfers one descriptor.
    let descriptor = unsafe { openat_with_mode(directory, name.as_ptr(), flags, mode) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful `openat` returned a fresh owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[allow(unsafe_code)]
fn unlinkat(directory: RawFd, name: &CStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    // SAFETY: `name` is a live C string and flags=0 removes only the named non-directory entry.
    if unsafe { unlinkat(directory, name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn create_file(directory: &File, name: &str, bytes: &[u8]) -> Result<(), ReviewInspectionError> {
    let name = component(OsStr::new(name))?;
    let mut file = openat(directory.as_raw_fd(), &name, CREATE_FILE_FLAGS, 0o400)
        .map_err(|error| ReviewInspectionError::io("create a new read-only copy", error))?;
    file.write_all(bytes)
        .map_err(|error| ReviewInspectionError::io("write an exact copy", error))?;
    file.sync_all()
        .map_err(|error| ReviewInspectionError::io("make an exact copy durable", error))?;
    Ok(())
}

fn manifest(
    bundle: &str,
    target: &str,
    object: &str,
    copies: &[ReviewInspectionCopy<'_>],
) -> Vec<u8> {
    let files = copies
        .iter()
        .map(|copy| {
            Json::object([
                ("side", Json::text(copy.side)),
                (
                    "file",
                    Json::text(format!("{}.{}", title(copy.side), copy.extension)),
                ),
                ("reviewed_path", Json::text(copy.reviewed_path)),
                ("version_id", Json::text(copy.version)),
                ("content_digest", Json::text(copy.digest)),
                ("bytes", Json::text(copy.bytes.len().to_string())),
                ("read_only", Json::Bool(true)),
            ])
        })
        .collect();
    format!(
        "{}\n",
        Json::object([
            ("schema", Json::text(SCHEMA)),
            ("review_bundle", Json::text(bundle)),
            ("subject_operation", Json::text(target)),
            ("object_id", Json::text(object)),
            ("files", Json::Array(files)),
            ("working_folder_unchanged", Json::Bool(true)),
            ("opens_document_content", Json::Bool(false)),
            ("approval_input", Json::text("exact-saved-bytes")),
        ])
        .encode()
    )
    .into_bytes()
}

fn title(side: &str) -> &'static str {
    match side {
        "before" => "Before",
        "after" => "After",
        _ => "Invalid",
    }
}

fn validate_inputs(
    destination: &Path,
    bundle: &str,
    target: &str,
    object: &str,
    copies: &[ReviewInspectionCopy<'_>],
) -> Result<(), ReviewInspectionError> {
    if !destination.is_absolute() {
        return Err(ReviewInspectionError::Invalid(
            "the selected destination was not absolute",
        ));
    }
    if !canonical_hex(bundle, 64) || !canonical_hex(target, 64) || !canonical_hex(object, 32) {
        return Err(ReviewInspectionError::Invalid(
            "the reviewed identities were not canonical",
        ));
    }
    if copies.is_empty() || copies.len() > MAX_COPIES {
        return Err(ReviewInspectionError::Invalid(
            "the export did not contain one or two reviewed sides",
        ));
    }
    let mut sides = std::collections::HashSet::new();
    let extension = copies[0].extension;
    for copy in copies {
        if !matches!(copy.side, "before" | "after")
            || !sides.insert(copy.side)
            || copy.extension != extension
            || !matches!(copy.extension, "pdf" | "pptx" | "docx" | "xlsx")
            || !canonical_hex(copy.version, 64)
            || !canonical_hex(copy.digest, 64)
            || copy.bytes.is_empty()
        {
            return Err(ReviewInspectionError::Invalid(
                "one reviewed copy was malformed or inconsistent",
            ));
        }
    }
    Ok(())
}

pub(crate) fn export_review_inspection(
    destination: &Path,
    bundle: &str,
    target: &str,
    object: &str,
    copies: &[ReviewInspectionCopy<'_>],
) -> Result<ReviewInspectionExport, ReviewInspectionError> {
    export_review_inspection_with_hook(destination, bundle, target, object, copies, || {})
}

fn export_review_inspection_with_hook(
    destination: &Path,
    bundle: &str,
    target: &str,
    object: &str,
    copies: &[ReviewInspectionCopy<'_>],
    hook: impl FnOnce(),
) -> Result<ReviewInspectionExport, ReviewInspectionError> {
    validate_inputs(destination, bundle, target, object, copies)?;
    let parent_metadata = fs::symlink_metadata(destination)
        .map_err(|error| ReviewInspectionError::io("inspect the selected destination", error))?;
    if !parent_metadata.file_type().is_dir() || parent_metadata.file_type().is_symlink() {
        return Err(ReviewInspectionError::Invalid(
            "the selected destination was not a real directory",
        ));
    }
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(destination)
        .map_err(|error| ReviewInspectionError::io("open the selected destination", error))?;
    let retained_parent = parent
        .metadata()
        .map_err(|error| ReviewInspectionError::io("inspect the opened destination", error))?;
    let parent_identity = (retained_parent.dev(), retained_parent.ino());
    if !retained_parent.is_dir()
        || parent_identity != (parent_metadata.dev(), parent_metadata.ino())
    {
        return Err(ReviewInspectionError::Invalid(
            "the selected destination changed during inspection",
        ));
    }
    hook();

    let prefix = format!("Mesh review {}", &bundle[..12]);
    let mut created = None;
    for attempt in 1..=MAX_EXPORT_ATTEMPTS {
        let leaf = if attempt == 1 {
            prefix.clone()
        } else {
            format!("{prefix} {attempt}")
        };
        let leaf_name = component(OsStr::new(&leaf))?;
        match mkdirat(parent.as_raw_fd(), &leaf_name, 0o700) {
            Ok(()) => {
                created = Some((leaf, leaf_name));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(ReviewInspectionError::io(
                    "create a new inspection folder",
                    error,
                ));
            }
        }
    }
    let (leaf, leaf_name) = created.ok_or(ReviewInspectionError::Invalid(
        "one hundred inspection folders already used this review name",
    ))?;
    let directory = openat(parent.as_raw_fd(), &leaf_name, OPEN_DIRECTORY_FLAGS, 0)
        .map_err(|error| ReviewInspectionError::io("open the new inspection folder", error))?;
    let directory_metadata = directory
        .metadata()
        .map_err(|error| ReviewInspectionError::io("inspect the new inspection folder", error))?;
    if !directory_metadata.is_dir() || directory_metadata.permissions().mode() & 0o077 != 0 {
        return Err(ReviewInspectionError::Invalid(
            "the new inspection folder was not private",
        ));
    }
    create_file(
        &directory,
        "INCOMPLETE.txt",
        b"Mesh did not finish this review-copy export. Do not use these files for review.\n",
    )?;
    let mut files = Vec::with_capacity(copies.len());
    for copy in copies {
        let name = format!("{}.{}", title(copy.side), copy.extension);
        create_file(&directory, &name, copy.bytes)?;
        files.push(ReviewInspectionFile {
            side: copy.side,
            path: destination.join(&leaf).join(&name),
            version: copy.version.to_owned(),
            digest: copy.digest.to_owned(),
        });
    }
    create_file(
        &directory,
        "ABOUT.json",
        &manifest(bundle, target, object, copies),
    )?;
    unlinkat(
        directory.as_raw_fd(),
        &component(OsStr::new("INCOMPLETE.txt"))?,
    )
    .map_err(|error| ReviewInspectionError::io("complete the inspection folder", error))?;
    directory
        .sync_all()
        .map_err(|error| ReviewInspectionError::io("make the inspection folder durable", error))?;
    parent
        .sync_all()
        .map_err(|error| ReviewInspectionError::io("make the destination update durable", error))?;

    let current_parent = fs::symlink_metadata(destination)
        .map_err(|error| ReviewInspectionError::io("recheck the selected destination", error))?;
    if current_parent.file_type().is_symlink()
        || !current_parent.is_dir()
        || (current_parent.dev(), current_parent.ino()) != parent_identity
    {
        return Err(ReviewInspectionError::Invalid(
            "the selected destination changed while exact copies were written; Mesh did not reveal a path",
        ));
    }
    let named_directory = fs::symlink_metadata(destination.join(&leaf))
        .map_err(|error| ReviewInspectionError::io("recheck the inspection folder", error))?;
    if named_directory.file_type().is_symlink()
        || !named_directory.is_dir()
        || (named_directory.dev(), named_directory.ino())
            != (directory_metadata.dev(), directory_metadata.ino())
    {
        return Err(ReviewInspectionError::Invalid(
            "the selected destination changed while exact copies were written; Mesh did not reveal a path",
        ));
    }
    Ok(ReviewInspectionExport {
        directory: destination.join(leaf),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mesh-review-inspection-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).expect("create scratch");
        path
    }

    fn copies<'a>(before: &'a [u8], after: &'a [u8]) -> [ReviewInspectionCopy<'a>; 2] {
        const BEFORE_VERSION: &str =
            "1111111111111111111111111111111111111111111111111111111111111111";
        const BEFORE_DIGEST: &str =
            "2222222222222222222222222222222222222222222222222222222222222222";
        const AFTER_VERSION: &str =
            "3333333333333333333333333333333333333333333333333333333333333333";
        const AFTER_DIGEST: &str =
            "4444444444444444444444444444444444444444444444444444444444444444";
        [
            ReviewInspectionCopy {
                side: "before",
                reviewed_path: "finance/board-pack.pptx",
                extension: "pptx",
                version: BEFORE_VERSION,
                digest: BEFORE_DIGEST,
                bytes: before,
            },
            ReviewInspectionCopy {
                side: "after",
                reviewed_path: "finance/board-pack.pptx",
                extension: "pptx",
                version: AFTER_VERSION,
                digest: AFTER_DIGEST,
                bytes: after,
            },
        ]
    }

    #[test]
    fn exact_copies_are_new_read_only_and_never_overwrite_an_earlier_export() {
        let root = scratch("exact");
        let values = copies(b"before-presentation", b"after-presentation");
        let first = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &values,
        )
        .expect("first export");
        assert_eq!(
            fs::read(first.directory.join("Before.pptx")).unwrap(),
            b"before-presentation"
        );
        assert_eq!(
            fs::read(first.directory.join("After.pptx")).unwrap(),
            b"after-presentation"
        );
        assert_eq!(
            fs::metadata(first.directory.join("Before.pptx"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
        assert!(!first.directory.join("INCOMPLETE.txt").exists());
        let about = fs::read_to_string(first.directory.join("ABOUT.json")).unwrap();
        assert!(about.contains("\"working_folder_unchanged\":true"));
        assert!(about.contains("\"opens_document_content\":false"));

        let second = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &values,
        )
        .expect("second export");
        assert_ne!(first.directory, second.directory);
        assert_eq!(
            fs::read(first.directory.join("After.pptx")).unwrap(),
            b"after-presentation"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_replaced_destination_cannot_redirect_exact_review_bytes() {
        let root = scratch("replacement");
        let destination = root.join("chosen");
        let displaced = root.join("displaced");
        let outside = root.join("outside");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&outside).unwrap();
        let values = copies(b"before", b"after");
        let result = export_review_inspection_with_hook(
            &destination,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &values,
            || {
                fs::rename(&destination, &displaced).unwrap();
                symlink(&outside, &destination).unwrap();
            },
        );
        assert!(matches!(result, Err(ReviewInspectionError::Invalid(_))));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert!(displaced.join("Mesh review aaaaaaaaaaaa").exists());
        let _ = fs::remove_file(&destination);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linked_destinations_and_malformed_identities_fail_before_writes() {
        let root = scratch("refusal");
        let real = root.join("real");
        let linked = root.join("linked");
        fs::create_dir(&real).unwrap();
        symlink(&real, &linked).unwrap();
        let values = copies(b"before", b"after");
        assert!(matches!(
            export_review_inspection(
                &linked,
                &"aa".repeat(32),
                &"bb".repeat(32),
                &"cc".repeat(16),
                &values,
            ),
            Err(ReviewInspectionError::Invalid(_))
        ));
        assert!(matches!(
            export_review_inspection(
                &real,
                "not-a-bundle",
                &"bb".repeat(32),
                &"cc".repeat(16),
                &values
            ),
            Err(ReviewInspectionError::Invalid(_))
        ));
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
        let _ = fs::remove_dir_all(root);
    }
}
