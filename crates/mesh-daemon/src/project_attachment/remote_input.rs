//! Saved-input export uses existing retained attachment authority, never a live project scan.
use super::ProvisionedAttachment;
use crate::fleet::RemoteInputSource;
use std::io;

impl ProvisionedAttachment {
    /// Prepare bounded read-only export of one exact saved version.
    ///
    /// Native callers must authorize the selected project/version and destination peer separately.
    /// Preparation uses normal retained-history inspection, including disposable-index recovery. The returned
    /// handle owns directory pins and immutable metadata, not that lock or an execution context;
    /// network pacing therefore cannot hold up a subsequent capture. Reads never use current files.
    /// Missing, foreign, changed or unsupported history refuses instead of exporting a partial tree.
    pub fn prepare_remote_input(&self, version: &str) -> io::Result<RemoteInputSource> {
        self.attachment.inspect_saved(
            self.metadata_path(),
            self.store.clone(),
            version,
            |workspace, operation| workspace.remote_input_source(operation),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{RemoteAssignment, RemoteInputEntry, RemoteInputReceiver};
    use crate::project_attachment::{AttachmentStorage, ObservationLimits};
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_cas::{Cas, Digest32, StoreLayout};
    use mesh_store::RecordDigest;
    use mesh_types::{PublicKey, Signature};
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        attached: ProvisionedAttachment,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-remote-export-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let source = root.join("project");
            fs::create_dir_all(source.join("dir/empty")).unwrap();
            fs::write(source.join("zero"), []).unwrap();
            fs::write(source.join("dir/run"), b"#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(source.join("dir/run"), fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(source.join("binary"), vec![0xff; 140_000]).unwrap();
            let metadata = root.join("metadata");
            fs::create_dir(&metadata).unwrap();
            let storage = AttachmentStorage::open(&metadata).unwrap();
            let attached = storage.provision(&source).unwrap();
            Self {
                root,
                source,
                attached,
            }
        }
        fn save(&self) -> String {
            let capture = self
                .attached
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let key = SigningKey::from_bytes(&[74; 32]);
            self.attached
                .project()
                .save_capture(
                    self.attached.metadata_path(),
                    &capture,
                    PublicKey::from_bytes(key.verifying_key().to_bytes()),
                    |body| {
                        Ok::<_, String>(Signature::from_bytes(key.sign(body.as_bytes()).to_bytes()))
                    },
                )
                .unwrap()
                .operation()
                .to_string()
        }
        fn chunk(&self, source: &RemoteInputSource) -> (Digest32, PathBuf) {
            let digest = source
                .manifest()
                .entries()
                .iter()
                .find_map(|entry| match entry {
                    RemoteInputEntry::File { path, chunks, .. } if path == "binary" => {
                        Some(chunks[0].digest)
                    }
                    _ => None,
                })
                .unwrap();
            (
                digest,
                StoreLayout::new(self.attached.metadata_path()).chunk_path(&digest),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn saved_remote_export_preserves_exact_tree_and_does_not_hold_capture_lock() {
        let fixture = Fixture::new();
        let version = fixture.save();
        let source = fixture.attached.prepare_remote_input(&version).unwrap();
        let manifest = source.manifest().clone();
        assert_eq!(manifest.input().to_string(), version);
        assert!(manifest.entries().iter().any(|entry| matches!(entry,
            RemoteInputEntry::Directory { path } if path == "dir/empty")));
        assert!(manifest.entries().iter().any(|entry| matches!(entry,
            RemoteInputEntry::File { path, executable: true, .. } if path == "dir/run")));
        assert!(manifest.entries().iter().any(|entry| matches!(entry,
            RemoteInputEntry::File { path, chunks, .. } if path == "zero" && chunks.is_empty())));

        // A new capture completes while the old export handle is still alive. Neither its saved
        // input nor the source project is switched to the exported version.
        fs::write(fixture.source.join("binary"), b"new live editor work").unwrap();
        let newer = fixture.save();
        assert_ne!(newer, version);
        assert_eq!(source.manifest(), &manifest);
        let destination = Cas::open(fixture.root.join("receiver")).unwrap();
        let assignment = RemoteAssignment {
            id: "remote-export".into(),
            worker_key: "ab".repeat(32),
            input: manifest.input(),
            bundle: manifest.bundle(),
            lease_sequence: 1,
            lease_until_ms: 100,
        };
        let mut receiver =
            RemoteInputReceiver::new(manifest.clone(), &assignment, &destination).unwrap();
        let chunks: BTreeMap<_, _> = manifest
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                RemoteInputEntry::File { chunks, .. } => {
                    Some(chunks.iter().map(|part| (part.digest, part.bytes)))
                }
                _ => None,
            })
            .flatten()
            .collect();
        for (digest, size) in chunks {
            let bytes = source.read_chunk(digest).unwrap();
            assert_eq!(bytes.len() as u64, size);
            let mut offset = 0;
            for part in bytes.chunks(65_536) {
                receiver
                    .accept(digest, offset, part, offset + part.len() as u64 == size)
                    .unwrap();
                offset += part.len() as u64;
            }
        }
        receiver.verify_complete().unwrap();
        let (binary, _) = fixture.chunk(&source);
        assert!(source
            .read_chunk(binary)
            .unwrap()
            .iter()
            .all(|byte| *byte == 0xff));
        assert_eq!(
            fs::read(fixture.source.join("binary")).unwrap(),
            b"new live editor work"
        );
        assert!(source.read_chunk(Digest32::from_bytes([99; 32])).is_err());
        drop(source);
        let reopened = fixture.attached.prepare_remote_input(&version).unwrap();
        assert_eq!(reopened.manifest(), &manifest);
    }

    #[test]
    fn saved_remote_export_refuses_foreign_noncanonical_and_corrupt_content_without_repair() {
        let fixture = Fixture::new();
        let version = fixture.save();
        assert!(fixture
            .attached
            .prepare_remote_input(&RecordDigest::from_bytes([99; 32]).to_string())
            .is_err());
        assert!(fixture
            .attached
            .prepare_remote_input(&version.to_uppercase())
            .is_err());
        let source = fixture.attached.prepare_remote_input(&version).unwrap();
        let (digest, path) = fixture.chunk(&source);
        let original = fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        corrupt[0] ^= 1;
        fs::write(&path, &corrupt).unwrap();
        assert!(source.read_chunk(digest).is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            corrupt,
            "read-only refusal must not quarantine or repair"
        );
        fs::write(&path, &original).unwrap();
        assert_eq!(source.read_chunk(digest).unwrap(), original);
        fs::remove_file(&path).unwrap();
        assert!(source.read_chunk(digest).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn saved_remote_export_refuses_oversized_linked_or_replaced_storage() {
        let fixture = Fixture::new();
        let source = fixture
            .attached
            .prepare_remote_input(&fixture.save())
            .unwrap();
        let (digest, path) = fixture.chunk(&source);
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        assert!(source.read_chunk(digest).is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), 8 * 1024 * 1024);
        fs::remove_file(&path).unwrap();
        symlink(fixture.source.join("binary"), &path).unwrap();
        assert!(source.read_chunk(digest).is_err());
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        let metadata = fixture.attached.metadata_path();
        let moved = fixture.root.join("retained-store");
        fs::rename(metadata, &moved).unwrap();
        fs::create_dir(metadata).unwrap();
        assert!(source.read_chunk(digest).is_err());
        assert_eq!(fs::read_dir(metadata).unwrap().count(), 0);
    }
}
