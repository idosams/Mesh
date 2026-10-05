//! Exact signed starting-state reconstruction from a custody-bound saved input.
//! This verifies content, not current grants, completed consumption, or publication authority.
use super::*;
use crate::project_attachment::NativeGrantedInput;
use crate::{
    authenticated_changeset::AuthenticatedChangeSet,
    workspace_custody::WorkspaceInitializationGuard,
};

pub(in crate::project_attachment) struct StartingSource<'a> {
    pub configuration: &'a str,
    pub prospective: &'a str,
    pub actor: PublicKey,
    pub limits: ObservationLimits,
    pub envelope: &'a AuthenticatedChangeSet,
}

pub(in crate::project_attachment) fn verify_starting_source(
    destination: &ProvisionedAttachment,
    input: &NativeGrantedInput<'_>,
    guard: &WorkspaceInitializationGuard,
    expected: StartingSource<'_>,
) -> io::Result<String> {
    guard
        .require_roots(&[
            destination.store.clone(),
            destination.project().pinned.clone(),
        ])
        .map_err(error)?;
    expected.limits.validate()?;
    if !expected.envelope.signed_by(expected.actor) {
        return Err(invalid("starting actor changed"));
    }
    let rules = input.starting_exclusion_rules()?;
    let policy = crate::project_attachment::observation::policy_digest(&rules);
    let mut proposed = Json::parse(expected.configuration).map_err(error)?;
    let Json::Object(fields) = &mut proposed else {
        return Err(invalid("invalid destination configuration"));
    };
    fields
        .iter_mut()
        .find(|(key, _)| key == "exclusions")
        .ok_or_else(|| invalid("missing exclusions"))?
        .1 = Json::text(policy.to_string());
    let (prospective, _) = destination.project().history_configuration_with_previous(
        &destination.store,
        Some(policy),
        Some(proposed.encode()),
    )?;
    let snapshot = prepare_initial_snapshot(
        input,
        WorkspaceId::from_bytes(short_id(prospective.as_bytes())),
        ActorId::from_bytes(*expected.actor.as_bytes()),
        expected.limits,
    )?;
    // Discard the temporary content copy before loading retained staged objects.
    let statement = operation_checkpoint_signing_body(
        &AuthenticatedOperationCheckpointRequest::new(
            WorkspaceId::from_bytes(short_id(prospective.as_bytes())),
            ActorId::from_bytes(*expected.actor.as_bytes()),
            SessionId::from_bytes(short_id(expected.actor.as_bytes())),
            ActorSequence::FIRST,
            CausalParents::genesis(),
            HeadId::from_bytes([0; 32]),
            PolicyEpoch::new(1),
            Hlc::new(0, 0),
            snapshot.operations,
            expected.actor,
            Signature::from_bytes([0; 64]),
        ),
        &StartHead,
    );
    if expected.envelope.changeset() != statement || expected.prospective != prospective {
        return Err(invalid(
            "retained signed start differs from exact saved source",
        ));
    }
    guard.ensure_current().map_err(error)?;
    Ok(prospective)
}
