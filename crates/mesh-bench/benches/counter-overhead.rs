//! The counter-overhead bench: what collection costs, measured rather than assumed.
//!
//! ```text
//! cargo bench -p mesh-bench --bench counter-overhead
//! cargo bench -p mesh-bench --bench counter-overhead -- --observations 4096
//! ```
//!
//! # Why this bench exists
//!
//! `## Acceptance criteria` of task `01KZC2SQABSC6CT5Z4JCKHHQKQ` says collection overhead is
//! **measured and stated, not assumed negligible**, and plan §2.10 refuses a performance claim with
//! no number behind it. "Five atomic adds, so it is basically free" is a claim with no number.
//!
//! # Two arms, because an absolute number here would answer nothing
//!
//! An iteration records `--observations` observations. It is run twice:
//!
//! - **instrumented** — through `mesh_daemon::counters::Counters::record`;
//! - **bare** — the identical loop over the identical seeded sequence, with the record call
//!   replaced by `std::hint::black_box` over the same two arguments.
//!
//! The bare arm is the cost of *reaching* the observations: the loop, the index arithmetic, the
//! sequence read. Subtracting it leaves the cost of the counter plane itself. Publishing only the
//! instrumented arm would attribute the loop to the counters and overstate them, on a number this
//! program would then quote at itself.
//!
//! # What this bench asserts, and what it only reports
//!
//! It **asserts** the deterministic facts: zero allocations per observation, five atomic writes,
//! the exact byte size of the counter state, and — after the run — that the registry's own tally
//! equals the number of observations the workload knows it performed. Every one of those holds on
//! any machine under any load.
//!
//! It **reports** the wall-clock numbers: p50, p95, p99, the sample count, and the derived
//! per-observation cost, each printed with the conditions of
//! `benchmarks/runners/README.md`. It asserts **no** duration budget. Two wall-clock budget
//! assertions on this repository's merge path have already failed under machine load, and a gate
//! that fails on load teaches its readers to re-run rather than read. A timing number that moves
//! with the machine is reported with its conditions or it is not reported at all.
//!
//! # Correctness before timing
//!
//! Both arms verify before the clock is read: the workload replays its seeded sequence and compares
//! a digest of what the counters hold against a digest computed independently from the sequence
//! itself. The harness refuses to time a workload that has not verified, so a counter that adds
//! wrongly produces no timing number at all.
//!
//! `harness = false`: the bench owns its `main`, so `--observations` reaches this code rather than
//! libtest, and no benchmark framework sits between the clock and the workload.

use mesh_bench::clock::{MonotonicClock, SystemWallClock};
use mesh_bench::corpus::rng::SplitMix64;
use mesh_bench::env::SystemProbe;
use mesh_bench::harness::{Harness, RunConfig, RunOutcome};
use mesh_bench::json::{Json, JsonObject};
use mesh_bench::schema::{BenchmarkResult, CacheState, Verification, WorkloadDescriptor};
use mesh_bench::workload::{Workload, WorkloadError};
use mesh_daemon::counters::{
    counter_count, CounterId, Counters, ALLOCATIONS_PER_OBSERVATION, ATOMIC_WRITES_PER_OBSERVATION,
    CONDITIONS,
};

/// How many observations one iteration records by default.
///
/// Large enough that one iteration is far above the clock's own resolution, small enough that the
/// whole run stays under a second on a laptop.
const DEFAULT_OBSERVATIONS: u64 = 2_048;

/// The seed the observation sequence is generated from.
///
/// Fixed and committed: a third party re-running this bench drives the identical sequence of
/// counters and values, which is what makes the two arms comparable to each other and to a run on
/// another machine. `SplitMix64` lives in `crates/mesh-bench/src/corpus/rng.rs`.
const SEED: u64 = 42;

/// Timed iterations. Above the twenty the publishing policy demands, and enough that p99 is not one
/// sample wearing a percentile's name.
const ITERATIONS: u64 = 200;

/// Untimed iterations run before the clock starts.
const WARMUP: u64 = 20;

fn main() {
    // Cargo passes `--bench` to benchmark targets; anything else is ours.
    let arguments: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| argument != "--bench")
        .collect();
    let observations = match parse_observations(&arguments) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("counter-overhead: {message}");
            std::process::exit(2);
        }
    };

    if let Err(error) = run(observations) {
        eprintln!("counter-overhead: {error}");
        std::process::exit(1);
    }
}

/// `--observations N`, or the default.
fn parse_observations(arguments: &[String]) -> Result<u64, String> {
    let mut observations = DEFAULT_OBSERVATIONS;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--observations" {
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| "--observations needs a number".to_owned())?;
            observations = value
                .parse()
                .map_err(|_| format!("--observations {value:?} is not a number"))?;
            if observations == 0 {
                return Err("--observations 0 measures nothing".to_owned());
            }
            index += 2;
            continue;
        }
        return Err(format!("unrecognised argument {:?}", arguments[index]));
    }
    Ok(observations)
}

/// The deterministic checks, then both measured arms, then the report.
fn run(observations: u64) -> Result<(), String> {
    state_is_the_size_it_says()?;

    let probe = SystemProbe::here().map_err(|error| error.to_string())?;
    let harness = Harness::new(MonotonicClock::new(), SystemWallClock, probe);

    let instrumented = measure(&harness, Arm::Instrumented, observations)?;
    let bare = measure(&harness, Arm::Bare, observations)?;

    println!("{}", instrumented.to_json_pretty());
    report(&instrumented, &bare, observations);
    Ok(())
}

/// The half of the overhead that has no clock in it, checked before anything is timed.
///
/// A failure here is a change to the counter plane's cost that no timing run would have made
/// obvious, so it stops the bench rather than being printed next to the numbers.
fn state_is_the_size_it_says() -> Result<(), String> {
    let overhead = Counters::new().overhead();
    if overhead.allocations_per_observation != ALLOCATIONS_PER_OBSERVATION {
        return Err(format!(
            "an observation now performs {} allocations; an allocation inside an observation \
             perturbs the thing being measured",
            overhead.allocations_per_observation
        ));
    }
    if overhead.atomic_writes_per_observation != ATOMIC_WRITES_PER_OBSERVATION {
        return Err(format!(
            "an observation now performs {} atomic writes rather than {ATOMIC_WRITES_PER_OBSERVATION}",
            overhead.atomic_writes_per_observation
        ));
    }
    let expected_bytes = core::mem::size_of::<Counters>() as u64
        + counter_count() as u64 * 2 * core::mem::size_of::<std::sync::atomic::AtomicU64>() as u64;
    if overhead.state_bytes != expected_bytes {
        return Err(format!(
            "the counter state is {} bytes and the layout says {expected_bytes}",
            overhead.state_bytes
        ));
    }
    println!(
        "counter-overhead: deterministic cost — {} counters, {} bytes of state, {} atomic writes \
         and {} allocations per observation",
        overhead.counters,
        overhead.state_bytes,
        overhead.atomic_writes_per_observation,
        overhead.allocations_per_observation
    );
    Ok(())
}

/// One measured arm.
fn measure(
    harness: &Harness<MonotonicClock, SystemWallClock, SystemProbe>,
    arm: Arm,
    observations: u64,
) -> Result<BenchmarkResult, String> {
    let config = RunConfig::new(
        arm.benchmark_id(),
        "cargo bench -p mesh-bench --bench counter-overhead",
    )
    // Warm: the observation sequence and the counter state are both in memory before timing
    // starts. There is no cold form of this measurement — a counter plane with a cold cache is a
    // measurement of the machine's cache and not of the counter plane.
    .with_cache_state(CacheState::Warm)
    .with_iterations(ITERATIONS)
    .with_warmup_iterations(WARMUP);

    let mut workload = ObservationWorkload::new(arm, observations);
    match harness
        .run(&mut workload, &config)
        .map_err(|error| error.to_string())?
    {
        RunOutcome::Measured(result) => {
            workload.check_tally()?;
            Ok(*result)
        }
        RunOutcome::VerificationFailed(failure) => Err(failure.to_string()),
    }
}

/// Print both arms, their difference, and the conditions the difference is true under.
fn report(instrumented: &BenchmarkResult, bare: &BenchmarkResult, observations: u64) {
    println!("counter-overhead: {CONDITIONS}");
    println!(
        "counter-overhead: {} observations per iteration, {} samples per arm, {} failed iterations",
        observations,
        instrumented.sample_count,
        instrumented.failure_count + bare.failure_count
    );
    for (name, result) in [("instrumented", instrumented), ("bare", bare)] {
        println!(
            "counter-overhead: {name:<12} p50 {} ns · p95 {} ns · p99 {} ns (n={})",
            result.latency.p50_ns,
            result.latency.p95_ns,
            result.latency.p99_ns,
            result.sample_count
        );
    }
    for (label, instrumented_ns, bare_ns) in [
        ("p50", instrumented.latency.p50_ns, bare.latency.p50_ns),
        ("p95", instrumented.latency.p95_ns, bare.latency.p95_ns),
        ("p99", instrumented.latency.p99_ns, bare.latency.p99_ns),
    ] {
        match instrumented_ns.checked_sub(bare_ns) {
            Some(difference) => println!(
                "counter-overhead: {label} cost of collection {} ps per observation \
                 ({difference} ns over {observations} observations)",
                difference.saturating_mul(1_000) / observations
            ),
            // Reported rather than clamped to zero. The two arms are separate runs, so noise can
            // put the bare arm above the instrumented one, and a negative difference printed as
            // zero is a measurement quietly rounded in the direction that flatters the counters.
            None => println!(
                "counter-overhead: {label} the bare arm came out slower than the instrumented one \
                 ({bare_ns} ns against {instrumented_ns} ns), so this machine is too noisy to \
                 separate them at {label} — see the conditions above"
            ),
        }
    }
}

/// Which arm a workload is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    /// Every observation goes through the counter plane.
    Instrumented,
    /// Every observation is discarded, so the loop is what is left.
    Bare,
}

impl Arm {
    /// The identifier this arm's row carries.
    const fn benchmark_id(self) -> &'static str {
        match self {
            Self::Instrumented => "mesh-daemon/counters/record-instrumented",
            Self::Bare => "mesh-daemon/counters/record-bare",
        }
    }

    /// The word the row's parameters carry.
    const fn word(self) -> &'static str {
        match self {
            Self::Instrumented => "instrumented",
            Self::Bare => "bare",
        }
    }
}

/// A fixed sequence of `(counter, value)` pairs, replayed every iteration.
struct ObservationWorkload {
    arm: Arm,
    /// `(catalogue index, value)`, generated once from [`SEED`] and never regenerated.
    sequence: Vec<(usize, u64)>,
    counters: Counters,
    /// Observations this workload knows it performed, counted by the workload and not by the
    /// registry. The registry's own tally is checked against this after the run.
    performed: u64,
    /// A sink the bare arm accumulates into, so the loop cannot be optimised away.
    sink: u64,
}

impl ObservationWorkload {
    /// A workload over `observations` pairs drawn from the committed generator.
    fn new(arm: Arm, observations: u64) -> Self {
        let mut rng = SplitMix64::derived(SEED, "counter-overhead", 0);
        let width = counter_count() as u64;
        let sequence = (0..observations)
            .map(|_| {
                let index = rng.in_range(0, width - 1) as usize;
                let value = rng.in_range(0, 4_096);
                (index, value)
            })
            .collect();
        Self {
            arm,
            sequence,
            counters: Counters::new(),
            performed: 0,
            sink: 0,
        }
    }

    /// Put the workload back where it started.
    fn reset(&mut self) {
        self.counters = Counters::new();
        self.performed = 0;
        self.sink = 0;
    }

    /// One pass over the sequence — the only thing that is ever timed.
    fn replay(&mut self) {
        match self.arm {
            Arm::Instrumented => {
                for (index, value) in &self.sequence {
                    let id =
                        CounterId::at(*index).expect("the sequence is built from the catalogue");
                    self.counters.record(id, *value);
                }
            }
            Arm::Bare => {
                for (index, value) in &self.sequence {
                    // The same two arguments reach the same place; only the counter plane is gone.
                    // `black_box` is what stops the optimiser from deleting a loop with no effect
                    // and handing back a measurement of nothing.
                    self.sink = self
                        .sink
                        .wrapping_add(std::hint::black_box(*index as u64 + *value));
                }
            }
        }
        self.performed += self.sequence.len() as u64;
    }

    /// What the counters hold after one replay, as a digest independent of the counters themselves.
    ///
    /// The expectation is folded from the sequence with a different traversal — grouped by counter
    /// rather than in observation order — so an indexing mistake in `record` cannot be reproduced
    /// by the check that is supposed to catch it.
    fn expected_digest(&self) -> String {
        let mut observations = vec![0_u64; counter_count()];
        let mut totals = vec![0_u64; counter_count()];
        for (index, value) in &self.sequence {
            observations[*index] += 1;
            totals[*index] = totals[*index].saturating_add(*value);
        }
        digest_of(&observations, &totals)
    }

    /// What one replay of the sequence has to leave behind, computed without replaying it.
    ///
    /// The bare arm's expectation is folded straight from the sequence: if the optimiser deletes
    /// the loop the sink stays at zero, the two sides disagree, and the harness refuses to time a
    /// workload that has not verified. A bench that measures a deleted loop is the failure this
    /// guards against.
    fn expected_verification(&self) -> String {
        match self.arm {
            Arm::Instrumented => self.expected_digest(),
            Arm::Bare => {
                let sum = self.sequence.iter().fold(0_u64, |total, (index, value)| {
                    total.wrapping_add(*index as u64 + *value)
                });
                format!("sink:{sum}")
            }
        }
    }

    /// What one replay of the sequence actually left behind.
    fn observed_verification(&self) -> String {
        match self.arm {
            Arm::Instrumented => self.observed_digest(),
            Arm::Bare => format!("sink:{}", self.sink),
        }
    }

    /// How correctness was established, in the row's own words.
    const fn verification_method(self_arm: Arm) -> &'static str {
        match self_arm {
            Arm::Instrumented => {
                "counter totals after one replay of the seeded observation sequence"
            }
            Arm::Bare => "loop accumulator after one replay of the seeded observation sequence",
        }
    }

    /// What the counters actually hold.
    fn observed_digest(&self) -> String {
        let observations: Vec<u64> = (0..counter_count())
            .map(|index| {
                self.counters
                    .observations_of(CounterId::at(index).expect("in range"))
            })
            .collect();
        let totals: Vec<u64> = (0..counter_count())
            .map(|index| {
                self.counters
                    .total_of(CounterId::at(index).expect("in range"))
            })
            .collect();
        digest_of(&observations, &totals)
    }

    /// After the run: the registry's independent tally must equal what this workload performed.
    fn check_tally(&self) -> Result<(), String> {
        if self.arm == Arm::Bare {
            return Ok(());
        }
        let collection = self.counters.snapshot().collection;
        if !collection.snapshot_consistent {
            return Err(format!(
                "the final counter snapshot overlapped {} writer(s) before and {} after its \
                 reading pass, so no timing from this run is usable",
                collection.writers_in_flight_before_readings,
                collection.writers_in_flight_after_readings
            ));
        }
        let recorded = collection.observations_recorded;
        if recorded == self.performed {
            return Ok(());
        }
        Err(format!(
            "the registry tallied {recorded} observations and the workload performed \
             {}: collection lost or invented observations, so no timing from this run is usable",
            self.performed
        ))
    }
}

/// A stable text digest of two parallel counter arrays.
///
/// Deliberately not a cryptographic hash: this is a benchmark's own correctness gate and it needs
/// to be readable when it fails. It is the pairs that differ from zero, in catalogue order.
fn digest_of(observations: &[u64], totals: &[u64]) -> String {
    let mut rendered = String::new();
    for (index, (count, total)) in observations.iter().zip(totals.iter()).enumerate() {
        if *count != 0 {
            rendered.push_str(&format!("{index}:{count}:{total};"));
        }
    }
    format!("{}#{}", rendered.len(), rendered)
}

impl Workload for ObservationWorkload {
    fn descriptor(&self) -> WorkloadDescriptor {
        WorkloadDescriptor {
            generator: "mesh-daemon/counter-observations".to_owned(),
            generator_version: "1".to_owned(),
            seed: SEED,
            parameters: JsonObject::new()
                .with("arm", Json::string(self.arm.word()))
                .with("observations", Json::Uint(self.sequence.len() as u64))
                .with("counters", Json::Uint(counter_count() as u64))
                .with(
                    "atomic_writes_per_observation",
                    Json::Uint(ATOMIC_WRITES_PER_OBSERVATION),
                )
                .with("rng", Json::string("mesh-bench/corpus/rng SplitMix64"))
                .with("timed_phases", Json::string("record-each-observation")),
        }
    }

    fn verify(&mut self) -> Result<Verification, WorkloadError> {
        // A fresh registry and a fresh sink, so verification measures one replay and not whatever
        // a previous call left behind.
        self.reset();
        let expected = self.expected_verification();
        self.replay();
        let observed = self.observed_verification();
        let verification =
            Verification::new(Self::verification_method(self.arm), expected, observed);
        self.reset();
        Ok(verification)
    }

    fn prepare(&mut self, _cache_state: CacheState) -> Result<(), WorkloadError> {
        // Nothing to warm and nothing to drop: the sequence is already in memory and the counter
        // state is one small array that stays hot for the whole run by construction.
        Ok(())
    }

    fn iterate(&mut self) -> Result<(), WorkloadError> {
        self.replay();
        Ok(())
    }
}
