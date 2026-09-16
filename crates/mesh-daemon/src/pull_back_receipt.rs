//! Durable local proof that one reviewed Pull-back installed an exact ordinary-folder entry.
//!
//! A byte match is not deletion authority: an unrelated file can independently acquire the same
//! bytes after a private Mesh path is retired. These immutable receipts live in the workspace's
//! private, descriptor-pinned namespace and bind the exact destination directory object, relative
//! path, saved identity, installed entry object, bytes, and portable executable state. They are
//! persisted while the future destination inode still has a private temporary name, so receipt
//! failure exposes no unproven ordinary-folder mutation and a crash after installation remains
//! recoverable. Cleanup still revalidates every fact immediately before removing anything.

use std::fs::Permissions;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mesh_cas::DurableFs as _;
use mesh_operations::ObjectId;
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};

use crate::root_authority::PinnedRootFs;

pub(crate) const RECEIPT_DIRECTORY_NAME: &str = "pull-back-receipts";
const RECEIPT_MAGIC: &[u8] = b"mesh.pull-back-receipt/1\0";
const KEY_MAGIC: &[u8] = b"mesh.pull-back-receipt-key/2\0";
static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
pub(crate) struct FileReceipt<'a> {
    pub workspace_installation: &'a str,
    pub target_root_installation: &'a str,
    pub path: &'a str,
    pub source_version: &'a str,
    pub source_digest: RecordDigest,
    pub source_executable: bool,
    pub target_entry_installation: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct DirectoryReceipt<'a> {
    pub workspace_installation: &'a str,
    pub target_root_installation: &'a str,
    pub path: &'a str,
    pub target_entry_installation: &'a str,
}

/// Durable authority tying genesis-import cleanup to the exact ordinary directory that supplied
/// those files. Imported object ancestry alone is not enough: another directory can independently
/// contain the same path and bytes.
#[derive(Clone, Copy)]
pub(crate) struct ImportOriginReceipt<'a> {
    pub workspace_installation: &'a str,
    pub target_root: &'a Path,
    pub target_root_installation: &'a str,
    pub object: ObjectId,
    pub path: &'a Path,
}

pub(crate) fn record_file(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipt: &FileReceipt<'_>,
) -> io::Result<()> {
    record(filesystem, storage_root, &file_bytes(receipt))
}

pub(crate) fn record_directory(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipt: &DirectoryReceipt<'_>,
) -> io::Result<()> {
    record(filesystem, storage_root, &directory_bytes(receipt))
}

pub(crate) fn proves_file(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipt: &FileReceipt<'_>,
) -> bool {
    proves(filesystem, storage_root, &file_bytes(receipt))
}

pub(crate) fn proves_directory(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipt: &DirectoryReceipt<'_>,
) -> bool {
    proves(filesystem, storage_root, &directory_bytes(receipt))
}

#[cfg(test)]
pub(crate) fn record_import_origin(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipt: &ImportOriginReceipt<'_>,
) -> io::Result<()> {
    record(filesystem, storage_root, &import_origin_bytes(receipt))
}

pub(crate) fn record_import_origins(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipts: &[ImportOriginReceipt<'_>],
) -> io::Result<()> {
    if receipts.is_empty() {
        return Ok(());
    }
    let directory = storage_root.join(RECEIPT_DIRECTORY_NAME);
    filesystem.create_dir_all(&directory)?;
    for receipt in receipts {
        record_without_sync(
            filesystem,
            storage_root,
            &directory,
            &import_origin_bytes(receipt),
        )?;
    }
    filesystem.sync_dir(&directory)
}

pub(crate) fn proves_import_origin(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    receipt: &ImportOriginReceipt<'_>,
) -> bool {
    proves(filesystem, storage_root, &import_origin_bytes(receipt))
}

fn record(filesystem: &PinnedRootFs, storage_root: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = storage_root.join(RECEIPT_DIRECTORY_NAME);
    filesystem.create_dir_all(&directory)?;
    record_without_sync(filesystem, storage_root, &directory, bytes)?;
    filesystem.sync_dir(&directory)
}

fn record_without_sync(
    filesystem: &PinnedRootFs,
    storage_root: &Path,
    directory: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    let key = receipt_key(bytes);
    let final_path = receipt_path(storage_root, bytes);
    if filesystem
        .read(&final_path)
        .is_ok_and(|existing| existing == bytes)
    {
        return Ok(());
    }
    let counter = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".{}.{}.{counter}.tmp", key, std::process::id()));
    filesystem.write_new_file(&temporary, bytes, Permissions::from_mode(0o600))?;
    if let Err(error) = filesystem.rename(&temporary, &final_path) {
        let _ = filesystem.remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn proves(filesystem: &PinnedRootFs, storage_root: &Path, expected: &[u8]) -> bool {
    filesystem
        .read(&receipt_path(storage_root, expected))
        .is_ok_and(|bytes| bytes == expected)
}

fn receipt_path(storage_root: &Path, bytes: &[u8]) -> PathBuf {
    storage_root
        .join(RECEIPT_DIRECTORY_NAME)
        .join(format!("{}.receipt", receipt_key(bytes)))
}

fn receipt_key(receipt: &[u8]) -> String {
    let mut bytes = KEY_MAGIC.to_vec();
    push_field(&mut bytes, receipt);
    Blake3::digest_bytes(&bytes).to_hex()
}

fn file_bytes(receipt: &FileReceipt<'_>) -> Vec<u8> {
    let mut bytes = RECEIPT_MAGIC.to_vec();
    push_field(&mut bytes, b"file");
    push_field(&mut bytes, receipt.workspace_installation.as_bytes());
    push_field(&mut bytes, receipt.target_root_installation.as_bytes());
    push_field(&mut bytes, receipt.path.as_bytes());
    push_field(&mut bytes, receipt.source_version.as_bytes());
    push_field(&mut bytes, receipt.source_digest.as_bytes());
    push_field(&mut bytes, &[u8::from(receipt.source_executable)]);
    push_field(&mut bytes, receipt.target_entry_installation.as_bytes());
    bytes
}

fn directory_bytes(receipt: &DirectoryReceipt<'_>) -> Vec<u8> {
    let mut bytes = RECEIPT_MAGIC.to_vec();
    push_field(&mut bytes, b"folder");
    push_field(&mut bytes, receipt.workspace_installation.as_bytes());
    push_field(&mut bytes, receipt.target_root_installation.as_bytes());
    push_field(&mut bytes, receipt.path.as_bytes());
    push_field(&mut bytes, receipt.target_entry_installation.as_bytes());
    bytes
}

fn import_origin_bytes(receipt: &ImportOriginReceipt<'_>) -> Vec<u8> {
    let mut bytes = RECEIPT_MAGIC.to_vec();
    push_field(&mut bytes, b"import-origin");
    push_field(&mut bytes, receipt.workspace_installation.as_bytes());
    push_field(&mut bytes, path_bytes(receipt.target_root).as_ref());
    push_field(&mut bytes, receipt.target_root_installation.as_bytes());
    push_field(&mut bytes, receipt.object.as_bytes());
    push_field(&mut bytes, path_bytes(receipt.path).as_ref());
    bytes
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.to_string_lossy().into_owned().into_bytes())
}

fn push_field(bytes: &mut Vec<u8>, field: &[u8]) {
    bytes.extend_from_slice(&u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
    bytes.extend_from_slice(field);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_file::managed_directory_identity;
    use crate::root_authority::PinnedWorkspaceRoot;
    use std::os::unix::fs::symlink;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mesh-pull-back-receipt-{name}-{}-{}",
            std::process::id(),
            TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("scratch root");
        root
    }

    #[test]
    fn exact_receipts_are_canonical_immutable_and_never_follow_a_symlink() {
        let root = scratch("canonical");
        let pinned = PinnedWorkspaceRoot::open(root.clone()).expect("pin root");
        let filesystem = pinned.filesystem();
        let first = FileReceipt {
            workspace_installation: "workspace-a",
            target_root_installation: "ordinary-root-a",
            path: "notes.txt",
            source_version: "version-a",
            source_digest: RecordDigest::from_bytes([0x41; 32]),
            source_executable: false,
            target_entry_installation: "file-inode-a",
        };
        record_file(&filesystem, &root, &first).expect("record exact receipt");
        assert!(proves_file(&filesystem, &root, &first));

        let second = FileReceipt {
            source_version: "version-b",
            source_digest: RecordDigest::from_bytes([0x42; 32]),
            target_entry_installation: "file-inode-b",
            ..first
        };
        record_file(&filesystem, &root, &second).expect("replace exact receipt");
        assert!(
            proves_file(&filesystem, &root, &first),
            "historical exact receipts remain immutable for safe failed-retry recovery"
        );
        assert!(proves_file(&filesystem, &root, &second));
        assert!(!proves_file(
            &filesystem,
            &root,
            &FileReceipt {
                workspace_installation: "copied-workspace",
                ..second
            }
        ));

        let path = receipt_path(&root, &file_bytes(&second));
        let exact = std::fs::read(&path).expect("receipt bytes");
        std::fs::remove_file(&path).expect("remove receipt");
        let outside = root.join("outside-receipt");
        std::fs::write(&outside, exact).expect("outside bytes");
        symlink(&outside, &path).expect("replace receipt with symlink");
        assert!(!proves_file(&filesystem, &root, &second));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn directory_receipt_binds_workspace_root_path_and_entry_identity() {
        let root = scratch("directory");
        let pinned = PinnedWorkspaceRoot::open(root.clone()).expect("pin root");
        let filesystem = pinned.filesystem();
        let receipt = DirectoryReceipt {
            workspace_installation: "workspace-a",
            target_root_installation: "ordinary-root-a",
            path: "generated/reports",
            target_entry_installation: "directory-inode-a",
        };
        record_directory(&filesystem, &root, &receipt).expect("record directory receipt");
        assert!(proves_directory(&filesystem, &root, &receipt));
        assert!(!proves_directory(
            &filesystem,
            &root,
            &DirectoryReceipt {
                target_entry_installation: "directory-inode-b",
                ..receipt
            }
        ));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn import_origin_receipt_never_transfers_to_an_identical_unrelated_directory() {
        let root = scratch("import-origin");
        let original = root.join("original");
        let unrelated = root.join("unrelated");
        std::fs::create_dir(&original).expect("original");
        std::fs::create_dir(&unrelated).expect("unrelated");
        let original = std::fs::canonicalize(original).expect("canonical original");
        let unrelated = std::fs::canonicalize(unrelated).expect("canonical unrelated");
        let original_installation = managed_directory_identity(&original)
            .expect("original identity")
            .token();
        let unrelated_installation = managed_directory_identity(&unrelated)
            .expect("unrelated identity")
            .token();
        let pinned = PinnedWorkspaceRoot::open(root.clone()).expect("pin root");
        let filesystem = pinned.filesystem();
        let exact = ImportOriginReceipt {
            workspace_installation: "workspace-a",
            target_root: &original,
            target_root_installation: &original_installation,
            object: ObjectId::from_bytes([0x11; 16]),
            path: Path::new("docs/original.txt"),
        };
        record_import_origin(&filesystem, &root, &exact).expect("record import origin");
        assert!(proves_import_origin(&filesystem, &root, &exact));
        assert!(!proves_import_origin(
            &filesystem,
            &root,
            &ImportOriginReceipt {
                target_root: &unrelated,
                target_root_installation: &unrelated_installation,
                ..exact
            }
        ));
        assert!(!proves_import_origin(
            &filesystem,
            &root,
            &ImportOriginReceipt {
                workspace_installation: "workspace-b",
                ..exact
            }
        ));
        assert!(!proves_import_origin(
            &filesystem,
            &root,
            &ImportOriginReceipt {
                object: ObjectId::from_bytes([0x22; 16]),
                ..exact
            }
        ));
        assert!(!proves_import_origin(
            &filesystem,
            &root,
            &ImportOriginReceipt {
                path: Path::new("docs/renamed.txt"),
                ..exact
            }
        ));
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
