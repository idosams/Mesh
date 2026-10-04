use super::*;
use crate::project_attachment::dependency_owner_context::OwnerHistoryContext;
use ed25519_dalek::{Signer as _, SigningKey};
use std::io::Write as _;
impl AttachmentStorage {
    pub(in crate::project_attachment) fn assert_chained_consumed_start(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        destination: &ProvisionedAttachment,
        original: &ProvisionedAttachment,
        grant: RecordDigest,
    ) {
        let available = [original];
        let request_id = RecordDigest::from_bytes([160; 32]);
        let request = |grant| NativeConsumedStartRequest {
            input: NativeGrantInspection {
                source,
                version,
                destination,
                grant,
            },
            available: &available,
            request: request_id,
            limits: ObservationLimits::default(),
        };
        let key = SigningKey::from_bytes(&[161; 32]);
        let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
        let source_journal =
            fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let source_editor = fs::read(source.project().root().join("kept")).unwrap();
        assert_ne!(source_editor, b"sync recovery progress");
        let candidate = self
            .prepare_consumed_start(owner, request(grant), actor, |payload| {
                let _guard = crate::workspace_custody::lock_workspace_initialization_set(&[
                    owner.store.clone(),
                    source.store.clone(),
                    destination.store.clone(),
                ])
                .unwrap();
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            })
            .unwrap();
        let before_destination =
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let revoked = self
            .grant_saved_input_with_inputs(
                owner,
                crate::project_attachment::NativeInputGrantRequest {
                    source,
                    version,
                    destination,
                    allowed: false,
                    expected_previous: Some(grant),
                    request: RecordDigest::from_bytes([164; 32]),
                },
                &available,
            )
            .unwrap();
        assert!(
            candidate.stage(self).is_err(),
            "signed child start reused a revoked grant"
        );
        assert!(fs::read_dir(destination.project().root())
            .unwrap()
            .next()
            .is_none());
        assert_eq!(
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            before_destination
        );
        let grant = self
            .grant_saved_input_with_inputs(
                owner,
                crate::project_attachment::NativeInputGrantRequest {
                    source,
                    version,
                    destination,
                    allowed: true,
                    expected_previous: Some(revoked.record()),
                    request: RecordDigest::from_bytes([165; 32]),
                },
                &available,
            )
            .unwrap()
            .record();
        let candidate = self
            .prepare_consumed_start(owner, request(grant), actor, |payload| {
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            })
            .unwrap();
        let operation = candidate.operation();
        let staged = candidate.stage(self).unwrap();
        let failure = candidate
            .commit_phase_with_io(
                self,
                &staged,
                true,
                CommitPhase::Complete,
                |step, file, frame| {
                    if step == "owner-staged" {
                        file.write_all(&frame[..1])?;
                        file.sync_all()?;
                        return Err(io::Error::other("partial chained owner receipt"));
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap_err();
        assert_eq!(failure.to_string(), "partial chained owner receipt");
        assert_eq!(
            fs::read(destination.project().root().join("kept")).unwrap(),
            b"sync recovery progress"
        );
        assert!(destination.saved_versions().is_err());
        let physical = destination.store.identity().unwrap();
        let installed = fs::metadata(destination.project().root().join("kept"))
            .unwrap()
            .ino();
        let restart = |mode, expected| {
            let value = Json::object([
                ("storage", Json::text(self.path.to_string_lossy())),
                ("owner", Json::text(owner.id())),
                ("source", Json::text(source.id())),
                ("destination", Json::text(destination.id())),
                ("original", Json::text(original.id())),
                ("version", Json::text(version.operation().to_hex())),
                ("grant", Json::text(grant.to_hex())),
                ("request", Json::text(request_id.to_hex())),
                ("operation", Json::text(operation.to_hex())),
                ("mode", Json::text(mode)),
            ]);
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
                .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
            assert_eq!(
                child.status.code(),
                Some(expected),
                "{} {}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
        };
        restart("chain-complete-partial", 75);
        assert!(self.saved_consumed_versions(owner, request(grant)).is_err());
        assert!(self
            .saved_dependency_versions(owner.id(), destination.id(), &[source.id(), original.id()])
            .is_err());
        restart("chain-complete-lost", 76);
        let owner_journal = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let destination_journal =
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        restart("chain-complete", 0);
        restart("chain-complete", 0);
        assert_eq!(
            fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            owner_journal
        );
        assert_eq!(
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            destination_journal
        );
        assert_eq!(destination.store.identity().unwrap(), physical);
        assert_eq!(
            fs::metadata(destination.project().root().join("kept"))
                .unwrap()
                .ino(),
            installed
        );
        let saved = self.saved_consumed_versions(owner, request(grant)).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].operation(), operation);
        let catalog_inputs = [source.id(), original.id()];
        self.assert_catalog_read_freshness(owner.id(), destination.id(), &catalog_inputs);
        assert_eq!(
            self.saved_dependency_versions(owner.id(), destination.id(), &catalog_inputs)
                .unwrap(),
            saved
        );
        assert_eq!(
            self.saved_dependency_versions(owner.id(), owner.id(), &[])
                .unwrap(),
            owner.saved_versions().unwrap()
        );
        assert!(self
            .saved_dependency_versions(owner.id(), destination.id(), &[])
            .is_err());
        assert!(self
            .saved_dependency_versions(destination.id(), destination.id(), &catalog_inputs)
            .is_err());
        assert_eq!(
            self.saved_dependency_file(
                owner.id(),
                destination.id(),
                &catalog_inputs,
                saved[0],
                "kept"
            )
            .unwrap(),
            Some(b"sync recovery progress".to_vec())
        );
        assert_eq!(
            self.consumed_saved_file(owner, request(grant), saved[0], "kept")
                .unwrap(),
            Some(b"sync recovery progress".to_vec())
        );
        let input_graph = self
            .inspect_dependency_graph(owner, source, version, &[original])
            .unwrap();
        let graph = self
            .inspect_dependency_graph(owner, destination, saved[0], &[source, original])
            .unwrap();
        assert_eq!(graph.operation_count(), input_graph.operation_count() + 1);
        assert_eq!(
            graph,
            self.inspect_consumed_dependency_graph(owner, request(grant), saved[0])
                .unwrap()
        );
        let retained = graph.retained_content_json();
        let Some(Json::Array(stores)) = retained.get("stores") else {
            panic!("missing retained stores");
        };
        assert_eq!(stores.len(), 3);
        assert!(
            self.inspect_dependency_graph(owner, destination, saved[0], &[original])
                .is_err(),
            "omitted intermediate lane cannot be treated as independent"
        );
        fs::write(
            destination.project().root().join("kept"),
            b"new child progress",
        )
        .unwrap();
        let capture = destination
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let next = self
            .prepare_consumed_capture(
                owner,
                request(grant),
                &capture,
                actor,
                RecordDigest::from_bytes([162; 32]),
                |payload| {
                    Ok::<_, &'static str>(Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap()
            .commit()
            .unwrap();
        let catalog_saved = self
            .saved_dependency_versions(owner.id(), destination.id(), &catalog_inputs)
            .unwrap();
        assert_eq!(catalog_saved.len(), 2);
        assert_eq!(catalog_saved.last(), Some(&next));
        assert_eq!(
            self.saved_dependency_file(
                owner.id(),
                destination.id(),
                &catalog_inputs,
                saved[0],
                "kept"
            )
            .unwrap(),
            Some(b"sync recovery progress".to_vec())
        );
        assert_eq!(
            self.saved_dependency_file(owner.id(), destination.id(), &catalog_inputs, next, "kept")
                .unwrap(),
            Some(b"new child progress".to_vec())
        );
        let graph = self
            .inspect_dependency_graph(owner, destination, next, &[source, original])
            .unwrap();
        assert_eq!(graph.operation_count(), input_graph.operation_count() + 2);
        assert_eq!(
            self.consumed_saved_file(owner, request(grant), saved[0], "kept")
                .unwrap(),
            Some(b"sync recovery progress".to_vec())
        );
        assert_eq!(
            self.consumed_saved_file(owner, request(grant), next, "kept")
                .unwrap(),
            Some(b"new child progress".to_vec())
        );
        assert_eq!(
            fs::read(source.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            source_journal
        );
        assert_eq!(
            fs::read(source.project().root().join("kept")).unwrap(),
            source_editor
        );
        assert_eq!(
            fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            owner_journal
        );
    }
    pub(in crate::project_attachment) fn chained_start_restart_test(
        &self,
        owner: &ProvisionedAttachment,
        source: &ProvisionedAttachment,
        destination: &ProvisionedAttachment,
        value: &Json,
    ) {
        let text = |name| value.get(name).and_then(Json::as_text).unwrap();
        let parse = |name| RecordDigest::parse_hex(text(name)).unwrap();
        let original = self.reopen(text("original")).unwrap();
        let available = [&original];
        let version = {
            let graph = self
                .prepare_dependency_graph(owner, source, parse("version"), &available)
                .unwrap();
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&graph.roots).unwrap();
            let context = OwnerHistoryContext::recovering(owner, parse("request")).unwrap();
            let context = self
                .resolve_consumed_histories(owner, &[source, &original], &guard, context)
                .unwrap();
            let (_, _, history) = context.history(source).unwrap();
            SavedAttachmentVersion::from_verified_history(&history, parse("version")).unwrap()
        };
        let request = || NativeConsumedStartRequest {
            input: NativeGrantInspection {
                source,
                version,
                destination,
                grant: parse("grant"),
            },
            available: &available,
            request: parse("request"),
            limits: ObservationLimits::default(),
        };
        let (candidate, staged) = self
            .recover_installing_consumed_start(owner, request())
            .unwrap();
        assert_eq!(candidate.operation(), parse("operation"));
        candidate
            .commit_phase_with_io(
                self,
                &staged,
                true,
                CommitPhase::Complete,
                |step, file, frame| {
                    if text("mode") == "chain-complete-partial" && step == "complete-staged" {
                        file.write_all(&frame[..1])?;
                        file.sync_all()?;
                        std::process::exit(75);
                    }
                    if text("mode") == "chain-complete-lost" && step == "complete-synced" {
                        std::process::exit(76);
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap();
        let saved = self.saved_consumed_versions(owner, request()).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].operation(), parse("operation"));
        assert_eq!(
            self.consumed_saved_file(owner, request(), saved[0], "kept")
                .unwrap(),
            Some(b"sync recovery progress".to_vec())
        );
    }
}
