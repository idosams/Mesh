//! The validation record: what ran, in what environment, with what exit status, producing what.
//!
//! # Fail closed, expressed in the type
//!
//! The obvious way to model an outcome is an exit status and a flag. That model has a
//! representable lie in it: a step that never started still has an exit status field, and the
//! value somebody puts there is nearly always zero. [`RunOutcome`] has no such field to fill —
//! only [`RunOutcome::Executed`] carries an [`ExitStatus`], and a step that was blocked or faulted
//! carries a reason instead. So "an unknown or crashed validator reads as a pass" is not a bug
//! that has to be avoided; it is a value that cannot be written.
//!
//! [`RunVerdict`] follows from the outcome by a total function, and exactly one of its five words
//! means the subject satisfied the rule. `RunVerdict::Errored` and `RunVerdict::Blocked` are both
//! `!is_pass()`, and [`RunLedger::clears`] requires a `Passed` record for **every** planned step,
//! so a step with no record at all fails the check too — the absent-evidence case, which is the
//! one a boolean would have got wrong.
//!
//! # Every executed run records four things
//!
//! Command, environment digest, exit status, artifacts. The environment digest is
//! `Option`-shaped, and the reason is honesty rather than convenience: a step that never ran
//! observed no environment, and giving it the digest of an empty environment would record a
//! measurement nobody made. `every_executed_record_carries_an_environment` asserts the other
//! direction — that a run which *did* execute always has one.

use mesh_types::{Absorb, Blake3, ContentDigest, Digest32, DigestHasher, DigestWriter, DomainTag};

use crate::command::{CommandDigest, EnvironmentDigest, ValidationCommand};
use crate::registry::ValidatorId;
use crate::selection::ValidationPlan;

/// The domain a validation record's identifier is derived in.
const RECORD_DOMAIN: DomainTag = DomainTag::new("mesh.v0.validator.record");

/// The domain a ledger's digest is derived in.
const LEDGER_DOMAIN: DomainTag = DomainTag::new("mesh.v0.validator.ledger");

/// The longest artifact name this crate will carry, in bytes.
pub const MAX_ARTIFACT_NAME_BYTES: usize = 256;

/// A validation record's identifier, derived from its content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidationRecordId(Digest32);

impl ValidationRecordId {
    /// The identifier's bytes.
    #[must_use]
    pub const fn digest(self) -> Digest32 {
        self.0
    }

    /// The identifier as lowercase hexadecimal.
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

impl From<Digest32> for ValidationRecordId {
    fn from(digest: Digest32) -> Self {
        Self(digest)
    }
}

/// How a process that ran ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExitStatus {
    /// It exited with this code.
    Code(i32),
    /// It was killed by this signal. Never a success, whatever the signal.
    Signal(u8),
}

impl ExitStatus {
    /// Whether the process reported success. True for exactly one value: code zero.
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Code(0))
    }

    /// The status's stable wire form.
    #[must_use]
    pub fn to_wire(self) -> String {
        match self {
            Self::Code(code) => format!("exit:{code}"),
            Self::Signal(signal) => format!("signal:{signal}"),
        }
    }
}

impl core::fmt::Display for ExitStatus {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.to_wire())
    }
}

/// Why a step never started.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockReason {
    /// The user has not approved a validation profile for this project.
    AwaitingProfileApproval,
    /// The workspace handed to the run holds a different review snapshot than the plan was made
    /// for, so running would validate a tree nobody planned for and produce evidence that names
    /// the wrong review.
    WorkspaceSnapshotMismatch,
    /// The approved profile does not admit this exact command.
    NotInProfile,
    /// The profile was approved in a policy epoch that is no longer in force.
    ProfileEpochSuperseded,
    /// The profile was approved for a different workspace.
    ProfileWorkspaceMismatch,
    /// The command cannot be confined to the isolated workspace, so it is never started
    /// automatically. The reason it needs the host travels with it.
    NotSandboxable(String),
}

impl BlockReason {
    /// The reason's stable wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AwaitingProfileApproval => "awaiting-profile-approval",
            Self::WorkspaceSnapshotMismatch => "workspace-snapshot-mismatch",
            Self::NotInProfile => "not-in-profile",
            Self::ProfileEpochSuperseded => "profile-epoch-superseded",
            Self::ProfileWorkspaceMismatch => "profile-workspace-mismatch",
            Self::NotSandboxable(_) => "not-sandboxable",
        }
    }

    /// Whether this block is one the user can clear by approving a profile.
    #[must_use]
    pub const fn clears_with_approval(&self) -> bool {
        matches!(
            self,
            Self::AwaitingProfileApproval
                | Self::NotInProfile
                | Self::ProfileEpochSuperseded
                | Self::ProfileWorkspaceMismatch
        )
    }
}

impl core::fmt::Display for BlockReason {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotSandboxable(why) => write!(formatter, "not-sandboxable: {why}"),
            other => formatter.write_str(other.as_str()),
        }
    }
}

/// Why a step that started produced no usable answer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunFault {
    /// The executor could not tell what happened.
    ExecutorFailed(String),
    /// The process died without an exit status.
    Crashed,
    /// The run exceeded its budget and was stopped.
    TimedOut,
    /// The sandbox could not be established.
    SandboxUnavailable,
    /// The snapshot the run reported afterwards is not the one it was given, so the run changed
    /// what it was validating.
    SnapshotDrift,
}

impl RunFault {
    /// The fault's stable wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ExecutorFailed(_) => "executor-failed",
            Self::Crashed => "crashed",
            Self::TimedOut => "timed-out",
            Self::SandboxUnavailable => "sandbox-unavailable",
            Self::SnapshotDrift => "snapshot-drift",
        }
    }
}

impl core::fmt::Display for RunFault {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ExecutorFailed(detail) => write!(formatter, "executor-failed: {detail}"),
            other => formatter.write_str(other.as_str()),
        }
    }
}

/// What one step's outcome was. Exactly one variant carries an exit status.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunOutcome {
    /// A process ran and ended.
    Executed {
        /// How it ended.
        exit: ExitStatus,
    },
    /// Nothing was started.
    Blocked {
        /// Why not.
        reason: BlockReason,
    },
    /// Something was attempted and produced no usable answer.
    Faulted {
        /// What went wrong.
        fault: RunFault,
    },
    /// The validator judged the change out of its scope.
    Skipped {
        /// Why it does not apply.
        why: String,
    },
}

impl RunOutcome {
    /// The verdict this outcome implies. Total, and the only route to [`RunVerdict::Passed`].
    #[must_use]
    pub const fn verdict(&self) -> RunVerdict {
        match self {
            Self::Executed { exit } => {
                if exit.is_success() {
                    RunVerdict::Passed
                } else {
                    RunVerdict::Failed
                }
            }
            Self::Blocked { .. } => RunVerdict::Blocked,
            Self::Faulted { .. } => RunVerdict::Errored,
            Self::Skipped { .. } => RunVerdict::Skipped,
        }
    }

    /// The exit status, when a process ran. `None` otherwise, because there was none.
    #[must_use]
    pub const fn exit(&self) -> Option<ExitStatus> {
        match self {
            Self::Executed { exit } => Some(*exit),
            _ => None,
        }
    }

    /// The outcome's stable wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Executed { .. } => "executed",
            Self::Blocked { .. } => "blocked",
            Self::Faulted { .. } => "faulted",
            Self::Skipped { .. } => "skipped",
        }
    }
}

/// What a validator concluded, in five words.
///
/// The three words a review bundle absorbs — passed, failed, skipped — plus the two this crate
/// needs to say that nothing was concluded at all. `tests/approval-vocabulary.rs` asserts the
/// first three are exactly `mesh-approval`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunVerdict {
    /// The subject satisfies the rule.
    Passed,
    /// The subject violates the rule.
    Failed,
    /// The rule does not apply to this subject.
    Skipped,
    /// Nothing ran, so nothing is known.
    Blocked,
    /// Something ran and produced no usable answer, so nothing is known.
    Errored,
}

impl RunVerdict {
    /// Every verdict.
    pub const ALL: [Self; 5] = [
        Self::Passed,
        Self::Failed,
        Self::Skipped,
        Self::Blocked,
        Self::Errored,
    ];

    /// Whether this verdict is evidence the subject satisfies the rule. True for exactly one word.
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Passed)
    }

    /// Whether this verdict means no conclusion was reached.
    #[must_use]
    pub const fn is_inconclusive(self) -> bool {
        matches!(self, Self::Blocked | Self::Errored)
    }

    /// The verdict's stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Blocked => "blocked",
            Self::Errored => "errored",
        }
    }

    /// This verdict in the three-word vocabulary a review bundle absorbs.
    ///
    /// The handoff, and the one place the narrowing happens. A review bundle knows *passed*,
    /// *failed* and *skipped*; this crate additionally knows *blocked* and *errored*, and both of
    /// those mean nothing was established. They narrow to **failed**, never to skipped and never
    /// to passed: a bundle carrying "skipped" for a validator that crashed would tell a reviewer
    /// the rule did not apply, which is a different and false statement.
    #[must_use]
    pub const fn bundle_word(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Skipped => "skipped",
            Self::Failed | Self::Blocked | Self::Errored => "failed",
        }
    }
}

impl core::fmt::Display for RunVerdict {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Something a validation run produced, named by content.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Artifact {
    name: String,
    content: Digest32,
    bytes: u64,
}

impl Artifact {
    /// One artifact.
    ///
    /// # Errors
    ///
    /// [`ArtifactError`] when the name is empty, over [`MAX_ARTIFACT_NAME_BYTES`], or holds a
    /// path separator or a NUL — an artifact name is a label, never a path.
    pub fn new(name: &str, content: Digest32, bytes: u64) -> Result<Self, ArtifactError> {
        if name.is_empty() {
            return Err(ArtifactError::EmptyName);
        }
        if name.len() > MAX_ARTIFACT_NAME_BYTES {
            return Err(ArtifactError::NameTooLong { bytes: name.len() });
        }
        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return Err(ArtifactError::NameIsAPath(name.to_owned()));
        }
        Ok(Self {
            name: name.to_owned(),
            content,
            bytes,
        })
    }

    /// What it is called.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The digest of its content, which is how it is fetched and how it is compared.
    #[must_use]
    pub const fn content(&self) -> Digest32 {
        self.content
    }

    /// How large it is.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Absorb for Artifact {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(&self.name);
        writer.digest(&self.content);
        writer.u64(self.bytes);
    }
}

/// Why an artifact was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactError {
    /// The name was empty.
    EmptyName,
    /// The name was over [`MAX_ARTIFACT_NAME_BYTES`].
    NameTooLong {
        /// How many bytes it held.
        bytes: usize,
    },
    /// The name held a path separator or a NUL.
    NameIsAPath(String),
}

impl core::fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("an artifact name is empty"),
            Self::NameTooLong { bytes } => {
                write!(formatter, "an artifact name is {bytes} bytes")
            }
            Self::NameIsAPath(name) => write!(formatter, "`{name}` is a path, not a label"),
        }
    }
}

impl std::error::Error for ArtifactError {}

/// One validation run, recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationRecord {
    validator: ValidatorId,
    command: ValidationCommand,
    environment: Option<EnvironmentDigest>,
    snapshot: Digest32,
    outcome: RunOutcome,
    artifacts: Vec<Artifact>,
}

impl ValidationRecord {
    /// Record one run.
    ///
    /// `environment` is `Some` exactly when a process ran. Artifacts are sorted and de-duplicated,
    /// so a caller's collection order does not reach the record's identifier.
    #[must_use]
    pub fn new(
        validator: ValidatorId,
        command: ValidationCommand,
        environment: Option<EnvironmentDigest>,
        snapshot: Digest32,
        outcome: RunOutcome,
        artifacts: impl IntoIterator<Item = Artifact>,
    ) -> Self {
        let mut artifacts: Vec<Artifact> = artifacts.into_iter().collect();
        artifacts.sort();
        artifacts.dedup();
        Self {
            validator,
            command,
            environment,
            snapshot,
            outcome,
            artifacts,
        }
    }

    /// Which validator.
    #[must_use]
    pub const fn validator(&self) -> &ValidatorId {
        &self.validator
    }

    /// What it was asked to run.
    #[must_use]
    pub const fn command(&self) -> &ValidationCommand {
        &self.command
    }

    /// The command's identity, which is what a profile admits.
    #[must_use]
    pub fn command_digest(&self) -> CommandDigest {
        self.command.digest()
    }

    /// The environment a process observed, when one ran.
    #[must_use]
    pub const fn environment(&self) -> Option<EnvironmentDigest> {
        self.environment
    }

    /// The immutable review snapshot the run was against.
    #[must_use]
    pub const fn snapshot(&self) -> Digest32 {
        self.snapshot
    }

    /// What happened.
    #[must_use]
    pub const fn outcome(&self) -> &RunOutcome {
        &self.outcome
    }

    /// What it concluded.
    #[must_use]
    pub const fn verdict(&self) -> RunVerdict {
        self.outcome.verdict()
    }

    /// The exit status, when a process ran.
    #[must_use]
    pub const fn exit(&self) -> Option<ExitStatus> {
        self.outcome.exit()
    }

    /// What it produced, in name order.
    #[must_use]
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    /// This record's identifier, derived from its content.
    #[must_use]
    pub fn id(&self) -> ValidationRecordId {
        let mut writer = DigestWriter::new(RECORD_DOMAIN, Blake3::hasher());
        self.absorb(&mut writer);
        ValidationRecordId(writer.finish())
    }
}

impl Absorb for ValidationRecord {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(self.validator.as_str());
        self.command.absorb(writer);
        writer.option(self.environment.as_ref(), |writer, environment| {
            writer.digest(&environment.digest());
        });
        writer.digest(&self.snapshot);
        writer.text(self.outcome.as_str());
        writer.text(&outcome_detail(&self.outcome));
        writer.sequence(&self.artifacts, |writer, artifact| artifact.absorb(writer));
    }
}

/// The outcome's payload, as one framed string.
fn outcome_detail(outcome: &RunOutcome) -> String {
    match outcome {
        RunOutcome::Executed { exit } => exit.to_wire(),
        RunOutcome::Blocked { reason } => reason.to_string(),
        RunOutcome::Faulted { fault } => fault.to_string(),
        RunOutcome::Skipped { why } => why.clone(),
    }
}

/// Every record from one validation pass, in a fixed order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunLedger {
    records: Vec<ValidationRecord>,
}

impl RunLedger {
    /// A ledger with nothing in it.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// A ledger over `records`, ordered by validator and then by command digest.
    #[must_use]
    pub fn of(records: impl IntoIterator<Item = ValidationRecord>) -> Self {
        let mut records: Vec<ValidationRecord> = records.into_iter().collect();
        records.sort_by(|left, right| {
            left.validator()
                .cmp(right.validator())
                .then_with(|| left.command_digest().cmp(&right.command_digest()))
        });
        Self { records }
    }

    /// Every record, in order.
    #[must_use]
    pub fn records(&self) -> &[ValidationRecord] {
        &self.records
    }

    /// How many runs are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Every record whose verdict is `verdict`.
    pub fn with_verdict(&self, verdict: RunVerdict) -> impl Iterator<Item = &ValidationRecord> {
        self.records
            .iter()
            .filter(move |record| record.verdict() == verdict)
    }

    /// Whether any record reached no conclusion.
    #[must_use]
    pub fn has_inconclusive(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.verdict().is_inconclusive())
    }

    /// Whether every step of `plan` has evidence, against `plan`'s own review, and all of it passed.
    ///
    /// The fail-closed oracle, and the three things it refuses:
    ///
    /// * **Absent evidence.** A plan step with no record at all fails, which is the case a "no
    ///   failures recorded" check would have called clean.
    /// * **Evidence of another review.** A record whose [`snapshot`](ValidationRecord::snapshot) is
    ///   not [`ValidationPlan::snapshot`] does not count as evidence here — it is not even
    ///   considered, so it cannot supply the one passing record a step needs. Nothing about a
    ///   validator identifier or a command digest mentions a snapshot (`cargo nextest run` is the
    ///   same command whatever tree it was pointed at), so without this the ledger of a review that
    ///   passed clears a review nothing ever ran against. That is the transferable form of a
    ///   validator deciding its own verdict, because the actor chooses which ledger to submit.
    /// * **A failure with a pass beside it.** Every matching record must pass, not merely one of
    ///   them. A ledger carries no ordering, so there is nothing that could justify reading a pass
    ///   as superseding a failure recorded for the same step; a re-run produces a fresh ledger.
    ///
    /// `tests/evidence-replay.rs` constructs all three as an attacker would.
    #[must_use]
    pub fn clears(&self, plan: &ValidationPlan) -> bool {
        plan.steps().iter().all(|step| {
            let mut evidence = false;
            for record in &self.records {
                if record.snapshot() != plan.snapshot()
                    || record.validator() != step.validator()
                    || record.command_digest() != step.command().digest()
                {
                    continue;
                }
                if !record.verdict().is_pass() {
                    return false;
                }
                evidence = true;
            }
            evidence
        })
    }

    /// The ledger's identity, so a review bundle carrying validation evidence is a different
    /// bundle when the evidence differs.
    #[must_use]
    pub fn digest(&self) -> Digest32 {
        let mut writer = DigestWriter::new(LEDGER_DOMAIN, Blake3::hasher());
        writer.sequence(&self.records, |writer, record| {
            writer.digest(&record.id().digest());
        });
        writer.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Artifact, ArtifactError, BlockReason, ExitStatus, RunFault, RunLedger, RunOutcome,
        RunVerdict, ValidationRecord, MAX_ARTIFACT_NAME_BYTES,
    };
    use crate::command::{EnvironmentDigest, ValidationCommand};
    use crate::registry::ValidatorId;
    use mesh_types::Digest32;

    fn snapshot() -> Digest32 {
        Digest32::from_bytes([8; 32])
    }

    fn record(outcome: RunOutcome) -> ValidationRecord {
        let environment = match outcome {
            RunOutcome::Executed { .. } => Some(EnvironmentDigest::empty()),
            _ => None,
        };
        ValidationRecord::new(
            ValidatorId::parse("cargo-test").expect("legal"),
            ValidationCommand::at_root("cargo", ["test".to_owned()]).expect("legal"),
            environment,
            snapshot(),
            outcome,
            [],
        )
    }

    #[test]
    fn only_a_zero_exit_is_a_pass() {
        assert!(ExitStatus::Code(0).is_success());
        for status in [
            ExitStatus::Code(1),
            ExitStatus::Code(-1),
            ExitStatus::Code(101),
            ExitStatus::Signal(9),
            ExitStatus::Signal(0),
        ] {
            assert!(!status.is_success(), "{status} was read as success");
        }
    }

    #[test]
    fn no_outcome_except_a_zero_exit_produces_a_pass() {
        assert_eq!(
            RunOutcome::Executed {
                exit: ExitStatus::Code(0)
            }
            .verdict(),
            RunVerdict::Passed
        );
        let never_passing = [
            RunOutcome::Executed {
                exit: ExitStatus::Code(1),
            },
            RunOutcome::Executed {
                exit: ExitStatus::Signal(11),
            },
            RunOutcome::Blocked {
                reason: BlockReason::AwaitingProfileApproval,
            },
            RunOutcome::Blocked {
                reason: BlockReason::NotInProfile,
            },
            RunOutcome::Faulted {
                fault: RunFault::Crashed,
            },
            RunOutcome::Faulted {
                fault: RunFault::SnapshotDrift,
            },
            RunOutcome::Faulted {
                fault: RunFault::ExecutorFailed("unknown validator".to_owned()),
            },
            RunOutcome::Skipped {
                why: "not applicable".to_owned(),
            },
        ];
        for outcome in never_passing {
            assert!(
                !outcome.verdict().is_pass(),
                "`{}` produced a pass",
                outcome.as_str()
            );
        }
    }

    #[test]
    fn an_outcome_that_did_not_execute_has_no_exit_status_to_read() {
        assert_eq!(
            RunOutcome::Blocked {
                reason: BlockReason::NotInProfile
            }
            .exit(),
            None
        );
        assert_eq!(
            RunOutcome::Faulted {
                fault: RunFault::TimedOut
            }
            .exit(),
            None
        );
        assert_eq!(
            RunOutcome::Executed {
                exit: ExitStatus::Code(3)
            }
            .exit(),
            Some(ExitStatus::Code(3))
        );
    }

    #[test]
    fn every_executed_record_carries_an_environment_and_no_other_one_does() {
        let executed = record(RunOutcome::Executed {
            exit: ExitStatus::Code(0),
        });
        assert!(executed.environment().is_some());
        let blocked = record(RunOutcome::Blocked {
            reason: BlockReason::AwaitingProfileApproval,
        });
        assert!(blocked.environment().is_none());
    }

    #[test]
    fn the_record_identifier_separates_every_field() {
        let base = record(RunOutcome::Executed {
            exit: ExitStatus::Code(0),
        });
        let failed = record(RunOutcome::Executed {
            exit: ExitStatus::Code(1),
        });
        let blocked = record(RunOutcome::Blocked {
            reason: BlockReason::NotInProfile,
        });
        let other_block = record(RunOutcome::Blocked {
            reason: BlockReason::AwaitingProfileApproval,
        });
        let ids = [base.id(), failed.id(), blocked.id(), other_block.id()];
        for (at, id) in ids.iter().enumerate() {
            assert!(!ids[at + 1..].contains(id), "an identifier is repeated");
        }
        assert_eq!(base.id(), base.clone().id());
    }

    #[test]
    fn artifacts_are_ordered_however_they_arrive() {
        let one = Artifact::new("junit.xml", Digest32::from_bytes([1; 32]), 10).expect("legal");
        let two = Artifact::new("stdout.txt", Digest32::from_bytes([2; 32]), 20).expect("legal");
        let forwards = ValidationRecord::new(
            ValidatorId::parse("a").expect("legal"),
            ValidationCommand::at_root("a", []).expect("legal"),
            None,
            snapshot(),
            RunOutcome::Skipped {
                why: "x".to_owned(),
            },
            [one.clone(), two.clone()],
        );
        let backwards = ValidationRecord::new(
            ValidatorId::parse("a").expect("legal"),
            ValidationCommand::at_root("a", []).expect("legal"),
            None,
            snapshot(),
            RunOutcome::Skipped {
                why: "x".to_owned(),
            },
            [two, one],
        );
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.id(), backwards.id());
        assert_eq!(forwards.artifacts()[0].name(), "junit.xml");
    }

    #[test]
    fn an_artifact_name_is_a_label_and_not_a_path() {
        for name in ["", "a/b", "a\\b", "a\0b"] {
            assert!(
                Artifact::new(name, Digest32::from_bytes([0; 32]), 0).is_err(),
                "`{name}` was accepted"
            );
        }
        let long = "a".repeat(MAX_ARTIFACT_NAME_BYTES + 1);
        assert_eq!(
            Artifact::new(&long, Digest32::from_bytes([0; 32]), 0),
            Err(ArtifactError::NameTooLong {
                bytes: MAX_ARTIFACT_NAME_BYTES + 1
            })
        );
        assert!(Artifact::new("junit.xml", Digest32::from_bytes([0; 32]), 4).is_ok());
    }

    #[test]
    fn the_ledger_orders_records_however_they_arrive() {
        let first = ValidationRecord::new(
            ValidatorId::parse("a").expect("legal"),
            ValidationCommand::at_root("a", []).expect("legal"),
            None,
            snapshot(),
            RunOutcome::Skipped { why: String::new() },
            [],
        );
        let second = ValidationRecord::new(
            ValidatorId::parse("b").expect("legal"),
            ValidationCommand::at_root("b", []).expect("legal"),
            None,
            snapshot(),
            RunOutcome::Skipped { why: String::new() },
            [],
        );
        let forwards = RunLedger::of([first.clone(), second.clone()]);
        let backwards = RunLedger::of([second, first]);
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.digest(), backwards.digest());
        assert_eq!(forwards.records()[0].validator().as_str(), "a");
    }

    #[test]
    fn an_empty_ledger_reports_itself_empty() {
        let empty = RunLedger::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(!empty.has_inconclusive());
        assert_eq!(empty.with_verdict(RunVerdict::Passed).count(), 0);
    }

    #[test]
    fn the_verdict_words_are_distinct_and_exactly_one_is_a_pass() {
        let names: Vec<&str> = RunVerdict::ALL.iter().map(|word| word.as_str()).collect();
        for (at, name) in names.iter().enumerate() {
            assert!(!names[at + 1..].contains(name), "`{name}` is repeated");
        }
        assert_eq!(
            RunVerdict::ALL
                .iter()
                .filter(|verdict| verdict.is_pass())
                .count(),
            1
        );
        assert_eq!(
            RunVerdict::ALL
                .iter()
                .filter(|verdict| verdict.is_inconclusive())
                .count(),
            2
        );
    }

    #[test]
    fn a_block_that_a_profile_approval_cannot_clear_says_so() {
        assert!(BlockReason::AwaitingProfileApproval.clears_with_approval());
        assert!(BlockReason::NotInProfile.clears_with_approval());
        assert!(!BlockReason::NotSandboxable("needs the network".to_owned()).clears_with_approval());
    }
}
