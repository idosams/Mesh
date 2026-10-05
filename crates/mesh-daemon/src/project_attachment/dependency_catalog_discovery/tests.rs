use super::*;
use crate::project_attachment::{
    NativeConsumedStartRequest, NativeGrantInspection, NativeInputGrantRequest, ObservationLimits,
    ProvisionedAttachment,
};
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_types::{PublicKey, Signature};
use std::{fs, path::PathBuf};
struct RestoreName {
    original: PathBuf,
    moved: PathBuf,
}
impl RestoreName {
    fn move_aside(original: &std::path::Path) -> Self {
        let moved = original.with_extension("discovery-offline");
        assert!(!moved.exists());
        fs::rename(original, &moved).unwrap();
        Self {
            original: original.to_owned(),
            moved,
        }
    }
}
impl Drop for RestoreName {
    fn drop(&mut self) {
        fs::rename(&self.moved, &self.original).expect("restore discovery fixture");
    }
}
impl AttachmentStorage {
    pub(in crate::project_attachment) fn assert_native_discovery(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        available: &[&ProvisionedAttachment],
    ) {
        let saved = self
            .discovered_dependency_versions(owner.id(), source.id())
            .unwrap();
        assert_eq!(self.candidate_owning_root(source.id()).unwrap(), owner.id());
        assert_eq!(
            self.registered_dependency_versions(source.id()).unwrap(),
            saved
        );
        assert_eq!(
            self.registered_dependency_versions(owner.id()).unwrap(),
            owner.saved_versions().unwrap()
        );
        assert_eq!(saved.len(), 2);
        let old_operation = saved[0].operation().to_string();
        let new_operation = version.operation().to_string();
        let old_preview = self
            .registered_dependency_text(source.id(), &old_operation, "kept")
            .unwrap();
        assert_eq!(
            old_preview.get("text").and_then(crate::ipc::Json::as_text),
            Some("sync recovery progress")
        );
        let new_preview = self
            .registered_dependency_text(source.id(), &new_operation, "kept")
            .unwrap();
        assert_eq!(
            new_preview.get("text").and_then(crate::ipc::Json::as_text),
            Some("new child progress")
        );
        let entries = self
            .registered_dependency_entries(source.id(), &old_operation, None)
            .unwrap();
        assert!(entries.encode().contains("kept"));
        assert!(self
            .registered_dependency_entries(source.id(), &old_operation, Some("missing-cursor"))
            .is_err());
        let comparison = self
            .registered_dependency_comparison(
                source.id(),
                &old_operation,
                &new_operation,
                (None, Some("kept")),
            )
            .unwrap();
        assert!(comparison.encode().contains("modified"));
        assert!(self
            .registered_dependency_comparison(
                source.id(),
                &old_operation,
                &new_operation,
                (None, Some("absent"))
            )
            .is_err());
        assert!(self
            .registered_dependency_text(source.id(), &"00".repeat(32), "kept")
            .is_err());
        assert!(self
            .registered_dependency_text(source.id(), &old_operation, "../kept")
            .is_err());
        let root_operation = owner
            .saved_versions()
            .unwrap()
            .last()
            .unwrap()
            .operation()
            .to_string();
        assert_eq!(
            self.registered_dependency_entries(owner.id(), &root_operation, None)
                .unwrap(),
            owner.inspect_entries(&root_operation, None).unwrap()
        );
        assert_eq!(
            self.registered_dependency_text(owner.id(), &root_operation, "kept")
                .unwrap(),
            owner.inspect_text(&root_operation, "kept").unwrap()
        );

        assert_eq!(saved.last(), Some(&version));
        assert_eq!(
            self.discovered_dependency_file(owner.id(), source.id(), saved[0], "kept")
                .unwrap(),
            Some(b"sync recovery progress".to_vec())
        );
        assert_eq!(
            self.discovered_dependency_file(owner.id(), source.id(), version, "kept")
                .unwrap(),
            Some(b"new child progress".to_vec())
        );
        assert!(self
            .discovered_dependency_versions(source.id(), source.id())
            .is_err());

        let id = |n| RecordDigest::from_bytes([n; 32]);
        let root_version = *owner.saved_versions().unwrap().last().unwrap();
        // Allocation parent is the owning root, but the consumed source is a different deep lane.
        let peer = self
            .reserve_dependency_lane(owner, owner, root_version, id(180))
            .unwrap();
        let grant = self
            .grant_saved_input_with_inputs(
                owner,
                NativeInputGrantRequest {
                    source,
                    version,
                    destination: &peer,
                    allowed: true,
                    expected_previous: None,
                    request: id(181),
                },
                available,
            )
            .unwrap();
        let request = || NativeConsumedStartRequest {
            input: NativeGrantInspection {
                source,
                version,
                destination: &peer,
                grant: grant.record(),
            },
            available,
            request: id(182),
            limits: ObservationLimits::default(),
        };
        let key = SigningKey::from_bytes(&[183; 32]);
        let candidate = self
            .prepare_consumed_start(
                owner,
                request(),
                PublicKey::from_bytes(key.verifying_key().to_bytes()),
                |payload| {
                    Ok::<_, &'static str>(Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap();
        let staged = candidate.stage(self).unwrap();
        candidate
            .complete_fenced_consumption(self, &staged)
            .unwrap();
        let expected = self.saved_consumed_versions(owner, request()).unwrap();
        assert_eq!(
            self.discovered_dependency_versions(owner.id(), peer.id())
                .unwrap(),
            expected
        );
        assert_eq!(
            self.discovered_dependency_file(owner.id(), peer.id(), expected[0], "kept")
                .unwrap(),
            Some(b"new child progress".to_vec())
        );
        let selection = self
            .discover_dependency_read(owner.id(), peer.id())
            .unwrap();
        assert!(
            selection.works.contains_key(source.id()),
            "consumed peer omitted because it was not an allocation parent"
        );
        assert_eq!(selection.works.len(), 4);
        assert_eq!(self.candidate_owning_root(peer.id()).unwrap(), owner.id());
        assert_eq!(
            self.registered_dependency_versions(peer.id()).unwrap(),
            expected
        );
        assert_eq!(
            self.registered_dependency_file(peer.id(), expected[0], "kept")
                .unwrap(),
            Some(b"new child progress".to_vec())
        );

        self.grant_saved_input_with_inputs(
            owner,
            NativeInputGrantRequest {
                source,
                version,
                destination: &peer,
                allowed: false,
                expected_previous: Some(grant.record()),
                request: id(184),
            },
            available,
        )
        .unwrap();
        let input_ids = selection
            .works
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let mut called = false;
        let stale = self.with_checked_catalog_dependency_history(
            owner.id(),
            peer.id(),
            &input_ids,
            |guard| self.verify_discovery(owner.id(), peer.id(), &selection, guard),
            |_, _, _| {
                called = true;
                Ok(())
            },
        );
        assert!(
            stale.is_err(),
            "stale discovery reused an earlier owner policy"
        );
        assert!(!called, "stale discovery reached the read callback");
        assert_eq!(
            self.discovered_dependency_versions(owner.id(), peer.id())
                .unwrap(),
            expected,
            "revoking future input access must not erase completed historical consumption"
        );

        let unrelated_path = owner
            .project()
            .root()
            .parent()
            .unwrap()
            .join("discovery-unrelated");
        fs::create_dir(&unrelated_path).unwrap();
        let unrelated = self.provision(&unrelated_path).unwrap();
        assert!(
            self.registered_dependency_versions(unrelated.id()).is_err(),
            "unenrolled work must not silently enter native dependency reads"
        );
        let offline = RestoreName::move_aside(&unrelated_path);
        let isolated = self
            .discover_dependency_read(owner.id(), peer.id())
            .unwrap();
        assert!(!isolated.works.contains_key(unrelated.id()));
        assert_eq!(
            self.discovered_dependency_versions(owner.id(), peer.id())
                .unwrap(),
            expected
        );
        drop(offline);

        let missing_owner = RestoreName::move_aside(owner.project().root());
        let owner_refused = self.registered_dependency_versions(peer.id());
        drop(missing_owner);
        assert!(
            owner_refused.is_err(),
            "missing owning root became a local-only read"
        );
        assert_eq!(
            self.registered_dependency_versions(peer.id()).unwrap(),
            expected
        );

        let missing = RestoreName::move_aside(source.project().root());
        let refused = self.discovered_dependency_versions(owner.id(), peer.id());
        let automatic_refused = self.registered_dependency_versions(peer.id());
        let review_refused = self.registered_dependency_text(
            peer.id(),
            &expected[0].operation().to_string(),
            "kept",
        );

        drop(missing);
        assert!(
            review_refused.is_err(),
            "missing required input became a partial preview"
        );

        assert!(
            automatic_refused.is_err(),
            "automatic owner selection omitted a required input"
        );

        assert!(
            refused.is_err(),
            "missing required cross-lane source became a partial success"
        );
        assert_eq!(
            self.discovered_dependency_versions(owner.id(), peer.id())
                .unwrap(),
            expected
        );
    }
}
