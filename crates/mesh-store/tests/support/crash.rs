#![allow(dead_code)]
//! Killing a real process, faithfully, from a test that is also the process being killed.
//!
//! # Why this is shared rather than copied
//!
//! `tests/crash-commit-sequence.rs` established this machinery for plan §6.3's eleven steps and
//! `tests/recovery.rs` needs exactly the same interruption to prove what a recovery finds
//! afterwards. Two copies would be two definitions of "a faithful crash", and the second one would
//! be the one that quietly stopped killing the database engine.
//!
//! # What "faithful" means here, and the part that is easy to get wrong
//!
//! An interruption modelled by returning `Err` tests the error path — destructors still run,
//! buffers still flush, and the cleanup a crash skips is exactly the cleanup that happens. So a
//! test spawns *this same test binary* as a child, drives it to a chosen point, and `SIGKILL`s it.
//! [`assert_killed`] is asserted every time, so a child that exited on its own can never be
//! mistaken for one that was killed.
//!
//! The test database engine is a **grandchild**: this harness sends every batch to a `sqlite3`
//! process the child spawns. Killing only the child leaves that process alive, orphaned
//! but still holding the write lock and still executing the SQL already in its pipe — which is not
//! a crash, and which produced a database a parent read while it was still being written.
//! Production runs the engine in the caller's own process, so the faithful interruption kills both,
//! and the grandchildren are collected *before* the child dies because an orphan is reparented and
//! can no longer be found from its original parent.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// What the child prints when it has reached its stop point.
///
/// Matched as a *substring*: libtest writes `test <name> ... ` with no trailing newline before the
/// body runs, so the child's first line of output is glued to it.
pub const READY_TOKEN: &str = "MESH-STORE-READY";

/// Which child to spawn and what to tell it.
#[derive(Clone, Debug)]
pub struct ChildSpec {
    test_name: String,
    environment: Vec<(String, String)>,
}

impl ChildSpec {
    /// A child that runs the libtest case with this exact name.
    pub fn new(test_name: &str) -> Self {
        Self {
            test_name: test_name.to_owned(),
            environment: Vec::new(),
        }
    }

    /// Set one environment variable for the child. Immutable in style: it consumes and returns.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.environment.push((key.to_owned(), value.into()));
        self
    }
}

/// Tell the parent this process has reached its stop point.
pub fn announce_ready() {
    println!("{READY_TOKEN}");
    std::io::stdout().flush().expect("stdout flushes");
}

/// Block until the parent kills this process, or give up so an orphan cannot outlive the suite.
pub fn wait_to_be_killed() -> ! {
    for _ in 0..600 {
        std::thread::sleep(Duration::from_millis(100));
    }
    std::process::exit(97);
}

/// Spawn the child, wait for [`READY_TOKEN`], kill it, and return how it died.
pub fn kill_child_at_ready(spec: &ChildSpec) -> ExitStatus {
    spawn_and_kill(spec, None).0
}

/// Spawn the child and kill it after `delay`, wherever it has got to.
pub fn kill_child_after(spec: &ChildSpec, delay: Duration) -> ExitStatus {
    spawn_and_kill(spec, Some(delay)).0
}

/// How long a child takes to reach its stop point on this machine, measured from spawn.
pub fn measure_span(spec: &ChildSpec) -> Duration {
    spawn_and_kill(spec, None).1
}

/// Spawn, wait or sleep, then kill the child and every engine process it started.
pub fn spawn_and_kill(spec: &ChildSpec, delay: Option<Duration>) -> (ExitStatus, Duration) {
    let started = Instant::now();
    let binary = std::env::current_exe().expect("the test binary knows its own path");
    let mut command = Command::new(binary);
    command
        .args([
            &spec.test_name,
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (key, value) in &spec.environment {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("the child test binary starts");

    let announced = match delay {
        Some(delay) => {
            std::thread::sleep(delay);
            delay
        }
        None => {
            let stdout = child.stdout.take().expect("the child's stdout is piped");
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                let read = reader
                    .read_line(&mut line)
                    .expect("the child's output is readable");
                if read == 0 {
                    let _ = child.kill();
                    let status = child.wait().expect("the child is reaped");
                    panic!(
                        "the child {} exited ({status:?}) before announcing readiness; its stderr \
                         is above",
                        spec.test_name
                    );
                }
                if line.contains(READY_TOKEN) {
                    break;
                }
            }
            started.elapsed()
        }
    };

    let engines = children_of(child.id());
    sigkill(child.id());
    for engine in engines {
        // Best effort: an engine that already exited is the common case at a step boundary.
        let _ = Command::new("/bin/kill")
            .args(["-9", &engine.to_string()])
            .output();
    }
    (child.wait().expect("the child is reaped"), announced)
}

/// The process identifiers of `parent`'s live children, read from `pgrep`.
pub fn children_of(parent: u32) -> Vec<u32> {
    let Ok(output) = Command::new("/usr/bin/pgrep")
        .args(["-P", &parent.to_string()])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Send `SIGKILL` through `/bin/kill`, which is how this is done without an `unsafe` block or a
/// dependency on `libc`. A failure to send is a failure of the test, never a silent skip.
pub fn sigkill(process: u32) {
    let output = Command::new("/bin/kill")
        .args(["-9", &process.to_string()])
        .output()
        .expect("/bin/kill runs; this suite cannot test a crash without it");
    assert!(
        output.status.success(),
        "/bin/kill -9 {process} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A child that exited on its own tested nothing about a crash, so this is asserted every time.
pub fn assert_killed(status: ExitStatus, context: &str) {
    assert!(
        status.code().is_none(),
        "the child at {context} exited with {status:?} instead of being killed by a signal, so \
         nothing about a crash was tested"
    );
}
