//! Owner-only preference for automatic private capture of unambiguous native edits.
//!
//! This record is convenience state, never workspace or signing authority. Missing, malformed,
//! linked, or shared-permission state fails closed to an explicit error; the caller may then keep
//! automatic capture disabled. The actual save path still re-verifies the workspace installation,
//! every file, and the native tree immediately before signing.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const RECORD_NAME: &str = "native-capture-preference";
const ENABLED: &[u8] = b"mesh-desktop-native-capture/1\nauto-save-safe\n";
const DISABLED: &[u8] = b"mesh-desktop-native-capture/1\nreview-before-save\n";
const MAX_RECORD_BYTES: u64 = 128;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

/// Native-host-owned preference stored beside the desktop's other private navigation state.
#[derive(Clone, Debug)]
pub struct NativeCapturePreference {
    directory: PathBuf,
    record: PathBuf,
}

impl NativeCapturePreference {
    #[must_use]
    pub fn new(application_data: &Path) -> Self {
        Self {
            directory: application_data.to_path_buf(),
            record: application_data.join(RECORD_NAME),
        }
    }

    /// Read the exact preference. A missing record means the safer review-before-save mode.
    pub fn enabled(&self) -> Result<bool, NativeCapturePreferenceError> {
        let metadata = match fs::symlink_metadata(&self.record) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(NativeCapturePreferenceError::io(
                    "inspect preference",
                    error,
                ))
            }
        };
        validate_directory(&self.directory)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(NativeCapturePreferenceError::Invalid(
                "preference is not a regular file",
            ));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(NativeCapturePreferenceError::Invalid(
                "preference is not owner-only",
            ));
        }
        if metadata.len() > MAX_RECORD_BYTES {
            return Err(NativeCapturePreferenceError::Invalid(
                "preference is too large",
            ));
        }
        let file = File::open(&self.record)
            .map_err(|error| NativeCapturePreferenceError::io("open preference", error))?;
        let opened = file
            .metadata()
            .map_err(|error| NativeCapturePreferenceError::io("inspect open preference", error))?;
        if !opened.is_file()
            || opened.dev() != metadata.dev()
            || opened.ino() != metadata.ino()
            || opened.permissions().mode() & 0o077 != 0
        {
            return Err(NativeCapturePreferenceError::Invalid(
                "preference changed while it was opened",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| NativeCapturePreferenceError::io("read preference", error))?;
        match bytes.as_slice() {
            ENABLED => Ok(true),
            DISABLED => Ok(false),
            _ => Err(NativeCapturePreferenceError::Invalid(
                "preference content is not canonical",
            )),
        }
    }

    /// Atomically persist the person's selected mode in an owner-only record.
    pub fn set_enabled(&self, enabled: bool) -> Result<(), NativeCapturePreferenceError> {
        ensure_directory(&self.directory)?;
        let temporary = self.directory.join(format!(
            ".{RECORD_NAME}.{}-{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let bytes = if enabled { ENABLED } else { DISABLED };
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|error| {
                    NativeCapturePreferenceError::io("create temporary preference", error)
                })?;
            file.write_all(bytes).map_err(|error| {
                NativeCapturePreferenceError::io("write temporary preference", error)
            })?;
            file.sync_all().map_err(|error| {
                NativeCapturePreferenceError::io("sync temporary preference", error)
            })?;
            fs::rename(&temporary, &self.record)
                .map_err(|error| NativeCapturePreferenceError::io("publish preference", error))?;
            let directory = File::open(&self.directory).map_err(|error| {
                NativeCapturePreferenceError::io("open preference directory", error)
            })?;
            directory.sync_all().map_err(|error| {
                NativeCapturePreferenceError::io("sync preference directory", error)
            })?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        if self.enabled()? != enabled {
            return Err(NativeCapturePreferenceError::Invalid(
                "published preference did not read back exactly",
            ));
        }
        Ok(())
    }
}

fn ensure_directory(directory: &Path) -> Result<(), NativeCapturePreferenceError> {
    match fs::symlink_metadata(directory) {
        Ok(_) => validate_directory(directory),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(|error| {
                    NativeCapturePreferenceError::io("create preference directory", error)
                })?;
            validate_directory(directory)
        }
        Err(error) => Err(NativeCapturePreferenceError::io(
            "inspect preference directory",
            error,
        )),
    }
}

fn validate_directory(directory: &Path) -> Result<(), NativeCapturePreferenceError> {
    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| NativeCapturePreferenceError::io("inspect preference directory", error))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(NativeCapturePreferenceError::Invalid(
            "preference directory is not a real directory",
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(NativeCapturePreferenceError::Invalid(
            "preference directory is not owner-only",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub enum NativeCapturePreferenceError {
    Invalid(&'static str),
    Io {
        action: &'static str,
        source: std::io::Error,
    },
}

impl NativeCapturePreferenceError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}

impl fmt::Display for NativeCapturePreferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => {
                write!(formatter, "native capture preference refused: {reason}")
            }
            Self::Io { action, source } => write!(formatter, "could not {action}: {source}"),
        }
    }
}

impl std::error::Error for NativeCapturePreferenceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mesh-native-capture-preference-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("scratch");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("private scratch");
        path
    }

    #[test]
    fn missing_is_disabled_and_round_trip_is_exact() {
        let root = scratch("round-trip");
        let preference = NativeCapturePreference::new(&root);
        assert!(!preference.enabled().expect("missing disabled"));
        preference.set_enabled(true).expect("enable");
        assert!(preference.enabled().expect("read enabled"));
        assert_eq!(fs::read(root.join(RECORD_NAME)).expect("bytes"), ENABLED);
        preference.set_enabled(false).expect("disable");
        assert!(!preference.enabled().expect("read disabled"));
        assert_eq!(fs::read(root.join(RECORD_NAME)).expect("bytes"), DISABLED);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn malformed_shared_and_linked_records_fail_closed() {
        for (name, prepare) in [("malformed", 0_u8), ("shared", 1_u8), ("linked", 2_u8)] {
            let root = scratch(name);
            let record = root.join(RECORD_NAME);
            match prepare {
                0 => fs::write(&record, b"enabled\n").expect("malformed"),
                1 => {
                    fs::write(&record, ENABLED).expect("shared");
                    fs::set_permissions(&record, fs::Permissions::from_mode(0o644))
                        .expect("shared mode");
                }
                2 => {
                    let outside = root.join("outside");
                    fs::write(&outside, ENABLED).expect("outside");
                    symlink(&outside, &record).expect("link");
                }
                _ => unreachable!(),
            }
            let preference = NativeCapturePreference::new(&root);
            assert!(preference.enabled().is_err(), "{name} record was accepted");
            fs::remove_dir_all(root).expect("cleanup");
        }
    }
}
