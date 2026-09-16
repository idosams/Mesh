//! The review bundle: the artifact a person approves, and the bytes an approval names.
//!
//! # What this type is for
//!
//! Mesh's whole thesis is that **only an exact human-reviewed state advances the protected shared
//! version**. That sentence is only checkable if "exactly this" has a name, and the name is only
//! trustworthy if two processes computing it from the same state agree to the byte. So
//! [`ReviewBundle`] has three properties, in this order of importance:
//!
//! 1. **It is computed, not declared.** [`compute_bundle`] reads states. There is no parameter
//!    through which an actor says what it changed — see [`crate::diff`].
//! 2. **It is deterministic.** Every collection it carries is ordered by content, nothing consults
//!    a clock or the environment, and [`ReviewBundle::id`] is the digest of the whole record under
//!    the framing in [`crate::digest`]. `tests/bundle.rs` generates a bundle in a *second process*
//!    and requires the same 64 hex characters, and separately mutates every field in turn and
//!    requires the identifier to move.
//! 3. **It is immutable.** Every field is private and no method takes `&mut self`. Work the actor
//!    does after the bundle exists cannot enter it, because there is no way in — a later checkpoint
//!    is a *different* bundle with a different name, and the approval still points at the one that
//!    was read.
//!
//! # What is in it, and why each part is in it rather than beside it
//!
//! | Part | Why it is inside |
//! |---|---|
//! | The canonical head and its state digest | An approval that does not name its base can be replayed onto a state nobody reviewed. |
//! | The actor, its head and its state digest | The bundle names whose work this is and exactly which checkpoint. |
//! | The change list | The authoritative diff. Replaying it onto the base reproduces the actor state. |
//! | Conflicts | Computed once, against the states the bundle names. Computing them at review time computes them against a state that may have moved. |
//! | Dependency impact | A reviewer shown three files and not the six artifacts they invalidate is being shown a partial truth. |
//! | Validation results | Evidence handed in, ordered here, and bound to this bundle rather than to whenever somebody last ran a validator. |
//! | The actor's explanation | Optional, and the only actor-supplied text in the record. It cannot change the change list; `tests/bundle.rs` asserts that. |
//!
//! # Refusal
//!
//! [`compute_bundle`] returns [`BundleRefusal`] rather than approximating. A case that cannot be
//! computed deterministically cannot be approved until it can — that is the task's
//! failure-and-recovery clause, and refusing is the whole of the safety.

use std::collections::BTreeSet;

use crate::conflict::{conflicts, Conflict};
use crate::diff::{apply, diff, ApplyError, DiffError, ObjectChange};
use crate::digest::{
    derive_id, Absorb, Blake3, CanonicalRecord, Digest32, DigestHasher, DigestWriter, DomainTag,
};
use crate::ids::{ActorId, HeadId, ObjectId, ReviewBundleId};
use crate::impact::{DependencyGraph, StaleOutput};
use crate::presentation::{present, DiffPresentation};
use crate::state::WorkspaceState;
use crate::validation::ValidationResult;

/// The longest explanation a bundle will carry, in bytes.
///
/// An explanation is prose a person reads next to a diff. A bound exists because an unbounded field
/// in a record whose identity is hashed is a way to make bundle computation cost whatever the actor
/// chooses; refusing an over-long one is cheaper than truncating, and truncation would put text in
/// the approved artifact that the actor did not write.
pub const MAX_EXPLANATION_BYTES: usize = 64 * 1024;

/// The domain a review bundle's identifier is derived in.
///
/// Versioned, because changing what the domain covers changes every identifier in it — which is a
/// protocol event and not an edit.
const BUNDLE_DOMAIN: DomainTag = DomainTag::new("mesh.v0.review-bundle");

/// Everything [`compute_bundle`] needs, and nothing it must not have.
///
/// Three states, because a review is a three-way question: what the actor departed from
/// (`fork`), where the canonical head is now (`canonical`), and what the actor is offering
/// (`actor`). The diff is `canonical → actor`; the conflicts are what the canonical head did over
/// objects the actor also touched since the fork.
///
/// There is deliberately no field here for "what the actor says it changed".
#[derive(Clone, Debug)]
pub struct BundleRequest {
    fork: WorkspaceState,
    canonical: WorkspaceState,
    canonical_head: HeadId,
    actor: WorkspaceState,
    actor_head: HeadId,
    author: ActorId,
    dependencies: DependencyGraph,
    validations: Vec<ValidationResult>,
    explanation: Option<String>,
}

impl BundleRequest {
    /// A request over the three states a review is a question about.
    ///
    /// `fork` is the state the actor departed from — the last state it and the canonical head
    /// agreed on. When the actor is up to date it is the same value as `canonical`, and the bundle
    /// carries no conflicts.
    #[must_use]
    pub fn new(
        fork: WorkspaceState,
        canonical: WorkspaceState,
        canonical_head: HeadId,
        actor: WorkspaceState,
        actor_head: HeadId,
        author: ActorId,
    ) -> Self {
        Self {
            fork,
            canonical,
            canonical_head,
            actor,
            actor_head,
            author,
            dependencies: DependencyGraph::new(),
            validations: Vec::new(),
            explanation: None,
        }
    }

    /// This request with a dependency graph, from which stale outputs are computed.
    #[must_use]
    pub fn with_dependencies(self, dependencies: DependencyGraph) -> Self {
        Self {
            dependencies,
            ..self
        }
    }

    /// This request with validation results attached. Order is not preserved and does not matter —
    /// [`compute_bundle`] sorts and de-duplicates them.
    #[must_use]
    pub fn with_validations(self, validations: Vec<ValidationResult>) -> Self {
        Self {
            validations,
            ..self
        }
    }

    /// This request with the actor's optional explanation.
    #[must_use]
    pub fn with_explanation(self, explanation: &str) -> Self {
        Self {
            explanation: Some(explanation.to_owned()),
            ..self
        }
    }
}

/// Why a bundle could not be computed. Refusing beats approximating: an approximated bundle is
/// approved for something other than what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleRefusal {
    /// The diff between the canonical state and the actor state could not be computed.
    Diff(DiffError),
    /// The diff between the fork and one of the two later states could not be computed, so the
    /// conflicts cannot be established and the bundle would understate what a reviewer faces.
    Ancestry(DiffError),
    /// The actor state is the canonical state. There is nothing to review, and an empty bundle
    /// would be an approval of nothing that still advanced a head.
    NothingToReview,
    /// The explanation exceeds [`MAX_EXPLANATION_BYTES`].
    ExplanationTooLong {
        /// How many bytes were supplied.
        found: usize,
    },
    /// A line of reviewable text contains a newline or a carriage return, so the rendering of the
    /// bundle would depend on who split it. Content is lines; a line is not.
    AmbiguousLine {
        /// The object whose content is ambiguous.
        object: ObjectId,
    },
    /// Replaying the computed change list onto the canonical state did not reproduce the actor
    /// state. Nothing known produces this; it is checked anyway, because the one claim the bundle
    /// makes is that it *is* the difference, and a claim nobody checks is a claim.
    NotSelfConsistent,
}

impl core::fmt::Display for BundleRefusal {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Diff(error) => write!(
                formatter,
                "the reviewed difference cannot be computed: {error}"
            ),
            Self::Ancestry(error) => {
                write!(formatter, "the conflicts cannot be established: {error}")
            }
            Self::NothingToReview => formatter
                .write_str("the actor state is the canonical state; there is nothing to review"),
            Self::ExplanationTooLong { found } => write!(
                formatter,
                "an explanation is at most {MAX_EXPLANATION_BYTES} bytes, found {found}"
            ),
            Self::AmbiguousLine { object } => write!(
                formatter,
                "object {object} holds a line containing a newline, so its rendering is not \
                 determined by its content"
            ),
            Self::NotSelfConsistent => formatter.write_str(
                "replaying the computed difference onto the base did not reproduce the actor state",
            ),
        }
    }
}

impl std::error::Error for BundleRefusal {}

impl From<ApplyError> for BundleRefusal {
    fn from(_: ApplyError) -> Self {
        Self::NotSelfConsistent
    }
}

/// The artifact a person approves.
///
/// Constructed only by [`compute_bundle`]. Every field is private, every accessor borrows, and no
/// method takes `&mut self` — so once it exists, nothing can be added to it. That is not a
/// convention: it is what makes an approval refer to exact bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewBundle {
    canonical_head: HeadId,
    canonical_state: Digest32,
    actor_head: HeadId,
    actor_state: Digest32,
    author: ActorId,
    changes: Vec<ObjectChange>,
    conflicts: Vec<Conflict>,
    stale_outputs: Vec<StaleOutput>,
    validations: Vec<ValidationResult>,
    explanation: Option<String>,
}

impl ReviewBundle {
    /// The name of these exact bytes: the digest of the whole record in the
    /// `mesh.v0.review-bundle` domain.
    ///
    /// Recomputed on demand rather than stored, so a bundle cannot carry an identifier that does
    /// not match it.
    #[must_use]
    pub fn id(&self) -> ReviewBundleId {
        derive_id::<Blake3, _>(self)
    }

    /// The canonical head this bundle would advance.
    #[must_use]
    pub const fn canonical_head(&self) -> HeadId {
        self.canonical_head
    }

    /// The digest of the exact canonical state the difference was computed against.
    #[must_use]
    pub const fn canonical_state(&self) -> Digest32 {
        self.canonical_state
    }

    /// The actor checkpoint this bundle is of.
    #[must_use]
    pub const fn actor_head(&self) -> HeadId {
        self.actor_head
    }

    /// The digest of the exact actor state offered.
    #[must_use]
    pub const fn actor_state(&self) -> Digest32 {
        self.actor_state
    }

    /// Whose work this is.
    #[must_use]
    pub const fn author(&self) -> ActorId {
        self.author
    }

    /// The authoritative difference, in a fixed order.
    #[must_use]
    pub fn changes(&self) -> &[ObjectChange] {
        &self.changes
    }

    /// Every object the difference touches, in identifier order.
    #[must_use]
    pub fn touched_objects(&self) -> BTreeSet<ObjectId> {
        self.changes.iter().map(ObjectChange::object).collect()
    }

    /// What the canonical head did over objects the actor also touched.
    #[must_use]
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// The derived outputs this work made stale.
    #[must_use]
    pub fn stale_outputs(&self) -> &[StaleOutput] {
        &self.stale_outputs
    }

    /// The validation results, in canonical order.
    #[must_use]
    pub fn validations(&self) -> &[ValidationResult] {
        &self.validations
    }

    /// Whether any validator failed. A reviewer may still approve; the bundle only reports.
    #[must_use]
    pub fn has_validation_failure(&self) -> bool {
        self.validations
            .iter()
            .any(|result| result.verdict().is_failure())
    }

    /// The actor's explanation, if it wrote one.
    ///
    /// The only actor-supplied text in the record, and it has no effect on
    /// [`ReviewBundle::changes`]. It is absorbed into [`ReviewBundle::id`] because it is part of
    /// what was read.
    #[must_use]
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }

    /// Replay this bundle's difference onto `base`.
    ///
    /// # Errors
    ///
    /// [`ApplyError`] when `base` is not the state the bundle was computed against.
    pub fn apply_to(&self, base: &WorkspaceState) -> Result<WorkspaceState, ApplyError> {
        apply(base, &self.changes)
    }

    /// This bundle rendered for review: the entries a surface draws, in a fixed order.
    ///
    /// Derived rather than stored, and deliberately outside [`ReviewBundle::id`]. A rendering
    /// inside the identifier would make every approval ever made depend on the version of the
    /// surface that drew it; a rendering derived from the change list is instead a *function* of
    /// the bytes that were approved, and [`DiffPresentation::digest`] names it separately.
    #[must_use]
    pub fn presentation(&self) -> DiffPresentation {
        present(&self.changes)
    }
}

impl Absorb for ReviewBundle {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.canonical_head.as_bytes());
        writer.digest(&self.canonical_state);
        writer.bytes(self.actor_head.as_bytes());
        writer.digest(&self.actor_state);
        writer.bytes(self.author.as_bytes());
        writer.sequence(&self.changes, |writer, item| item.absorb(writer));
        writer.sequence(&self.conflicts, |writer, item| item.absorb(writer));
        writer.sequence(&self.stale_outputs, |writer, item| item.absorb(writer));
        writer.sequence(&self.validations, |writer, item| item.absorb(writer));
        writer.option(self.explanation.as_ref(), |writer, explanation| {
            writer.text(explanation);
        });
    }
}

impl CanonicalRecord for ReviewBundle {
    type Id = ReviewBundleId;
    const DOMAIN: DomainTag = BUNDLE_DOMAIN;
}

/// Compute the bundle a person would approve, or refuse to.
///
/// # Errors
///
/// [`BundleRefusal`] in every case where the answer would have to be approximated. A refusal is not
/// a rejection of the work: the work stands and the checkpoint stands, and what is refused is the
/// claim that this particular artifact names exact bytes.
pub fn compute_bundle(request: &BundleRequest) -> Result<ReviewBundle, BundleRefusal> {
    if let Some(explanation) = &request.explanation {
        if explanation.len() > MAX_EXPLANATION_BYTES {
            return Err(BundleRefusal::ExplanationTooLong {
                found: explanation.len(),
            });
        }
    }
    reject_ambiguous_lines(&request.canonical)?;
    reject_ambiguous_lines(&request.actor)?;

    let changes = diff(&request.canonical, &request.actor).map_err(BundleRefusal::Diff)?;
    if changes.is_empty() {
        return Err(BundleRefusal::NothingToReview);
    }

    let landed = diff(&request.fork, &request.canonical).map_err(BundleRefusal::Ancestry)?;
    let authored = diff(&request.fork, &request.actor).map_err(BundleRefusal::Ancestry)?;

    let actor_state = request.actor.clone();
    let canonical_state = request.canonical.clone();
    let conflicts = conflicts(&landed, &authored, |object| {
        actor_state
            .path_of(object)
            .or_else(|| canonical_state.path_of(object))
    });

    let touched: BTreeSet<ObjectId> = changes.iter().map(ObjectChange::object).collect();
    let stale_outputs = request.dependencies.stale_outputs(&touched);

    let mut validations = request.validations.clone();
    validations.sort();
    validations.dedup();

    let bundle = ReviewBundle {
        canonical_head: request.canonical_head,
        canonical_state: request.canonical.digest(),
        actor_head: request.actor_head,
        actor_state: request.actor.digest(),
        author: request.author,
        changes,
        conflicts,
        stale_outputs,
        validations,
        explanation: request.explanation.clone(),
    };

    if bundle.apply_to(&request.canonical)? != request.actor {
        return Err(BundleRefusal::NotSelfConsistent);
    }
    Ok(bundle)
}

/// Refuse a state holding a "line" that is not one.
///
/// Content is lines with newlines excluded. A line holding a newline renders as two lines in one
/// surface and as an escaped literal in another, which means the artifact a person approved is not
/// determined by the state it was computed from.
fn reject_ambiguous_lines(state: &WorkspaceState) -> Result<(), BundleRefusal> {
    for (object, held) in state.objects() {
        let Some(lines) = held.content().and_then(crate::state::Content::lines) else {
            continue;
        };
        if lines
            .iter()
            .any(|line| line.contains('\n') || line.contains('\r'))
        {
            return Err(BundleRefusal::AmbiguousLine { object: *object });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::VersionId;
    use crate::name::NormalizedName;
    use crate::state::Content;
    use crate::validation::Verdict;

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn text(byte: u8, line: &str) -> Content {
        Content::Text {
            version: VersionId::from_bytes([byte; 32]),
            lines: vec![line.to_owned()],
        }
    }

    fn root() -> ObjectId {
        ObjectId::from_bytes([0; 16])
    }

    fn notes() -> ObjectId {
        ObjectId::from_bytes([1; 16])
    }

    fn canonical() -> WorkspaceState {
        WorkspaceState::new(root()).with_file(notes(), root(), name("notes.md"), text(1, "one"))
    }

    fn actor() -> WorkspaceState {
        canonical().with_file(notes(), root(), name("notes.md"), text(2, "two"))
    }

    fn request() -> BundleRequest {
        BundleRequest::new(
            canonical(),
            canonical(),
            HeadId::from_bytes([10; 32]),
            actor(),
            HeadId::from_bytes([11; 32]),
            ActorId::from_bytes([12; 32]),
        )
    }

    #[test]
    fn a_bundle_names_its_two_states_and_its_author() {
        let bundle = compute_bundle(&request()).unwrap();
        assert_eq!(bundle.canonical_head(), HeadId::from_bytes([10; 32]));
        assert_eq!(bundle.actor_head(), HeadId::from_bytes([11; 32]));
        assert_eq!(bundle.author(), ActorId::from_bytes([12; 32]));
        assert_eq!(bundle.canonical_state(), canonical().digest());
        assert_eq!(bundle.actor_state(), actor().digest());
    }

    #[test]
    fn a_bundle_replays_onto_its_base_and_reproduces_the_actor_state() {
        let bundle = compute_bundle(&request()).unwrap();
        assert_eq!(bundle.apply_to(&canonical()).unwrap(), actor());
    }

    #[test]
    fn nothing_to_review_is_refused() {
        let request = BundleRequest::new(
            canonical(),
            canonical(),
            HeadId::from_bytes([10; 32]),
            canonical(),
            HeadId::from_bytes([11; 32]),
            ActorId::from_bytes([12; 32]),
        );
        assert_eq!(
            compute_bundle(&request),
            Err(BundleRefusal::NothingToReview)
        );
    }

    #[test]
    fn an_over_long_explanation_is_refused() {
        let request = request().with_explanation(&"x".repeat(MAX_EXPLANATION_BYTES + 1));
        assert_eq!(
            compute_bundle(&request),
            Err(BundleRefusal::ExplanationTooLong {
                found: MAX_EXPLANATION_BYTES + 1
            })
        );
    }

    #[test]
    fn a_line_holding_a_newline_is_refused() {
        let broken = canonical().with_file(
            notes(),
            root(),
            name("notes.md"),
            Content::Text {
                version: VersionId::from_bytes([3; 32]),
                lines: vec!["one\ntwo".to_owned()],
            },
        );
        let request = BundleRequest::new(
            canonical(),
            canonical(),
            HeadId::from_bytes([10; 32]),
            broken,
            HeadId::from_bytes([11; 32]),
            ActorId::from_bytes([12; 32]),
        );
        assert_eq!(
            compute_bundle(&request),
            Err(BundleRefusal::AmbiguousLine { object: notes() })
        );
    }

    #[test]
    fn a_state_that_is_not_a_version_of_the_same_workspace_is_refused() {
        let elsewhere = WorkspaceState::new(ObjectId::from_bytes([9; 16]));
        let request = BundleRequest::new(
            canonical(),
            canonical(),
            HeadId::from_bytes([10; 32]),
            elsewhere,
            HeadId::from_bytes([11; 32]),
            ActorId::from_bytes([12; 32]),
        );
        assert!(matches!(
            compute_bundle(&request),
            Err(BundleRefusal::Diff(_))
        ));
    }

    #[test]
    fn validation_results_are_ordered_and_de_duplicated() {
        let one = ValidationResult::new("schema", None, Verdict::Passed, "");
        let two = ValidationResult::new("lint", None, Verdict::Failed, "long line");
        let forwards = request().with_validations(vec![one.clone(), two.clone(), one.clone()]);
        let backwards = request().with_validations(vec![two, one]);
        let first = compute_bundle(&forwards).unwrap();
        let second = compute_bundle(&backwards).unwrap();
        assert_eq!(first.validations().len(), 2);
        assert_eq!(first.id(), second.id());
        assert!(first.has_validation_failure());
    }

    #[test]
    fn the_explanation_is_carried_but_changes_nothing_about_the_difference() {
        let plain = compute_bundle(&request()).unwrap();
        let explained = compute_bundle(&request().with_explanation("I tidied the notes")).unwrap();
        assert_eq!(plain.changes(), explained.changes());
        assert_eq!(explained.explanation(), Some("I tidied the notes"));
        assert_ne!(plain.id(), explained.id());
    }

    #[test]
    fn a_bundle_with_no_conflicts_reports_none() {
        let bundle = compute_bundle(&request()).unwrap();
        assert!(bundle.conflicts().is_empty());
        assert!(bundle.stale_outputs().is_empty());
        assert_eq!(bundle.touched_objects(), BTreeSet::from([notes()]));
    }

    #[test]
    fn a_head_that_moved_over_the_actors_work_is_a_conflict_in_the_bundle() {
        let fork = canonical();
        let moved_on =
            fork.clone()
                .with_file(notes(), root(), name("notes.md"), text(5, "canonical"));
        let request = BundleRequest::new(
            fork,
            moved_on,
            HeadId::from_bytes([10; 32]),
            actor(),
            HeadId::from_bytes([11; 32]),
            ActorId::from_bytes([12; 32]),
        );
        let bundle = compute_bundle(&request).unwrap();
        assert_eq!(bundle.conflicts().len(), 1);
        assert_eq!(bundle.conflicts()[0].object(), notes());
        assert_eq!(bundle.conflicts()[0].path(), Some("/notes.md"));
    }

    #[test]
    fn dependency_impact_is_computed_into_the_bundle() {
        let derived = ObjectId::from_bytes([4; 16]);
        let graph = DependencyGraph::new().with_edge(derived, notes());
        let bundle = compute_bundle(&request().with_dependencies(graph)).unwrap();
        assert_eq!(bundle.stale_outputs().len(), 1);
        assert_eq!(bundle.stale_outputs()[0].output(), derived);
    }
}
