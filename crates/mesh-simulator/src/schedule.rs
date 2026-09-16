//! The seeded schedule and its stable failure-record representation.

use core::fmt;

use mesh_state::{
    EventId, IdentityChange, IdentityOutcome, Lamport, NormalizedName, ObjectId, ObjectKind,
    ObjectRegister, Stamp, VersionId,
};

/// The version written into every schedule and failure record.
///
/// Version 1 expands the seeded generator from four to all six identity-operation families.
pub const SIMULATOR_PROTOCOL_VERSION: &str = "mesh-simulator/1";

/// The fixed prefix needs eight deliveries to cover the five standing scenario families.
pub const MIN_SMOKE_STEPS: usize = 8;

const OBJECT_COUNT: u8 = 4;
const FIRST_SIMULATION_LAMPORT: u64 = 32;
const SUPPORT_DIRECTORY_COUNT: u8 = 2;

/// One family in the core identity-change vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperationFamily {
    /// Mint a stable object identity.
    Create,
    /// Bind an object into a directory.
    Link,
    /// Remove an object's current name binding.
    Unlink,
    /// Change an object's name in one directory.
    Rename,
    /// Move an object between directories.
    Move,
    /// Record a new immutable content version.
    WriteVersion,
}

impl OperationFamily {
    /// Every generated family in stable order.
    pub const ALL: [Self; 6] = [
        Self::Create,
        Self::Link,
        Self::Unlink,
        Self::Rename,
        Self::Move,
        Self::WriteVersion,
    ];
}

/// The complete source of pseudo-randomness for one simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seed(u64);

impl Seed {
    /// Construct a seed from its portable integer representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the portable integer representation.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Inputs that may change a generated schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationConfig {
    actor_count: u16,
    steps: usize,
    overlap_per_256: u16,
}

impl SimulationConfig {
    /// Validate a simulation configuration.
    ///
    /// # Errors
    ///
    /// Returns [`SimulationConfigError`] when fewer than two actors are available, the schedule
    /// cannot hold the standing smoke prefix, or the overlap probability is outside `0..=256`.
    pub const fn new(
        actor_count: u16,
        steps: usize,
        overlap_per_256: u16,
    ) -> Result<Self, SimulationConfigError> {
        if actor_count < 2 {
            return Err(SimulationConfigError::TooFewActors);
        }
        if steps < MIN_SMOKE_STEPS {
            return Err(SimulationConfigError::TooFewSteps);
        }
        if overlap_per_256 > 256 {
            return Err(SimulationConfigError::InvalidOverlap);
        }
        Ok(Self {
            actor_count,
            steps,
            overlap_per_256,
        })
    }

    /// Number of independently ordered actors in the schedule.
    #[must_use]
    pub const fn actor_count(self) -> u16 {
        self.actor_count
    }

    /// Number of delivered changes, including duplicates.
    #[must_use]
    pub const fn steps(self) -> usize {
        self.steps
    }

    /// Probability that a generated change shares the preceding change's Lamport counter.
    #[must_use]
    pub const fn overlap_per_256(self) -> u16 {
        self.overlap_per_256
    }
}

/// Why a simulation configuration was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulationConfigError {
    /// The standing concurrent scenarios need at least two actors.
    TooFewActors,
    /// The schedule is too short to contain the standing smoke scenarios.
    TooFewSteps,
    /// A probability over 256/256 is not meaningful.
    InvalidOverlap,
}

impl fmt::Display for SimulationConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooFewActors => "a simulation has at least two actors",
            Self::TooFewSteps => "a smoke schedule has at least eight deliveries",
            Self::InvalidOverlap => "overlap_per_256 is at most 256",
        })
    }
}

impl std::error::Error for SimulationConfigError {}

/// Why a recorded minimal reproduction could not be reconstructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReproductionError {
    /// The supplied schedule is not the exact schedule the failure record captured.
    SourceMismatch,
    /// Delivery indexes are not strictly increasing and unique.
    NonCanonicalIndexes,
    /// A delivery index does not exist in the captured schedule.
    DeliveryOutOfRange,
}

impl fmt::Display for ReproductionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SourceMismatch => "the failure record belongs to the exact supplied schedule",
            Self::NonCanonicalIndexes => {
                "minimal reproduction indexes are strictly increasing and unique"
            }
            Self::DeliveryOutOfRange => {
                "every minimal reproduction index names a captured delivery"
            }
        })
    }
}

impl std::error::Error for ReproductionError {}

/// A planted defect at the boundary between a schedule and the real state fold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateMutant {
    /// A rename is acknowledged but never reaches the state register.
    DropRename,
    /// A durable content version is acknowledged but never reaches the state register.
    DropWriteVersion,
    /// Reapplying an exact delivery is reported as newly applied work.
    TreatDuplicateAsApplied,
}

impl StateMutant {
    /// Every planted defect in stable smoke-corpus order.
    pub const ALL: [Self; 3] = [
        Self::DropRename,
        Self::DropWriteVersion,
        Self::TreatDuplicateAsApplied,
    ];

    /// Stable word used in diagnostic campaign reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DropRename => "drop-rename",
            Self::DropWriteVersion => "drop-write-version",
            Self::TreatDuplicateAsApplied => "treat-duplicate-as-applied",
        }
    }

    pub(crate) const fn drops(self, change: &IdentityChange) -> bool {
        matches!(
            (self, change),
            (Self::DropRename, IdentityChange::Rename { .. })
                | (Self::DropWriteVersion, IdentityChange::WriteVersion { .. })
        )
    }
}

/// One delivery in a generated schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledChange {
    delivery: usize,
    actor: u16,
    stamp: Stamp,
    change: IdentityChange,
}

impl ScheduledChange {
    /// Position in delivery order. Unlike the stamp, this is not protocol order.
    #[must_use]
    pub const fn delivery(&self) -> usize {
        self.delivery
    }

    /// Actor that produced the change.
    #[must_use]
    pub const fn actor(&self) -> u16 {
        self.actor
    }

    /// Stable protocol position of the change.
    #[must_use]
    pub const fn stamp(&self) -> Stamp {
        self.stamp
    }

    /// The real core-model change this delivery applies.
    #[must_use]
    pub const fn change(&self) -> &IdentityChange {
        &self.change
    }

    /// Core operation family this delivery exercises.
    #[must_use]
    pub const fn family(&self) -> OperationFamily {
        match self.change {
            IdentityChange::Create { .. } => OperationFamily::Create,
            IdentityChange::Link { .. } => OperationFamily::Link,
            IdentityChange::Unlink { .. } => OperationFamily::Unlink,
            IdentityChange::Rename { .. } => OperationFamily::Rename,
            IdentityChange::Move { .. } => OperationFamily::Move,
            IdentityChange::WriteVersion { .. } => OperationFamily::WriteVersion,
        }
    }
}

/// A deterministic sequence of changes and the configuration that generated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    seed: Seed,
    config: SimulationConfig,
    changes: Vec<ScheduledChange>,
}

impl Schedule {
    /// Generate a schedule. This function reads no ambient input.
    #[must_use]
    pub fn generate(seed: Seed, config: SimulationConfig) -> Self {
        let mut random = SplitMix64(seed.value());
        let mut changes = smoke_prefix();
        let mut lamport = FIRST_SIMULATION_LAMPORT + 4;

        while changes.len() < config.steps() {
            let delivery = changes.len();
            let actor = (random.next() % u64::from(config.actor_count())) as u16;
            if random.next() & 0xff >= u64::from(config.overlap_per_256()) {
                lamport = lamport.saturating_add(1);
            }
            let object_number = 1 + (random.next() % u64::from(OBJECT_COUNT)) as u8;
            let target = object(object_number);
            let generated = delivery - MIN_SMOKE_STEPS;
            let choice = if generated < OperationFamily::ALL.len() {
                generated as u64
            } else {
                random.next() % OperationFamily::ALL.len() as u64
            };
            let change = match choice {
                0 => IdentityChange::Create {
                    object: generated_object(delivery),
                    kind: if random.next() & 1 == 0 {
                        ObjectKind::File
                    } else {
                        ObjectKind::Directory
                    },
                },
                1 => IdentityChange::Link {
                    object: target,
                    directory: object(0),
                    name: name(&format!("linked-{delivery}-{object_number}.txt")),
                },
                2 => IdentityChange::Unlink {
                    object: target,
                    directory: object(0),
                    name: name(&format!("file-{object_number}.txt")),
                },
                3 => IdentityChange::Rename {
                    object: target,
                    directory: object(0),
                    from_name: name(&format!("file-{object_number}.txt")),
                    to_name: name(&format!("renamed-{delivery}-{object_number}.txt")),
                },
                4 => IdentityChange::Move {
                    object: target,
                    from_directory: object(0),
                    from_name: name(&format!("file-{object_number}.txt")),
                    to_directory: object(
                        5 + (random.next() % u64::from(SUPPORT_DIRECTORY_COUNT)) as u8,
                    ),
                    to_name: name(&format!("moved-{delivery}-{object_number}.txt")),
                },
                _ => IdentityChange::WriteVersion {
                    object: target,
                    version: version(random.next()),
                },
            };
            changes.push(scheduled(delivery, actor, lamport, delivery as u64, change));
        }

        Self {
            seed,
            config,
            changes,
        }
    }

    /// The seed that completely determines this schedule.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// The validated generation configuration.
    #[must_use]
    pub const fn config(&self) -> SimulationConfig {
        self.config
    }

    /// Deliveries, in delivery order.
    #[must_use]
    pub fn changes(&self) -> &[ScheduledChange] {
        &self.changes
    }

    /// Select deliveries by their positions in the captured schedule.
    ///
    /// # Errors
    ///
    /// Returns [`ReproductionError`] unless indexes are strictly increasing, unique and in range.
    pub fn reproduction(&self, deliveries: &[usize]) -> Result<Self, ReproductionError> {
        if deliveries.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ReproductionError::NonCanonicalIndexes);
        }
        let mut changes = Vec::with_capacity(deliveries.len());
        for delivery in deliveries {
            let Some(change) = self.changes.get(*delivery) else {
                return Err(ReproductionError::DeliveryOutOfRange);
            };
            changes.push(change.clone());
        }
        Ok(Self {
            seed: self.seed,
            config: self.config,
            changes,
        })
    }

    pub(crate) fn in_delivery_order(&self, deliveries: &[usize]) -> Option<Self> {
        let changes: Option<Vec<_>> = deliveries
            .iter()
            .map(|delivery| self.changes.get(*delivery).cloned())
            .collect();
        Some(Self {
            seed: self.seed,
            config: self.config,
            changes: changes?,
        })
    }

    /// Stable bytes suitable for a failure record or cross-machine comparison.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut text = format!(
            "{SIMULATOR_PROTOCOL_VERSION}\nseed={}\nactors={}\nsteps={}\noverlap_per_256={}\n",
            self.seed.value(),
            self.config.actor_count(),
            self.config.steps(),
            self.config.overlap_per_256()
        );
        for scheduled in &self.changes {
            render_change(&mut text, scheduled);
        }
        text.into_bytes()
    }

    /// Apply the schedule to the real [`ObjectRegister`].
    #[must_use]
    pub fn run(&self) -> SimulationResult {
        self.run_inner(None)
    }

    /// Apply one planted state/materialization defect to this schedule.
    #[must_use]
    pub fn run_mutated(&self, mutant: StateMutant) -> SimulationResult {
        self.run_inner(Some(mutant))
    }

    /// Check every standing invariant after every ordinary delivery.
    #[must_use]
    pub fn audit(&self) -> crate::InvariantReport {
        crate::invariant::audit(self, None)
    }

    /// Check the invariants with one planted defect active.
    #[must_use]
    pub fn audit_mutated(&self, mutant: StateMutant) -> crate::InvariantReport {
        crate::invariant::audit(self, Some(mutant))
    }

    fn run_inner(&self, mutant: Option<StateMutant>) -> SimulationResult {
        let mut register = initial_register();
        let mut outcomes = Vec::with_capacity(self.changes.len());
        for scheduled in &self.changes {
            let dropped = mutant.is_some_and(|mutant| mutant.drops(scheduled.change()));
            if dropped {
                outcomes.push("mutant-dropped");
                continue;
            }
            let (next, outcome) = register.apply(scheduled.change(), scheduled.stamp());
            register = next;
            let outcome = if mutant == Some(StateMutant::TreatDuplicateAsApplied)
                && outcome == IdentityOutcome::AlreadyApplied
            {
                "applied"
            } else {
                outcome_name(&outcome)
            };
            outcomes.push(outcome);
        }
        let mut state = String::new();
        for object in register.object_ids() {
            let path = register
                .path_of(object)
                .map_or_else(|| "detached".to_owned(), |path| path.to_string());
            let version = register
                .version_of(object)
                .map_or_else(|| "none".to_owned(), |version| version.to_hex());
            state.push_str(&format!("{object}|{path}|{version}\n"));
        }
        SimulationResult {
            outcomes,
            state: state.into_bytes(),
        }
    }
}

/// Stable outcome of applying one [`Schedule`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationResult {
    outcomes: Vec<&'static str>,
    state: Vec<u8>,
}

impl SimulationResult {
    /// Stable bytes containing every delivery outcome and the resulting object state.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (delivery, outcome) in self.outcomes.iter().enumerate() {
            bytes.extend_from_slice(format!("{delivery}:{outcome}\n").as_bytes());
        }
        bytes.extend_from_slice(b"state\n");
        bytes.extend_from_slice(&self.state);
        bytes
    }

    pub(crate) fn state(&self) -> &[u8] {
        &self.state
    }
}

/// The five fields needed to replay and minimize one simulator failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureRecord {
    seed: Seed,
    schedule: Vec<u8>,
    protocol_version: &'static str,
    configuration: SimulationConfig,
    minimal_reproduction: Vec<usize>,
}

impl FailureRecord {
    /// Capture a failing schedule and the delivery indexes that remain after minimization.
    #[must_use]
    pub fn capture(schedule: &Schedule, minimal_reproduction: Vec<usize>) -> Self {
        Self {
            seed: schedule.seed(),
            schedule: schedule.canonical_bytes(),
            protocol_version: SIMULATOR_PROTOCOL_VERSION,
            configuration: schedule.config(),
            minimal_reproduction,
        }
    }

    /// The recorded seed.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// The exact generated schedule bytes.
    #[must_use]
    pub fn schedule(&self) -> &[u8] {
        &self.schedule
    }

    /// Simulator protocol version used for the run.
    #[must_use]
    pub const fn protocol_version(&self) -> &'static str {
        self.protocol_version
    }

    /// Generation configuration.
    #[must_use]
    pub const fn configuration(&self) -> SimulationConfig {
        self.configuration
    }

    /// Delivery indexes left by the minimizer.
    #[must_use]
    pub fn minimal_reproduction(&self) -> &[usize] {
        &self.minimal_reproduction
    }

    /// Reconstruct the recorded one-minimal schedule from its exact source schedule.
    ///
    /// # Errors
    ///
    /// Returns [`ReproductionError`] if the source bytes, seed, configuration or protocol version
    /// differ, or if the stored indexes are not a canonical selection from that source.
    pub fn reproduction(&self, source: &Schedule) -> Result<Schedule, ReproductionError> {
        if self.protocol_version != SIMULATOR_PROTOCOL_VERSION
            || self.seed != source.seed()
            || self.configuration != source.config()
            || self.schedule != source.canonical_bytes()
        {
            return Err(ReproductionError::SourceMismatch);
        }
        source.reproduction(&self.minimal_reproduction)
    }
}

fn smoke_prefix() -> Vec<ScheduledChange> {
    let root = object(0);
    let first = object(1);
    let second = object(2);
    let third = object(3);
    let fourth = object(4);
    let write = IdentityChange::WriteVersion {
        object: first,
        version: version(1),
    };
    vec![
        scheduled(
            0,
            0,
            FIRST_SIMULATION_LAMPORT,
            0,
            IdentityChange::Rename {
                object: first,
                directory: root,
                from_name: name("file-1.txt"),
                to_name: name("renamed.txt"),
            },
        ),
        scheduled(1, 1, FIRST_SIMULATION_LAMPORT, 1, write.clone()),
        scheduled(
            2,
            0,
            FIRST_SIMULATION_LAMPORT + 1,
            2,
            IdentityChange::Unlink {
                object: second,
                directory: root,
                name: name("file-2.txt"),
            },
        ),
        scheduled(
            3,
            1,
            FIRST_SIMULATION_LAMPORT + 1,
            3,
            IdentityChange::WriteVersion {
                object: second,
                version: version(2),
            },
        ),
        scheduled(
            4,
            0,
            FIRST_SIMULATION_LAMPORT + 2,
            4,
            IdentityChange::Link {
                object: third,
                directory: root,
                name: name("same-name.txt"),
            },
        ),
        scheduled(
            5,
            1,
            FIRST_SIMULATION_LAMPORT + 2,
            5,
            IdentityChange::Link {
                object: fourth,
                directory: root,
                name: name("same-name.txt"),
            },
        ),
        // An exact repeat of delivery 1: same operation and the same protocol stamp.
        scheduled(6, 1, FIRST_SIMULATION_LAMPORT, 1, write),
        // A lower protocol position delivered last: delivery order is not protocol order.
        scheduled(
            7,
            0,
            FIRST_SIMULATION_LAMPORT + 1,
            7,
            IdentityChange::WriteVersion {
                object: third,
                version: version(3),
            },
        ),
    ]
}

pub(crate) fn initial_register() -> ObjectRegister {
    let root = object(0);
    let mut register = ObjectRegister::new(root, stamp(0, 0, 0));
    for number in 1..=OBJECT_COUNT {
        for (change, at) in [
            (
                IdentityChange::Create {
                    object: object(number),
                    kind: ObjectKind::File,
                },
                u64::from(number) * 2 - 1,
            ),
            (
                IdentityChange::Link {
                    object: object(number),
                    directory: root,
                    name: name(&format!("file-{number}.txt")),
                },
                u64::from(number) * 2,
            ),
        ] {
            let (next, outcome) = register.apply(&change, stamp(at, at, at));
            debug_assert!(matches!(outcome, IdentityOutcome::Applied { .. }));
            register = next;
        }
    }
    for offset in 0..SUPPORT_DIRECTORY_COUNT {
        let number = 5 + offset;
        for (change, at) in [
            (
                IdentityChange::Create {
                    object: object(number),
                    kind: ObjectKind::Directory,
                },
                u64::from(number) * 2 - 1,
            ),
            (
                IdentityChange::Link {
                    object: object(number),
                    directory: root,
                    name: name(&format!("dir-{number}")),
                },
                u64::from(number) * 2,
            ),
        ] {
            let (next, outcome) = register.apply(&change, stamp(at, at, at));
            debug_assert!(matches!(outcome, IdentityOutcome::Applied { .. }));
            register = next;
        }
    }
    register
}

fn scheduled(
    delivery: usize,
    actor: u16,
    lamport: u64,
    identity: u64,
    change: IdentityChange,
) -> ScheduledChange {
    ScheduledChange {
        delivery,
        actor,
        stamp: stamp(lamport, identity, identity ^ u64::from(actor)),
        change,
    }
}

fn stamp(lamport: u64, event: u64, content: u64) -> Stamp {
    let mut event_bytes = [0u8; 16];
    event_bytes[8..].copy_from_slice(&event.to_be_bytes());
    let mut content_bytes = [0u8; 32];
    for (offset, chunk) in content_bytes.chunks_exact_mut(8).enumerate() {
        chunk.copy_from_slice(&content.rotate_left((offset * 13) as u32).to_be_bytes());
    }
    Stamp::new(
        Lamport::new(lamport),
        EventId::from_bytes(event_bytes),
        content_bytes,
    )
}

fn object(number: u8) -> ObjectId {
    ObjectId::from_bytes([number; 16])
}

fn generated_object(delivery: usize) -> ObjectId {
    let value = (delivery as u128).wrapping_add(0x100);
    ObjectId::from_bytes(value.to_be_bytes())
}

fn version(value: u64) -> VersionId {
    let mut bytes = [0u8; 32];
    for (offset, chunk) in bytes.chunks_exact_mut(8).enumerate() {
        chunk.copy_from_slice(&value.rotate_left((offset * 11) as u32).to_be_bytes());
    }
    VersionId::from_bytes(bytes)
}

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("the simulator emits portable names")
}

fn outcome_name(outcome: &IdentityOutcome) -> &'static str {
    match outcome {
        IdentityOutcome::Applied { .. } => "applied",
        IdentityOutcome::Superseded { .. } => "superseded",
        IdentityOutcome::AlreadyApplied => "already-applied",
        IdentityOutcome::Refused(_) => "refused",
    }
}

fn render_change(text: &mut String, scheduled: &ScheduledChange) {
    let stamp = scheduled.stamp();
    let prefix = format!(
        "delivery={}|actor={}|lamport={}|event={:?}|content=",
        scheduled.delivery(),
        scheduled.actor(),
        stamp.lamport().value(),
        stamp.event()
    );
    text.push_str(&prefix);
    for byte in stamp.content() {
        text.push_str(&format!("{byte:02x}"));
    }
    text.push('|');
    match scheduled.change() {
        IdentityChange::Create { object, kind } => {
            text.push_str(&format!("create|{object}|{}", kind.as_str()));
        }
        IdentityChange::Link {
            object,
            directory,
            name,
        } => text.push_str(&format!("link|{object}|{directory}|{name}")),
        IdentityChange::Unlink {
            object,
            directory,
            name,
        } => text.push_str(&format!("unlink|{object}|{directory}|{name}")),
        IdentityChange::Rename {
            object,
            directory,
            from_name,
            to_name,
        } => text.push_str(&format!(
            "rename|{object}|{directory}|{from_name}|{to_name}"
        )),
        IdentityChange::Move {
            object,
            from_directory,
            from_name,
            to_directory,
            to_name,
        } => text.push_str(&format!(
            "move|{object}|{from_directory}|{from_name}|{to_directory}|{to_name}"
        )),
        IdentityChange::WriteVersion { object, version } => {
            text.push_str(&format!("write-version|{object}|{version}"));
        }
    }
    text.push('\n');
}

/// SplitMix64 has a complete integer specification and therefore reproduces on every Rust target.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}
