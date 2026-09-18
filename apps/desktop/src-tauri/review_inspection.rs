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
use std::time::SystemTime;

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

#[allow(unsafe_code)]
fn unlinkat_directory(directory: RawFd, name: &CStr) -> std::io::Result<()> {
    unsafe extern "C" {
        fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    }
    const AT_REMOVEDIR: i32 = 0x80;
    // SAFETY: `name` is a live C string and `AT_REMOVEDIR` can remove only an empty directory.
    if unsafe { unlinkat(directory, name.as_ptr(), AT_REMOVEDIR) } != 0 {
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

fn exact_object_keys(value: &Json, expected: &[&str]) -> bool {
    let Json::Object(fields) = value else {
        return false;
    };
    fields.len() == expected.len()
        && expected
            .iter()
            .all(|expected| fields.iter().any(|(actual, _)| actual == expected))
}

fn admitted_copy_extension(extension: &str) -> bool {
    matches!(
        extension,
        "pdf"
            | "pptx"
            | "docx"
            | "xlsx"
            | "png"
            | "jpg"
            | "gif"
            | "webp"
            | "txt"
            | "md"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "csv"
            | "log"
            | "rs"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "css"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "py"
            | "rb"
            | "go"
            | "java"
            | "swift"
            | "kt"
            | "bin"
    )
}

fn normalized_reviewed_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.chars().any(char::is_control)
        && Path::new(value)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn manifest_owned_files(
    manifest: &Json,
    directory_name: &str,
) -> Option<std::collections::BTreeMap<String, u64>> {
    if !exact_object_keys(
        manifest,
        &[
            "schema",
            "review_bundle",
            "subject_operation",
            "object_id",
            "files",
            "working_folder_unchanged",
            "opens_document_content",
            "approval_input",
        ],
    ) || manifest.get("schema").and_then(Json::as_text) != Some(SCHEMA)
        || manifest
            .get("working_folder_unchanged")
            .and_then(Json::as_bool)
            != Some(true)
        || manifest
            .get("opens_document_content")
            .and_then(Json::as_bool)
            != Some(false)
        || manifest.get("approval_input").and_then(Json::as_text) != Some("exact-saved-bytes")
    {
        return None;
    }
    let bundle = manifest.get("review_bundle")?.as_text()?;
    let operation = manifest.get("subject_operation")?.as_text()?;
    let object = manifest.get("object_id")?.as_text()?;
    if !canonical_hex(bundle, 64) || !canonical_hex(operation, 64) || !canonical_hex(object, 32) {
        return None;
    }
    let prefix = format!("Mesh review {}", &bundle[..12]);
    if directory_name != prefix {
        let attempt = directory_name.strip_prefix(&format!("{prefix} "))?;
        let attempt = attempt.parse::<usize>().ok()?;
        if !(2..=MAX_EXPORT_ATTEMPTS).contains(&attempt)
            || attempt.to_string() != directory_name[(prefix.len() + 1)..]
        {
            return None;
        }
    }

    let files = manifest.get("files")?.as_array()?;
    if files.is_empty() || files.len() > MAX_COPIES {
        return None;
    }
    let mut sides = std::collections::HashSet::new();
    let mut owned = std::collections::BTreeMap::new();
    for file in files {
        if !exact_object_keys(
            file,
            &[
                "side",
                "file",
                "reviewed_path",
                "version_id",
                "content_digest",
                "bytes",
                "read_only",
            ],
        ) {
            return None;
        }
        let side = file.get("side")?.as_text()?;
        let name = file.get("file")?.as_text()?;
        let reviewed_path = file.get("reviewed_path")?.as_text()?;
        let version = file.get("version_id")?.as_text()?;
        let digest = file.get("content_digest")?.as_text()?;
        let byte_length = file.get("bytes")?.as_text()?.parse::<u64>().ok()?;
        if !matches!(side, "before" | "after")
            || !sides.insert(side)
            || file.get("read_only")?.as_bool() != Some(true)
            || !normalized_reviewed_path(reviewed_path)
            || !canonical_hex(version, 64)
            || !canonical_hex(digest, 64)
        {
            return None;
        }
        let copy_extension = name.strip_prefix(title(side))?.strip_prefix('.')?;
        if !admitted_copy_extension(copy_extension) {
            return None;
        }
        if owned.insert(name.to_owned(), byte_length).is_some() {
            return None;
        }
    }
    Some(owned)
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
    for copy in copies {
        if !matches!(copy.side, "before" | "after")
            || !sides.insert(copy.side)
            || !admitted_copy_extension(copy.extension)
            || !canonical_hex(copy.version, 64)
            || !canonical_hex(copy.digest, 64)
        {
            return Err(ReviewInspectionError::Invalid(
                "one reviewed copy was malformed or inconsistent",
            ));
        }
    }
    Ok(())
}

struct PrunableInspection {
    path: PathBuf,
    modified: SystemTime,
    device: u64,
    inode: u64,
    children: Vec<std::ffi::OsString>,
}

fn prunable_app_owned_inspection(path: &Path) -> Option<PrunableInspection> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return None,
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || path.join("INCOMPLETE.txt").exists()
    {
        return None;
    }
    let about_path = path.join("ABOUT.json");
    let about_metadata = match fs::symlink_metadata(&about_path) {
        Ok(metadata) => metadata,
        Err(_) => return None,
    };
    if about_metadata.file_type().is_symlink()
        || !about_metadata.is_file()
        || about_metadata.len() > 16 * 1024
    {
        return None;
    }
    let about = match fs::read_to_string(&about_path) {
        Ok(about) => about,
        Err(_) => return None,
    };
    let manifest = Json::parse(&about).ok()?;
    let directory_name = path.file_name()?.to_str()?;
    let mut owned_files = manifest_owned_files(&manifest, directory_name)?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return None,
    };
    let mut children = Vec::new();
    let mut saw_about = false;
    for result in entries {
        let entry = result.ok()?;
        let name = entry.file_name();
        let name_text = name.to_str()?;
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => return None,
        };
        if !file_type.is_file() || file_type.is_symlink() {
            return None;
        }
        let entry_metadata = entry.metadata().ok()?;
        if entry_metadata.permissions().mode() & 0o777 != 0o400 {
            return None;
        }
        if name_text == "ABOUT.json" {
            if saw_about || entry_metadata.len() != about_metadata.len() {
                return None;
            }
            saw_about = true;
        } else if owned_files.remove(name_text) != Some(entry_metadata.len()) {
            return None;
        }
        children.push(name);
    }
    if !saw_about
        || !owned_files.is_empty()
        || children.len() < 2
        || children.len() > MAX_COPIES + 1
    {
        return None;
    }
    Some(PrunableInspection {
        path: path.to_path_buf(),
        modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        device: metadata.dev(),
        inode: metadata.ino(),
        children,
    })
}

#[cfg(test)]
fn completed_app_owned_inspection(path: &Path) -> bool {
    prunable_app_owned_inspection(path).is_some()
}

/// Retain only a bounded number of completed, schema-marked inspection folders in Mesh-owned
/// private storage. Unfinished, malformed, linked, or neighboring user-created entries are never
/// deletion candidates.
pub(crate) fn prune_app_owned_review_inspections(
    destination: &Path,
    retain: usize,
) -> Result<(), ReviewInspectionError> {
    prune_app_owned_review_inspections_with_hook(destination, retain, |_| {})
}

fn prune_app_owned_review_inspections_with_hook(
    destination: &Path,
    retain: usize,
    mut before_delete: impl FnMut(&Path),
) -> Result<(), ReviewInspectionError> {
    let metadata = fs::symlink_metadata(destination)
        .map_err(|error| ReviewInspectionError::io("inspect private review storage", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ReviewInspectionError::Invalid(
            "private review storage was not a real directory",
        ));
    }
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_DIRECTORY_FLAGS)
        .open(destination)
        .map_err(|error| ReviewInspectionError::io("open private review storage", error))?;
    let mut candidates = Vec::new();
    for result in fs::read_dir(destination)
        .map_err(|error| ReviewInspectionError::io("list private review storage", error))?
    {
        let entry = result
            .map_err(|error| ReviewInspectionError::io("read private review storage", error))?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("Mesh review "))
        {
            if let Some(candidate) = prunable_app_owned_inspection(&path) {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    let remove_count = candidates.len().saturating_sub(retain);
    for candidate in candidates.into_iter().take(remove_count) {
        let leaf = component(
            candidate
                .path
                .file_name()
                .ok_or(ReviewInspectionError::Invalid(
                    "a review-copy name was missing",
                ))?,
        )?;
        let directory = match openat(parent.as_raw_fd(), &leaf, OPEN_DIRECTORY_FLAGS, 0) {
            Ok(directory) => directory,
            Err(_) => continue,
        };
        let opened = directory
            .metadata()
            .map_err(|error| ReviewInspectionError::io("recheck an old review copy", error))?;
        if !opened.is_dir() || (opened.dev(), opened.ino()) != (candidate.device, candidate.inode) {
            continue;
        }
        before_delete(&candidate.path);
        let current = match fs::symlink_metadata(&candidate.path) {
            Ok(current) => current,
            Err(_) => continue,
        };
        if current.file_type().is_symlink()
            || !current.is_dir()
            || (current.dev(), current.ino()) != (candidate.device, candidate.inode)
        {
            continue;
        }
        for child in &candidate.children {
            unlinkat(directory.as_raw_fd(), &component(child)?).map_err(|error| {
                ReviewInspectionError::io("prune an old review-copy file", error)
            })?;
        }
        directory
            .sync_all()
            .map_err(|error| ReviewInspectionError::io("sync a pruned review copy", error))?;
        let current = match fs::symlink_metadata(&candidate.path) {
            Ok(current) => current,
            Err(_) => continue,
        };
        if current.file_type().is_symlink()
            || !current.is_dir()
            || (current.dev(), current.ino()) != (candidate.device, candidate.inode)
        {
            continue;
        }
        unlinkat_directory(parent.as_raw_fd(), &leaf)
            .map_err(|error| ReviewInspectionError::io("remove an empty old review copy", error))?;
    }
    parent
        .sync_all()
        .map_err(|error| ReviewInspectionError::io("sync private review storage", error))?;
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
    fn exact_text_copy_keeps_a_safe_review_extension_and_read_only_bytes() {
        const VERSION: &str = "1111111111111111111111111111111111111111111111111111111111111111";
        const DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
        let root = scratch("text");
        let copy = [ReviewInspectionCopy {
            side: "after",
            reviewed_path: "src/lib.rs",
            extension: "rs",
            version: VERSION,
            digest: DIGEST,
            bytes: b"pub fn exact() {}\n",
        }];
        let export = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &copy,
        )
        .expect("text export");
        let path = export.directory.join("After.rs");
        assert_eq!(fs::read(&path).unwrap(), b"pub fn exact() {}\n");
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_image_sides_may_keep_different_admitted_extensions() {
        let root = scratch("mixed-image-extensions");
        let base = copies(b"\x89PNG\r\n\x1a\nbefore", b"\xff\xd8\xffafter");
        let values = [
            ReviewInspectionCopy {
                extension: "png",
                reviewed_path: "assets/hero.png",
                ..base[0]
            },
            ReviewInspectionCopy {
                extension: "jpg",
                reviewed_path: "assets/hero.jpg",
                ..base[1]
            },
        ];
        let exported = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &values,
        )
        .expect("mixed exact image copies");
        assert!(exported.directory.join("Before.png").exists());
        assert!(exported.directory.join("After.jpg").exists());
        assert!(completed_app_owned_inspection(&exported.directory));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_saved_side_is_a_valid_exact_read_only_copy() {
        const VERSION: &str = "1111111111111111111111111111111111111111111111111111111111111111";
        const DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
        let root = scratch("empty");
        let copy = [ReviewInspectionCopy {
            side: "before",
            reviewed_path: "empty.txt",
            extension: "txt",
            version: VERSION,
            digest: DIGEST,
            bytes: b"",
        }];
        let export = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &copy,
        )
        .expect("empty exact copy");
        let path = export.directory.join("Before.txt");
        assert_eq!(fs::read(&path).unwrap(), b"");
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn opaque_binary_side_can_be_materialized_for_reveal_without_executable_suffix() {
        const VERSION: &str = "1111111111111111111111111111111111111111111111111111111111111111";
        const DIGEST: &str = "2222222222222222222222222222222222222222222222222222222222222222";
        let root = scratch("binary");
        let copy = [ReviewInspectionCopy {
            side: "after",
            reviewed_path: ".DS_Store",
            extension: "bin",
            version: VERSION,
            digest: DIGEST,
            bytes: b"\0opaque\xffbytes",
        }];
        let export = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &copy,
        )
        .expect("opaque exact copy");
        let path = export.directory.join("After.bin");
        assert_eq!(fs::read(&path).unwrap(), b"\0opaque\xffbytes");
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o400
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

    #[test]
    fn private_retention_prunes_only_completed_schema_owned_copies() {
        let root = scratch("retention");
        let values = copies(b"before", b"after");
        for _ in 0..3 {
            export_review_inspection(
                &root,
                &"aa".repeat(32),
                &"bb".repeat(32),
                &"cc".repeat(16),
                &values,
            )
            .expect("completed private copy");
        }
        let neighbor = root.join("user neighbor");
        fs::create_dir(&neighbor).unwrap();
        fs::write(neighbor.join("keep.txt"), b"keep").unwrap();
        let incomplete = root.join("Mesh review incomplete");
        fs::create_dir(&incomplete).unwrap();
        fs::write(incomplete.join("INCOMPLETE.txt"), b"keep").unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        let linked = root.join("Mesh review linked");
        symlink(&outside, &linked).unwrap();

        prune_app_owned_review_inspections(&root, 1).expect("bounded retention");

        let completed = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| completed_app_owned_inspection(path))
            .count();
        assert_eq!(completed, 1);
        assert_eq!(fs::read(neighbor.join("keep.txt")).unwrap(), b"keep");
        assert!(incomplete.join("INCOMPLETE.txt").exists());
        assert!(fs::symlink_metadata(&linked)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(outside.exists());
        let _ = fs::remove_file(linked);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn private_retention_refuses_a_replaced_directory_and_structural_manifest_decoy() {
        let root = scratch("retention-race");
        let values = copies(b"before", b"after");
        let export = export_review_inspection(
            &root,
            &"aa".repeat(32),
            &"bb".repeat(32),
            &"cc".repeat(16),
            &values,
        )
        .expect("completed private copy");
        let displaced = root.join("displaced exact copy");
        let replacement = export.directory.clone();
        prune_app_owned_review_inspections_with_hook(&root, 0, |selected| {
            assert_eq!(selected, replacement);
            fs::rename(selected, &displaced).unwrap();
            fs::create_dir(selected).unwrap();
            fs::set_permissions(selected, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(selected.join("user.txt"), b"replacement survives").unwrap();
        })
        .expect("replacement-safe pruning");
        assert_eq!(
            fs::read(replacement.join("user.txt")).unwrap(),
            b"replacement survives"
        );
        assert!(displaced.join("ABOUT.json").exists());
        assert!(displaced.join("Before.pptx").exists());

        let decoy = root.join("Mesh review aaaaaaaaaaaa 2");
        fs::create_dir(&decoy).unwrap();
        fs::set_permissions(&decoy, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            decoy.join("ABOUT.json"),
            b"{\"schema\":\"mesh-review-inspection/v1\",\"working_folder_unchanged\":true,\"opens_document_content\":false,\"approval_input\":\"exact-saved-bytes\"}",
        )
        .unwrap();
        fs::write(decoy.join("After.bin"), b"user bytes").unwrap();
        fs::set_permissions(decoy.join("ABOUT.json"), fs::Permissions::from_mode(0o400)).unwrap();
        fs::set_permissions(decoy.join("After.bin"), fs::Permissions::from_mode(0o400)).unwrap();
        assert!(!completed_app_owned_inspection(&decoy));
        prune_app_owned_review_inspections(&root, 0).expect("decoy-safe pruning");
        assert_eq!(fs::read(decoy.join("After.bin")).unwrap(), b"user bytes");
        let _ = fs::remove_dir_all(root);
    }
}
