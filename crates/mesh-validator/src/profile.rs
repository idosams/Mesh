//! The one-time profile approval: detection proposes, a human approves, and only then does
//! anything run.
//!
//! # The shape of the guarantee
//!
//! [`ValidationProfile`] has **no public constructor**. The only value of that type that can exist
//! is one [`ProfileProposal::approve`] returned, and that method takes a
//! [`HumanPrincipal`](mesh_policy::HumanPrincipal) — which `mesh-policy` only issues for an actor
//! enrolled as [`ActorKind::Human`](mesh_types::ActorKind). There is no `Default`, no
//! `from_commands`, no builder. So a caller cannot fabricate a profile that admits a command,
//! and [`execute_plan`](crate::execute_plan) takes `Option<&ValidationProfile>` with `None`
//! meaning "nothing runs" rather than "run the defaults".
//!
//! # What the guarantee is NOT
//!
//! A `HumanPrincipal` is an **attestation**, not a signature. `mesh-policy` says so and this crate
//! inherits it: what makes an enrolment true is key custody upstream, and a process that can name
//! any key can name one it calls human. Plan §2.10 forbids stating that as safety without
//! evidence, so it is stated as what it is — the same trust boundary every other authority
//! decision in this workspace sits on, reused rather than duplicated.
//!
//! # A profile does not survive a rotation
//!
//! A profile records the policy epoch it was approved in. Revocation rotates the epoch, so a
//! profile approved before a rotation is out of force afterwards and every step it would have
//! admitted is blocked until the user approves again. That is the fail-closed direction: a stale
//! approval stops commands rather than continuing to authorise them.
//!
//! # Widening needs a new approval, narrowing does not
//!
//! [`ValidationProfile::covers`] asks whether a plan is entirely admitted, and
//! [`ValidationProfile::unadmitted`] names exactly what a re-approval would add. A change that
//! selects fewer validators runs fewer commands under the same profile; a change that selects a
//! command the profile never saw does not run it.

use std::collections::BTreeSet;

use mesh_crypto::{ActorKey, WorkspaceScope};
use mesh_policy::HumanPrincipal;
use mesh_types::{Blake3, ContentDigest, Digest32, DigestWriter, DomainTag, PolicyEpoch};

use crate::command::{CommandDigest, ValidationCommand};
use crate::selection::ValidationPlan;

/// The domain a proposal's digest is derived in.
const PROPOSAL_DOMAIN: DomainTag = DomainTag::new("mesh.v0.validator.profile-proposal");

/// What detection would like to be allowed to run, shown to the user before anything runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileProposal {
    commands: Vec<ValidationCommand>,
}

impl ProfileProposal {
    /// A proposal covering nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The commands a plan would like to run, de-duplicated and in command order.
    ///
    /// Two validators that would run the same command produce one entry: the user approves a
    /// command, not a validator, so the same command approved twice is the same approval.
    #[must_use]
    pub fn from_plan(plan: &ValidationPlan) -> Self {
        let mut commands: Vec<ValidationCommand> = plan
            .steps()
            .iter()
            .map(|step| step.command().clone())
            .collect();
        commands.sort();
        commands.dedup();
        Self { commands }
    }

    /// The commands, in command order.
    #[must_use]
    pub fn commands(&self) -> &[ValidationCommand] {
        &self.commands
    }

    /// How many distinct commands are proposed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether nothing is proposed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The proposal's identity — what the user is being asked about, as one value.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        let mut writer = DigestWriter::new(PROPOSAL_DOMAIN, Blake3::hasher());
        writer.sequence(&self.commands, |writer, command| {
            writer.digest(&command.digest().digest());
        });
        writer.finish()
    }

    /// The user approves this proposal, for this workspace, in this policy epoch.
    ///
    /// This is the only way a [`ValidationProfile`] comes into existence.
    #[must_use]
    pub fn approve(
        &self,
        approver: &HumanPrincipal,
        workspace: WorkspaceScope,
        epoch: PolicyEpoch,
    ) -> ValidationProfile {
        ValidationProfile {
            admitted: self
                .commands
                .iter()
                .map(ValidationCommand::digest)
                .collect(),
            approver: approver.key(),
            workspace,
            epoch,
            proposal: self.digest(),
        }
    }
}

/// The commands the user has agreed may run without being asked again.
///
/// Constructed only by [`ProfileProposal::approve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationProfile {
    admitted: BTreeSet<CommandDigest>,
    approver: ActorKey,
    workspace: WorkspaceScope,
    epoch: PolicyEpoch,
    proposal: Digest32,
}

impl ValidationProfile {
    /// Whether this exact command may run.
    ///
    /// Exact, by digest: a command differing in one argument byte is a different command and is
    /// not admitted.
    #[must_use]
    pub fn admits(&self, command: &ValidationCommand) -> bool {
        self.admitted.contains(&command.digest())
    }

    /// Whether the profile is still in force in `epoch`.
    ///
    /// A rotation puts every earlier profile out of force. Approving in a *later* epoch than the
    /// one in force is also out of force: it names an epoch this peer has not reached, and
    /// admitting it would let a profile from the future authorise a command today.
    #[must_use]
    pub fn in_force(&self, epoch: PolicyEpoch) -> bool {
        self.epoch == epoch
    }

    /// Whether the profile is scoped to `workspace`.
    #[must_use]
    pub fn scoped_to(&self, workspace: WorkspaceScope) -> bool {
        self.workspace == workspace
    }

    /// Whether every step of `plan` is admitted.
    #[must_use]
    pub fn covers(&self, plan: &ValidationPlan) -> bool {
        plan.steps().iter().all(|step| self.admits(step.command()))
    }

    /// The commands in `plan` this profile does not admit — exactly what a re-approval would add.
    #[must_use]
    pub fn unadmitted(&self, plan: &ValidationPlan) -> Vec<ValidationCommand> {
        let mut missing: Vec<ValidationCommand> = plan
            .steps()
            .iter()
            .filter(|step| !self.admits(step.command()))
            .map(|step| step.command().clone())
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// The key of the human who approved it.
    #[must_use]
    pub const fn approver(&self) -> ActorKey {
        self.approver
    }

    /// The workspace it is good in.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceScope {
        self.workspace
    }

    /// The policy epoch it was approved in.
    #[must_use]
    pub const fn epoch(&self) -> PolicyEpoch {
        self.epoch
    }

    /// The digest of the proposal that was approved.
    #[must_use]
    pub const fn proposal_digest(&self) -> Digest32 {
        self.proposal
    }

    /// How many commands it admits.
    #[must_use]
    pub fn len(&self) -> usize {
        self.admitted.len()
    }

    /// Whether it admits nothing. An approved empty proposal is legal and runs nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.admitted.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProfileProposal, ValidationProfile};
    use crate::change::{ChangedPath, PathEdit, ReviewChange};
    use crate::command::ValidationCommand;
    use crate::registry::{SandboxRequirement, ValidatorId, ValidatorRegistry, ValidatorSpec};
    use crate::selection::{plan_validation, ValidationPlan};
    use crate::tooling::ToolingInventory;
    use crate::trigger::ValidationTrigger;

    use mesh_crypto::WorkspaceScope;
    use mesh_policy::{HumanPrincipal, Principal};
    use mesh_types::{ActorKind, Digest32, PolicyEpoch};

    const WORKSPACE: WorkspaceScope = WorkspaceScope::from_bytes([0x5a; 16]);
    const OTHER_WORKSPACE: WorkspaceScope = WorkspaceScope::from_bytes([0xa5; 16]);
    const EPOCH: PolicyEpoch = PolicyEpoch::new(7);

    fn approver() -> HumanPrincipal {
        HumanPrincipal::enrol(Principal::new(
            mesh_crypto::ActorKey::from_public_bytes([9; 32]),
            ActorKind::Human,
        ))
        .expect("a human enrols")
    }

    fn plan_of(commands: &[&str]) -> ValidationPlan {
        let mut registry = ValidatorRegistry::empty();
        for name in commands {
            registry = registry
                .with(
                    ValidatorSpec::new(
                        ValidatorId::parse(name).expect("legal"),
                        ValidationCommand::at_root(name, []).expect("legal"),
                        [ValidationTrigger::Always],
                        SandboxRequirement::Isolated,
                    )
                    .expect("one trigger"),
                )
                .expect("distinct");
        }
        plan_validation(
            &registry,
            &ReviewChange::against(Digest32::from_bytes([4; 32]))
                .with_path(ChangedPath::new("a.rs", PathEdit::Added, 1).expect("legal"))
                .expect("no duplicate"),
            &ToolingInventory::empty(),
        )
    }

    fn profile_for(plan: &ValidationPlan) -> ValidationProfile {
        ProfileProposal::from_plan(plan).approve(&approver(), WORKSPACE, EPOCH)
    }

    #[test]
    fn a_proposal_de_duplicates_and_orders_commands() {
        let mut registry = ValidatorRegistry::empty();
        for name in ["one", "two"] {
            registry = registry
                .with(
                    ValidatorSpec::new(
                        ValidatorId::parse(name).expect("legal"),
                        ValidationCommand::at_root("same", []).expect("legal"),
                        [ValidationTrigger::Always],
                        SandboxRequirement::Isolated,
                    )
                    .expect("one trigger"),
                )
                .expect("distinct");
        }
        let plan = plan_validation(
            &registry,
            &ReviewChange::against(Digest32::from_bytes([4; 32])),
            &ToolingInventory::empty(),
        );
        assert_eq!(plan.len(), 2);
        assert_eq!(ProfileProposal::from_plan(&plan).len(), 1);
    }

    #[test]
    fn an_approved_profile_admits_exactly_what_was_proposed() {
        let plan = plan_of(&["alpha", "beta"]);
        let profile = profile_for(&plan);
        assert_eq!(profile.len(), 2);
        assert!(profile.covers(&plan));
        assert!(profile.unadmitted(&plan).is_empty());
        assert!(profile.admits(&ValidationCommand::at_root("alpha", []).expect("legal")));
        assert!(!profile.admits(&ValidationCommand::at_root("gamma", []).expect("legal")));
    }

    #[test]
    fn one_changed_argument_byte_is_a_command_the_profile_does_not_admit() {
        let profile = profile_for(&plan_of(&["alpha"]));
        let tampered = ValidationCommand::at_root("alpha", ["--now-with-an-argument".to_owned()])
            .expect("legal");
        assert!(!profile.admits(&tampered));
    }

    #[test]
    fn a_plan_that_grew_names_exactly_what_a_re_approval_would_add() {
        let narrow = plan_of(&["alpha"]);
        let profile = profile_for(&narrow);
        let wider = plan_of(&["alpha", "beta"]);
        assert!(!profile.covers(&wider));
        let missing = profile.unadmitted(&wider);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].program(), "beta");
    }

    #[test]
    fn a_plan_that_shrank_is_still_covered() {
        let wide = plan_of(&["alpha", "beta"]);
        let profile = profile_for(&wide);
        assert!(profile.covers(&plan_of(&["alpha"])));
    }

    #[test]
    fn a_profile_is_out_of_force_in_any_other_epoch() {
        let profile = profile_for(&plan_of(&["alpha"]));
        assert!(profile.in_force(EPOCH));
        assert!(!profile.in_force(PolicyEpoch::new(8)));
        assert!(!profile.in_force(PolicyEpoch::new(6)));
    }

    #[test]
    fn a_profile_is_scoped_to_one_workspace() {
        let profile = profile_for(&plan_of(&["alpha"]));
        assert!(profile.scoped_to(WORKSPACE));
        assert!(!profile.scoped_to(OTHER_WORKSPACE));
    }

    #[test]
    fn the_profile_records_who_approved_what() {
        let plan = plan_of(&["alpha"]);
        let proposal = ProfileProposal::from_plan(&plan);
        let profile = proposal.approve(&approver(), WORKSPACE, EPOCH);
        assert_eq!(profile.approver(), approver().key());
        assert_eq!(profile.proposal_digest(), proposal.digest());
        assert_eq!(profile.epoch(), EPOCH);
        assert_eq!(profile.workspace(), WORKSPACE);
    }

    #[test]
    fn an_empty_proposal_approves_to_an_empty_profile() {
        let profile = ProfileProposal::empty().approve(&approver(), WORKSPACE, EPOCH);
        assert!(profile.is_empty());
        assert!(ProfileProposal::empty().is_empty());
        assert!(profile.covers(&ValidationPlan::nothing_for(Digest32::from_bytes([4; 32]))));
    }

    #[test]
    fn the_proposal_digest_changes_with_the_proposal() {
        assert_ne!(
            ProfileProposal::from_plan(&plan_of(&["alpha"])).digest(),
            ProfileProposal::from_plan(&plan_of(&["alpha", "beta"])).digest()
        );
    }
}
