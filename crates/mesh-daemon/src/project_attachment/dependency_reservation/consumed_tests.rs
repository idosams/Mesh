use super::*;
use crate::project_attachment::NativeConsumedStartRequest;
impl AttachmentStorage {
    pub(in crate::project_attachment) fn assert_consumed_child_reservation(
        &self,
        owner: &ProvisionedAttachment,
        start: &NativeConsumedStartRequest<'_>,
        version: SavedAttachmentVersion,
    ) {
        let parent = start.input.destination;
        let request = RecordDigest::from_bytes([140; 32]);
        let count = self.registrations().unwrap().len();
        let journal_path = parent.metadata_path().join(crate::RECORD_FILE_NAME);
        let marker_path = parent.metadata_path().join(super::super::history::HISTORY);
        let journal = fs::read(&journal_path).unwrap();
        let marker = fs::read(&marker_path).unwrap();
        let editor = fs::read(parent.project().root().join("kept")).unwrap();
        let error = self
            .reserve_with_inputs_and_hook(
                owner,
                parent,
                version,
                request,
                &[start.input.source],
                |step| {
                    if step == "initialized" {
                        Err(io::Error::other("interrupt consumed child reservation"))
                    } else {
                        Ok(())
                    }
                },
            )
            .err()
            .expect("reservation must interrupt");
        assert_eq!(error.to_string(), "interrupt consumed child reservation");
        let value = Json::object([
            ("storage", Json::text(self.path.to_string_lossy())),
            ("owner", Json::text(owner.id())),
            ("source", Json::text(start.input.source.id())),
            ("destination", Json::text(parent.id())),
            (
                "version",
                Json::text(start.input.version.operation().to_hex()),
            ),
            ("grant", Json::text(start.input.grant.to_hex())),
            ("request", Json::text(start.request.to_hex())),
            ("child_version", Json::text(version.operation().to_hex())),
            ("reservation", Json::text(request.to_hex())),
            ("count", Json::Number((count + 1) as u64)),
            ("mode", Json::text("reserve-consumed")),
        ]);
        for _ in 0..2 {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
                .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
            assert!(
                child.status.success(),
                "{} {}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
        }
        let child = self
            .reserve_dependency_lane_with_inputs(
                owner,
                parent,
                version,
                request,
                &[start.input.source],
            )
            .unwrap();
        let again = self
            .reserve_dependency_lane(owner, parent, version, request)
            .unwrap();
        assert_eq!(child.id(), again.id());
        assert_eq!(
            child.store.identity().unwrap(),
            again.store.identity().unwrap()
        );
        assert_eq!(self.registrations().unwrap().len(), count + 1);
        assert!(child.saved_versions().unwrap().is_empty());
        assert!(fs::read_dir(child.project().root())
            .unwrap()
            .next()
            .is_none());
        assert_eq!(fs::read(&journal_path).unwrap(), journal);
        assert_eq!(fs::read(&marker_path).unwrap(), marker);
        assert_eq!(
            fs::read(parent.project().root().join("kept")).unwrap(),
            editor
        );
        // Retry selection must not be changed to an operation from a different native work.
        assert!(self
            .reserve_dependency_lane_with_inputs(
                owner,
                parent,
                start.input.version,
                request,
                &[start.input.source]
            )
            .is_err());
        assert_eq!(self.registrations().unwrap().len(), count + 1);
    }
}
