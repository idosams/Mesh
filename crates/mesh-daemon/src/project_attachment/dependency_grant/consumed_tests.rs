use super::*;
use crate::project_attachment::{
    dependency_decision::PENDING, dependency_enrollment::read_private_in_store,
    dependency_owner_context::OwnerHistoryContext, NativeGrantInspection,
};
use std::{fs, io::Write as _};
fn id(n: u8) -> RecordDigest {
    RecordDigest::from_bytes([n; 32])
}
impl AttachmentStorage {
    pub(in crate::project_attachment) fn assert_consumed_child_grant(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        destination: &ProvisionedAttachment,
        original: &ProvisionedAttachment,
    ) {
        let request = |allowed, previous, number| NativeInputGrantRequest {
            source,
            version,
            destination,
            allowed,
            expected_previous: previous,
            request: id(number),
        };
        let before_source = fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let before_destination =
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let owner_path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
        let before_owner = fs::read(&owner_path).unwrap();
        let old_proof = {
            let _guard = crate::workspace_custody::lock_workspace_initialization_set(
                std::slice::from_ref(&owner.store),
            )
            .unwrap();
            owner
                .project()
                .read_configuration(owner.metadata_path(), &owner.store)
                .unwrap()
                .1
                .unwrap()
        };
        let failure = self
            .grant_with_inputs_and_io(
                owner,
                request(true, None, 150),
                &[original],
                |step, file, frame| {
                    if matches!(step, Step::Staged) {
                        file.write_all(&frame[..1])?;
                        file.sync_all()?;
                        return Err(io::Error::other("partial consumed-source grant"));
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap_err();
        assert_eq!(failure.to_string(), "partial consumed-source grant");
        assert_eq!(fs::read(&owner_path).unwrap().len(), before_owner.len() + 1);
        {
            let _guard = crate::workspace_custody::lock_workspace_initialization_set(
                std::slice::from_ref(&owner.store),
            )
            .unwrap();
            assert!(
                OwnerHistoryContext::for_control(owner, &old_proof).is_err(),
                "stale owner proof reused during partial grant recovery"
            );
        }
        assert!(self
            .inspect_dependency_graph(owner, source, version, &[original])
            .is_err());
        let restart = |allowed, previous: Option<RecordDigest>, number, generation| {
            let value = Json::object([
                ("storage", Json::text(self.path.to_string_lossy())),
                ("owner", Json::text(owner.id())),
                ("source", Json::text(original.id())),
                ("destination", Json::text(source.id())),
                ("child", Json::text(destination.id())),
                ("child_version", Json::text(version.operation().to_hex())),
                ("allowed", Json::Bool(allowed)),
                ("previous", Json::text(previous.unwrap_or(ZERO).to_hex())),
                ("request", Json::text(id(number).to_hex())),
                ("generation", Json::Number(generation)),
                ("mode", Json::text("grant-consumed")),
            ]);
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
                .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
            assert!(
                child.status.success(),
                "{} {}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
        };
        restart(true, None, 150, 1);
        let grant = self
            .grant_saved_input_with_inputs(owner, request(true, None, 150), &[original])
            .unwrap();
        assert_eq!(grant.generation(), 1);
        let journal = fs::read(&owner_path).unwrap();
        restart(true, None, 150, 1);
        assert_eq!(fs::read(&owner_path).unwrap(), journal);
        let read = |grant| -> io::Result<Vec<u8>> {
            let works = [source, destination, original];
            let graph =
                self.prepare_dependency_graph(owner, source, version.operation(), &works)?;
            let guard = crate::workspace_custody::lock_workspace_initialization_set(&graph.roots)
                .map_err(error)?;
            let context = self.resolve_consumed_histories(
                owner,
                &works,
                &guard,
                OwnerHistoryContext::current(owner),
            )?;
            let input = self.prepare_input_grant(
                owner,
                NativeGrantInspection {
                    source,
                    version,
                    destination,
                    grant,
                },
            )?;
            self.with_input_grant_owner(&input, &guard, &context, |input| {
                let mut bytes = Vec::new();
                input.write_file("kept", &mut bytes)?;
                Ok(bytes)
            })
        };
        assert_eq!(read(grant.record()).unwrap(), b"sync recovery progress");
        let failure = self
            .grant_with_inputs_and_io(
                owner,
                request(false, Some(grant.record()), 151),
                &[original],
                |step, _, _| {
                    if matches!(step, Step::Appended) {
                        Err(io::Error::other("lost consumed revoke reply"))
                    } else {
                        Ok(())
                    }
                },
                |file| file.sync_all(),
            )
            .unwrap_err();
        assert_eq!(failure.to_string(), "lost consumed revoke reply");
        restart(false, Some(grant.record()), 151, 2);
        let revoked = self
            .grant_saved_input_with_inputs(
                owner,
                request(false, Some(grant.record()), 151),
                &[original],
            )
            .unwrap();
        assert!(read(grant.record()).is_err());
        let next = self
            .grant_saved_input_with_inputs(
                owner,
                request(true, Some(revoked.record()), 152),
                &[original],
            )
            .unwrap();
        assert_eq!(next.generation(), 3);
        assert_eq!(read(next.record()).unwrap(), b"sync recovery progress");
        assert!(read(grant.record()).is_err());
        assert_eq!(
            fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            before_source
        );
        assert_eq!(
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            before_destination
        );
        assert!(destination.saved_versions().unwrap().is_empty());
        assert!(fs::read_dir(destination.project().root())
            .unwrap()
            .next()
            .is_none());
    }
    pub(in crate::project_attachment) fn consumed_grant_restart_test(
        &self,
        owner: &ProvisionedAttachment,
        original: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        value: &Json,
    ) {
        let text = |name| value.get(name).and_then(Json::as_text).unwrap();
        let parse = |name| RecordDigest::parse_hex(text(name)).unwrap();
        let destination = self.reopen(text("child")).unwrap();
        let works = [source, &destination, original];
        let operation = parse("child_version");
        let version = {
            let graph = self
                .prepare_dependency_graph(owner, source, operation, &works)
                .unwrap();
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&graph.roots).unwrap();
            let raw = match read_private_in_store(&owner.store, PENDING) {
                Ok(raw) => Some(raw),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => panic!("{e}"),
            };
            let (_, proof) = owner
                .project()
                .read_decision_configuration(owner.metadata_path(), &owner.store, raw.as_deref())
                .unwrap();
            let context = OwnerHistoryContext::for_control(owner, &proof.unwrap()).unwrap();
            let context = self
                .resolve_consumed_histories(owner, &works, &guard, context)
                .unwrap();
            let (_, _, history) = context.history(source).unwrap();
            SavedAttachmentVersion::from_verified_history(&history, operation).unwrap()
        };
        let previous = parse("previous");
        let grant = self
            .grant_saved_input_with_inputs(
                owner,
                NativeInputGrantRequest {
                    source,
                    version,
                    destination: &destination,
                    allowed: value.get("allowed") == Some(&Json::Bool(true)),
                    expected_previous: (previous != ZERO).then_some(previous),
                    request: parse("request"),
                },
                &[original],
            )
            .unwrap();
        assert_eq!(
            Some(grant.generation()),
            value.get("generation").and_then(Json::as_u64)
        );
    }
}
