//! Native read proof. Readable policy records are not consumption or publication authority.
use super::{
    dependency_enrollment::read_private_in_store,
    dependency_transaction::{digest, hash, intent_bytes, read_payload, text},
    history::{verify_history_binding, HISTORY},
    invalid, read_receipt, ProjectAttachment,
};
use crate::{
    dependency_policy::{DependencyPolicyHistory, NativeDependencyBinding},
    ipc::Json,
    root_authority::{PinnedRootFs, PinnedWorkspaceRoot},
    workspace::{workspace_installation, OpenWorkspace, RECORD_FILE_NAME},
    TrustedReviewers,
};
use mesh_cas::{Blake3, Cas};
use mesh_store::{scan_journal, RecordDigest, StoredRecord};
use std::{
    fs::File,
    io::{self, Read as _},
    os::unix::fs::MetadataExt as _,
    path::Path,
};
const MAX_BASE: usize = 64 * 1024 * 1024;
const MAX_HISTORY: usize = MAX_BASE + 16 * 1024 * 1024;

/// Constructible only after complete native registration, enrollment and payload validation.
/// The workspace opener checks even an empty/replaced journal against this exact read proof.
#[derive(PartialEq, Eq)]
pub(crate) struct VerifiedDependencyRead {
    store: (u64, u64),
    journal: (u64, u64),
    bytes: RecordDigest,
    binding: NativeDependencyBinding,
    policy: DependencyPolicyHistory,
}
impl VerifiedDependencyRead {
    pub(super) fn binding(&self) -> NativeDependencyBinding {
        self.binding
    }
    pub(super) fn policy(&self) -> &DependencyPolicyHistory {
        &self.policy
    }

    pub(crate) fn verify(
        &self,
        store: &PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
    ) -> io::Result<()> {
        store.ensure_namespace_identity()?;
        let held = file.metadata()?;
        let named = store
            .filesystem()
            .inspect_entry(Path::new(RECORD_FILE_NAME))?
            .metadata()?;
        if store.identity()? != self.store
            || !held.is_file()
            || held.nlink() != 1
            || (held.dev(), held.ino()) != self.journal
            || (named.dev(), named.ino()) != self.journal
            || hash(bytes) != self.bytes
        {
            return Err(invalid("validated dependency history changed"));
        }
        Ok(())
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

impl ProjectAttachment {
    pub(super) fn read_configuration(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
    ) -> io::Result<(String, Option<VerifiedDependencyRead>)> {
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        let receipt = self.receipt()?.encode();
        if read_receipt(store)? != receipt {
            return Err(invalid("history registration changed"));
        }
        let marker = read_private_in_store(store, HISTORY)?;
        let parsed = Json::parse(&marker).map_err(error)?;
        if parsed.get("schema").and_then(Json::as_text) != Some("mesh.attachment-history/v3") {
            let (configuration, _) =
                self.history_configuration_with_previous(store, None, Some(marker))?;
            return Ok((configuration, None));
        }
        let basis = text(&parsed, "previous_binding")?;
        let authority = digest(text(&parsed, "dependency_authority")?)?;
        let canonical = Json::object([
            ("schema", Json::text("mesh.attachment-history/v3")),
            ("previous_binding", Json::text(basis)),
            ("dependency_authority", Json::text(authority.to_hex())),
        ])
        .encode();
        if canonical != marker {
            return Err(invalid("noncanonical dependency fence"));
        }
        let (configuration, _) =
            self.history_configuration_with_previous(store, None, Some(basis.to_owned()))?;
        let identity = store.identity()?;
        let installation = workspace_installation(identity, identity);
        crate::DependencyEnrollmentFence::verify_while_initialized(
            metadata,
            &installation,
            authority,
        )
        .map_err(error)?;
        let installation = digest(
            installation
                .strip_prefix("blake3:")
                .ok_or_else(|| invalid("invalid native installation"))?,
        )?;
        let project = hash(receipt.as_bytes());
        let cas =
            Cas::<PinnedRootFs, Blake3>::with_filesystem(metadata, store.filesystem().read_only())
                .map_err(error)?;
        let intent = read_payload(&cas, authority, 4096)?;
        let decoded = Json::parse(std::str::from_utf8(&intent).map_err(error)?).map_err(error)?;
        let base_len = decoded
            .get("journal_bytes")
            .and_then(Json::as_u64)
            .filter(|n| *n <= MAX_BASE as u64)
            .ok_or_else(|| invalid("invalid enrollment boundary"))? as usize;
        let base_digest = digest(text(&decoded, "journal_digest")?)?;
        let mut journal = store
            .filesystem()
            .read_only()
            .read_file(Path::new(RECORD_FILE_NAME))?;
        let file = journal.metadata()?;
        let journal_identity = (file.dev(), file.ino());
        let mut bytes = Vec::new();
        (&mut journal)
            .take((MAX_HISTORY + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_HISTORY
            || base_len >= bytes.len()
            || intent
                != intent_bytes(
                    project,
                    installation,
                    journal_identity,
                    base_len as u64,
                    base_digest,
                )
            || hash(&bytes[..base_len]) != base_digest
        {
            return Err(invalid("dependency intent does not match native history"));
        }
        let base = scan_journal(&bytes[..base_len]).map_err(error)?;
        if base.tail().is_fragment()
            || base
                .records()
                .iter()
                .any(|r| matches!(r, StoredRecord::Dependency(_)))
        {
            return Err(invalid("invalid legacy enrollment prefix"));
        }
        let suffix = scan_journal(&bytes[base_len..]).map_err(error)?;
        if suffix.tail().is_fragment()
            || !matches!(suffix.records().first(), Some(StoredRecord::Dependency(_)))
        {
            return Err(invalid("incomplete dependency enrollment"));
        }
        let mut policy = DependencyPolicyHistory::new(NativeDependencyBinding {
            authority,
            project,
            installation,
        })
        .map_err(error)?;
        for record in suffix.records() {
            match record {
                StoredRecord::Dependency(record) => {
                    policy
                        .apply(*record, &read_payload(&cas, record.payload, 65_536)?)
                        .map_err(error)?;
                }
                // A valid legacy receipt alone cannot establish a dependency-aware publication.
                StoredRecord::Approval(_) => {
                    return Err(invalid(
                        "dependency publication verification is unavailable",
                    ))
                }
                _ => {}
            }
        }
        let proof = VerifiedDependencyRead {
            store: identity,
            journal: journal_identity,
            bytes: hash(&bytes),
            binding: NativeDependencyBinding {
                authority,
                project,
                installation,
            },
            policy,
        };
        proof.verify(store, &journal, &bytes)?;
        Ok((configuration, Some(proof)))
    }

    pub(super) fn with_read_history<T>(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        trusted: &TrustedReviewers,
        action: impl FnOnce(&OpenWorkspace, &PinnedWorkspaceRoot, &str) -> io::Result<T>,
    ) -> io::Result<T> {
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        let (configuration, proof) = self.read_configuration(metadata, &store)?;
        let workspace = OpenWorkspace::open_attachment_read_history(
            metadata,
            store.clone(),
            trusted,
            proof.as_ref(),
        )
        .map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let result = action(&workspace, &store, &configuration)?;
        let (after_configuration, after_proof) = self.read_configuration(metadata, &store)?;
        if after_configuration != configuration || after_proof != proof {
            return Err(invalid("history changed during inspection"));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_attachment::{AttachmentStorage, ObservationLimits, ProvisionedAttachment};
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::{fs, path::PathBuf};
    struct Fixture {
        root: PathBuf,
        attachment: ProvisionedAttachment,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("mesh-enrolled-read-{name}-{}", std::process::id()));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("source")).unwrap();
            fs::create_dir(root.join("metadata")).unwrap();
            fs::write(root.join("source/note"), b"saved original").unwrap();
            let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
            let attachment = storage.provision(&root.join("source")).unwrap();
            let input = attachment
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let key = SigningKey::from_bytes(&[67; 32]);
            attachment
                .project()
                .save_capture(
                    attachment.metadata_path(),
                    &input,
                    mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                    |payload| {
                        Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                            key.sign(payload.as_bytes()).to_bytes(),
                        ))
                    },
                )
                .unwrap();
            attachment.enroll_dependency_history().unwrap();
            Self { root, attachment }
        }
        fn journal(&self) -> PathBuf {
            self.attachment.metadata_path().join(RECORD_FILE_NAME)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn enrolled_history_reads_saved_bytes_and_cannot_promote_receipts() {
        let f = Fixture::new("immutable");
        let history = &f.attachment;
        let before = fs::read(f.journal()).unwrap();
        let versions = history.saved_versions().unwrap();
        let key = SigningKey::from_bytes(&[67; 32]);
        let actor = mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes());
        let input = history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        assert!(history
            .project()
            .save_capture(
                history.metadata_path(),
                &input,
                actor,
                |_| -> Result<mesh_types::Signature, &'static str> {
                    panic!("fenced capture must not sign")
                }
            )
            .is_err());
        assert!(history
            .request_review(&versions[0].operation().to_string(), actor)
            .is_err());
        fs::write(f.root.join("source/note"), b"editor continues").unwrap();
        assert_eq!(
            history
                .project()
                .saved_file(history.metadata_path(), versions[0], "note")
                .unwrap()
                .unwrap(),
            b"saved original"
        );
        history
            .project()
            .with_read_history(
                history.metadata_path(),
                history.store.clone(),
                &TrustedReviewers::default(),
                |workspace, _, _| {
                    assert!(workspace
                        .promote_approval_receipt(b"cannot write".to_vec())
                        .is_err());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(fs::read(f.journal()).unwrap(), before);
    }

    #[test]
    fn enrolled_history_refuses_torn_changed_or_replaced_journal_without_repair() {
        for mode in ["torn", "base", "replacement", "no-enrollment"] {
            let f = Fixture::new(mode);
            let mut bytes = fs::read(f.journal()).unwrap();
            match mode {
                "torn" => {
                    bytes.pop();
                }
                "base" => bytes[10] ^= 1,
                "no-enrollment" => bytes.truncate(bytes.len() - 145),
                "replacement" => {
                    fs::rename(f.journal(), f.root.join("old-journal")).unwrap();
                }
                _ => unreachable!(),
            }
            fs::write(f.journal(), &bytes).unwrap();
            assert!(f.attachment.saved_versions().is_err(), "{mode}");
            assert_eq!(fs::read(f.journal()).unwrap(), bytes);
        }
    }

    #[test]
    fn enrolled_history_refuses_missing_fences_and_corrupt_policy_objects() {
        for mode in ["attached", "managed", "intent", "payload"] {
            let f = Fixture::new(mode);
            let before = fs::read(f.journal()).unwrap();
            let scan = scan_journal(&before).unwrap();
            let Some(StoredRecord::Dependency(record)) = scan.records().last() else {
                panic!("enrollment")
            };
            let path = match mode {
                "attached" => f.attachment.metadata_path().join(HISTORY),
                "managed" => {
                    let names = fs::read_dir(f.attachment.metadata_path())
                        .unwrap()
                        .map(|e| e.unwrap().path())
                        .collect::<Vec<_>>();
                    names
                        .into_iter()
                        .find(|p| {
                            fs::read_to_string(p)
                                .is_ok_and(|s| s.contains("mesh.workspace-agent-custody/v2"))
                        })
                        .unwrap()
                }
                "intent" | "payload" => {
                    let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
                        f.attachment.metadata_path(),
                        f.attachment.store.filesystem().read_only(),
                    )
                    .unwrap();
                    let digest = if mode == "intent" {
                        record.authority
                    } else {
                        record.payload
                    };
                    f.attachment.metadata_path().join(
                        cas.layout()
                            .chunk_path(&mesh_cas::Digest32::from_bytes(*digest.as_bytes())),
                    )
                }
                _ => unreachable!(),
            };
            fs::write(&path, b"corrupt evidence").unwrap();
            assert!(f.attachment.saved_versions().is_err(), "{mode}");
            assert_eq!(fs::read(&path).unwrap(), b"corrupt evidence");
            assert_eq!(fs::read(f.journal()).unwrap(), before);
        }
    }

    #[test]
    fn sealed_read_proof_rejects_journal_change_even_without_dependency_records() {
        let f = Fixture::new("proof");
        let h = &f.attachment;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&h.store).unwrap();
        let (_, proof) = h
            .project()
            .read_configuration(h.metadata_path(), &h.store)
            .unwrap();
        let bytes = fs::read(f.journal()).unwrap();
        fs::write(f.journal(), &bytes[..bytes.len() - 145]).unwrap();
        assert!(OpenWorkspace::open_attachment_read_history(
            h.metadata_path(),
            h.store.clone(),
            &TrustedReviewers::default(),
            proof.as_ref()
        )
        .is_err());
    }
    #[test]
    fn enrolled_inspection_does_not_authorize_manual_agent_or_remote_consumption() {
        let f = Fixture::new("consumption-boundary");
        let h = &f.attachment;
        let version = h.saved_versions().unwrap()[0].operation().to_string();
        assert!(h.inspect_text(&version, "note").is_ok());
        let storage = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
        let destination = f.root.join("destination");
        fs::create_dir(&destination).unwrap();
        let pinned = PinnedWorkspaceRoot::open(destination.clone()).unwrap();
        let journal = fs::read(f.journal()).unwrap();
        let refused = [
            storage
                .open_version_lane(
                    h,
                    &version,
                    "0123456789abcdef0123456789abcdef",
                    ObservationLimits::default(),
                )
                .is_err(),
            h.validate_lane_version(&version).is_err(),
            h.materialize_saved_version(&version, &pinned, &destination)
                .is_err(),
            h.prepare_remote_input(&version).is_err(),
        ];
        assert_eq!(refused, [true; 4], "manual allocation, agent validation/materialization and remote export require native grants");
        assert!(!f.root.join("metadata/work-lanes").exists());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(fs::read(f.journal()).unwrap(), journal);
        assert_eq!(
            fs::read(f.root.join("source/note")).unwrap(),
            b"saved original"
        );
    }
}
