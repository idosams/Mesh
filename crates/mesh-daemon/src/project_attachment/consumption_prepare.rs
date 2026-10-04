//! Signed consumption candidates. Preparation writes nothing and does not reserve a commit order.
use super::{
    consumption_start::{prepare_initial_snapshot, InitialSnapshot},
    dependency_enrollment::read_private_in_store,
    dependency_transaction::hash,
    history::{short_id, verify_history_binding},
    invalid, AttachmentStorage, NativeDependencyWorkBinding, NativeGrantInspection,
    ObservationLimits, ProvisionedAttachment, SavedAttachmentVersion,
};
use crate::{
    checkpoint_storage::{
        operation_checkpoint_signing_body, prepare_authenticated_checkpoint,
        AuthenticatedOperationCheckpointRequest, PreparedAuthenticatedCheckpoint,
    },
    ipc::Json,
    workspace::OpenWorkspace,
};
use mesh_crypto::SigningPayload;
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, PolicyEpoch, SessionId,
    TransitionCommitment, WorkspaceId,
};
use mesh_store::RecordDigest;
use mesh_types::{PublicKey, Signature};
use std::{collections::BTreeMap, io, path::Path};

/// Exact trusted-native selection. Preparation does not grant access, copy content or start agents.
pub struct NativeConsumedStartRequest<'a> {
    /// Exact grant/source/destination selection.
    pub input: NativeGrantInspection<'a>,
    /// Native handles for every transitive input, never an asserted completeness list.
    pub available: &'a [&'a ProvisionedAttachment],
    /// Stable nonzero transaction request identity.
    pub request: RecordDigest,
    /// Bounded snapshot admission.
    pub limits: ObservationLimits,
}
#[derive(PartialEq, Eq)]
struct Basis {
    configuration: String,
    prospective: String,
    graph: RecordDigest,
    destination: NativeDependencyWorkBinding,
    enrollment: crate::dependency_policy::NativeDependencyBinding,
}
/// An immutable authenticated candidate, not a saved version or continuing permission.
/// A future consumption commit must retain custody through revalidation and durable completion.
pub struct PreparedNativeConsumedStart {
    owner: ProvisionedAttachment,
    source: ProvisionedAttachment,
    destination: ProvisionedAttachment,
    version: SavedAttachmentVersion,
    grant: RecordDigest,
    available: Vec<ProvisionedAttachment>,
    request: RecordDigest,
    limits: ObservationLimits,
    actor: PublicKey,
    basis: Basis,
    checkpoint: PreparedAuthenticatedCheckpoint,
}
impl PreparedNativeConsumedStart {
    /// Candidate operation identity. It has not been appended or acknowledged as saved work.
    pub fn operation(&self) -> RecordDigest {
        self.checkpoint.changeset_id
    }
    /// Stable transaction request, independent of the signing callback.
    pub fn request(&self) -> RecordDigest {
        self.request
    }
    /// Independently computed historical source closure identity, not publication permission.
    pub fn dependency_digest(&self) -> RecordDigest {
        self.basis.graph
    }
    /// Point-in-time revalidation only. This method writes nothing and grants no later mutation.
    pub fn revalidate(&self, storage: &AttachmentStorage) -> io::Result<()> {
        let available = self.available.iter().collect::<Vec<_>>();
        let request = NativeConsumedStartRequest {
            input: NativeGrantInspection {
                source: &self.source,
                version: self.version,
                destination: &self.destination,
                grant: self.grant,
            },
            available: &available,
            request: self.request,
            limits: self.limits,
        };
        let (basis, _, _) =
            storage.inspect_consumed_start_basis(&self.owner, &request, self.actor)?;
        if basis != self.basis {
            return Err(invalid("consumption preparation basis changed"));
        }
        Ok(())
    }
}
struct StartHead;
impl HeadDerivation for StartHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*hash(&commitment.canonical_bytes()).as_bytes())
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn absent(work: &ProvisionedAttachment, name: &str) -> io::Result<()> {
    match read_private_in_store(&work.store, name) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Err(invalid("destination has unfinished native work")),
    }
}
impl AttachmentStorage {
    /// Prepare one authenticated initial tree from exact granted saved bytes. Native custody is
    /// released before invoking the signer. No destination files, history or policy are changed.
    /// This development API has no commit method and cannot make the destination runnable.
    pub fn prepare_consumed_start<F, E>(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
        actor: PublicKey,
        sign: F,
    ) -> io::Result<PreparedNativeConsumedStart>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let (basis, snapshot, history) =
            self.inspect_consumed_start_basis(owner, &request, actor)?;
        let make_request = |signature| {
            AuthenticatedOperationCheckpointRequest::new(
                WorkspaceId::from_bytes(short_id(basis.prospective.as_bytes())),
                ActorId::from_bytes(*actor.as_bytes()),
                SessionId::from_bytes(short_id(actor.as_bytes())),
                ActorSequence::FIRST,
                CausalParents::genesis(),
                HeadId::from_bytes([0; 32]),
                PolicyEpoch::new(1),
                Hlc::new(0, 0),
                snapshot.operations.clone(),
                actor,
                signature,
            )
        };
        let body = operation_checkpoint_signing_body(
            &make_request(Signature::from_bytes([0; 64])),
            &StartHead,
        );
        let signature = sign(&SigningPayload::new(
            crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN,
            &body,
        ))
        .map_err(error)?;
        let checkpoint = prepare_authenticated_checkpoint(
            &history,
            make_request(signature),
            snapshot.files,
            &StartHead,
        )
        .map_err(error)?;
        Ok(PreparedNativeConsumedStart {
            owner: owner.clone(),
            source: request.input.source.clone(),
            destination: request.input.destination.clone(),
            version: request.input.version,
            grant: request.input.grant,
            available: request
                .available
                .iter()
                .map(|work| (*work).clone())
                .collect(),
            request: request.request,
            limits: request.limits,
            actor,
            basis,
            checkpoint,
        })
    }
    fn inspect_consumed_start_basis(
        &self,
        owner: &ProvisionedAttachment,
        request: &NativeConsumedStartRequest<'_>,
        actor: PublicKey,
    ) -> io::Result<(Basis, InitialSnapshot, OpenWorkspace)> {
        if request.request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing consumption request"));
        }
        request.limits.validate()?;
        let graph = self.prepare_dependency_graph(
            owner,
            request.input.source,
            request.input.version.operation(),
            request.available,
        )?;
        let grant = self.prepare_input_grant(
            owner,
            NativeGrantInspection {
                source: request.input.source,
                version: request.input.version,
                destination: request.input.destination,
                grant: request.input.grant,
            },
        )?;
        let roots = graph
            .roots
            .iter()
            .chain(&grant.roots)
            .map(|root| Ok((root.identity()?, root.clone())))
            .collect::<io::Result<BTreeMap<_, _>>>()?
            .into_values()
            .collect::<Vec<_>>();
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        let selected_graph = graph;
        let graph = self.inspect_prepared_dependency_graph(&selected_graph, &guard)?;
        // The current owner-consumption record format carries at most 256 exact inherited inputs.
        if graph.operation_count() > 256 {
            return Err(invalid("consumption closure exceeds policy format bound"));
        }
        let destination = request.input.destination;
        let origin = self
            .lane_origin(destination)?
            .ok_or_else(|| invalid("consumption needs a native reservation"))?;
        if !super::dependency_reservation::is_reservation(&origin) {
            return Err(invalid("consumption needs an empty native reservation"));
        }
        if !destination
            .attachment
            .pinned
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 1)?
            .is_empty()
        {
            return Err(invalid("consumption destination contains unexpected work"));
        }
        absent(destination, "dependency-capture.pending")?;
        absent(destination, super::dependency_decision::PENDING)?;
        let selected = self.prepare_dependency_work(owner, destination)?;
        let destination_binding = self.validate_dependency_work(&selected, &guard)?;
        let (configuration, proof) = destination
            .project()
            .read_configuration(destination.metadata_path(), &destination.store)?;
        let proof = proof.ok_or_else(|| invalid("destination enrollment missing"))?;
        let history = OpenWorkspace::open_attachment_read_history(
            destination.metadata_path(),
            destination.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        verify_history_binding(&history, &configuration)?;
        if history.operations() != 0 {
            return Err(invalid("consumption destination already has saved work"));
        }
        let (prospective, snapshot) = self.with_prepared_input_grant(&grant, &guard, |input| {
            let rules = input.starting_exclusion_rules()?;
            let policy = super::observation::policy_digest(&rules);
            let mut proposed = Json::parse(&configuration).map_err(error)?;
            let Json::Object(fields) = &mut proposed else {
                return Err(invalid("invalid destination configuration"));
            };
            let field = fields
                .iter_mut()
                .find(|(name, _)| name == "exclusions")
                .ok_or_else(|| invalid("missing destination exclusions"))?;
            field.1 = Json::text(policy.to_string());
            let (prospective, _) = destination.project().history_configuration_with_previous(
                &destination.store,
                Some(policy),
                Some(proposed.encode()),
            )?;
            let snapshot = prepare_initial_snapshot(
                &input,
                WorkspaceId::from_bytes(short_id(prospective.as_bytes())),
                ActorId::from_bytes(*actor.as_bytes()),
                request.limits,
            )?;
            Ok((prospective, snapshot))
        })?;
        let (current_configuration, current_proof) = destination
            .project()
            .read_configuration(destination.metadata_path(), &destination.store)?;
        if current_configuration != configuration
            || current_proof.as_ref() != Some(&proof)
            || !destination
                .attachment
                .pinned
                .filesystem()
                .read_directory_names_bounded(Path::new(""), 1)?
                .is_empty()
            || self.inspect_prepared_dependency_graph(&selected_graph, &guard)? != graph
        {
            return Err(invalid("consumption basis changed during preparation"));
        }
        guard.ensure_current().map_err(error)?;
        Ok((
            Basis {
                configuration,
                prospective,
                graph: graph.digest(),
                destination: destination_binding,
                enrollment: proof.binding(),
            },
            snapshot,
            history,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::fs;
    #[test]
    fn saved_ignore_rules_bind_candidate_without_changing_empty_reservation() {
        let root =
            std::env::temp_dir().join(format!("mesh-consumed-start-rules-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("metadata")).unwrap();
        fs::write(root.join("source/.gitignore"), b"ignored\n").unwrap();
        fs::write(root.join("source/ignored"), b"not admitted").unwrap();
        fs::write(root.join("source/kept"), b"saved bytes").unwrap();
        let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
        let owner = storage.provision(&root.join("source")).unwrap();
        let input = owner
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        let (_, created) = owner
            .project()
            .history_configuration(&owner.store, Some(input.exclusion_digest()))
            .unwrap();
        drop(
            OpenWorkspace::open_attachment_store(
                owner.metadata_path(),
                owner.store.clone(),
                created,
            )
            .unwrap(),
        );
        owner.enroll_dependency_history().unwrap();
        let key = SigningKey::from_bytes(&[129; 32]);
        let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
        let id = |n| RecordDigest::from_bytes([n; 32]);
        let sign = |payload: &SigningPayload| {
            Ok::<_, &'static str>(Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        };
        let version = owner
            .prepare_dependency_capture(&input, actor, id(1), sign)
            .unwrap()
            .commit()
            .unwrap();
        let destination = storage
            .reserve_dependency_lane(&owner, &owner, version, id(2))
            .unwrap();
        let grant = storage
            .grant_saved_input(
                &owner,
                super::super::NativeInputGrantRequest {
                    source: &owner,
                    version,
                    destination: &destination,
                    allowed: true,
                    expected_previous: None,
                    request: id(3),
                },
            )
            .unwrap();
        let request = || NativeConsumedStartRequest {
            input: NativeGrantInspection {
                source: &owner,
                version,
                destination: &destination,
                grant: grant.record(),
            },
            available: &[],
            request: id(4),
            limits: ObservationLimits::default(),
        };
        let marker = destination
            .metadata_path()
            .join(super::super::history::HISTORY);
        let before_marker = fs::read(&marker).unwrap();
        let owner_journal = fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let destination_journal =
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap();
        let prepared = storage
            .prepare_consumed_start(&owner, request(), actor, sign)
            .unwrap();
        assert_ne!(prepared.basis.configuration, prepared.basis.prospective);
        let proposed = Json::parse(&prepared.basis.prospective).unwrap();
        let expected =
            super::super::observation::policy_digest(&(Some("ignored\n".to_owned()), None))
                .to_string();
        assert_eq!(
            proposed.get("exclusions").and_then(Json::as_text),
            Some(expected.as_str())
        );
        let records = prepared.checkpoint.checkpoint.records();
        let record = records
            .iter()
            .find_map(|record| match record {
                mesh_store::StoredRecord::Operation(op) => Some(op),
                _ => None,
            })
            .unwrap();
        let payload = prepared
            .checkpoint
            .objects
            .iter()
            .find(|bytes| hash(bytes) == record.payload_digest)
            .unwrap();
        let fact = OpenWorkspace::verify_dependency_staged_operation(record, payload).unwrap();
        assert_eq!(
            fact.workspace,
            WorkspaceId::from_bytes(short_id(prepared.basis.prospective.as_bytes()))
        );
        assert_eq!(
            fact.manifests.len(),
            2,
            "saved rule file and admitted file only"
        );
        fs::write(root.join("source/.gitignore"), b"kept\n").unwrap();
        prepared.revalidate(&storage).unwrap();
        let repeated = storage
            .prepare_consumed_start(&owner, request(), actor, sign)
            .unwrap();
        assert_eq!(
            repeated.operation(),
            prepared.operation(),
            "unsaved rules cannot change the signed source snapshot"
        );
        assert!(storage
            .prepare_consumed_start(&owner, request(), actor, |_| Ok::<_, &'static str>(
                Signature::from_bytes([0; 64])
            ))
            .is_err());
        assert_eq!(fs::read(&marker).unwrap(), before_marker);
        assert_eq!(
            fs::read(owner.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            owner_journal
        );
        assert_eq!(
            fs::read(destination.metadata_path().join(crate::RECORD_FILE_NAME)).unwrap(),
            destination_journal
        );
        assert_eq!(
            fs::read_dir(destination.project().root()).unwrap().count(),
            0
        );
    }
}
