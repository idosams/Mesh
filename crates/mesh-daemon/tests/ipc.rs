//! The daemon half of the local IPC contract, exercised over a real Unix-domain socket.
//!
//! Run it exactly as the task contract names it:
//!
//! ```text
//! cargo nextest run -p mesh-daemon --test ipc
//! ```
//!
//! Four claims are checked here and each one is a claim somebody would otherwise have to take on
//! trust:
//!
//! 1. **The published contract and the code agree.** `crates/mesh-daemon/ipc-contract.json` is the
//!    one file both implementations read. If the Rust catalogue drifts from it, this goes red; if
//!    the TypeScript client drifts from it, `apps/desktop/src/ipc/contract.test.ts` goes red.
//! 2. **The encoding is byte-deterministic.** Every published vector is decoded, re-encoded, and
//!    compared byte for byte, so "one message has one encoding" is a measurement.
//! 3. **The transport is local only.** The crate's own sources are scanned for the network types,
//!    and a live connection's address is read back and asserted to be a filesystem path.
//! 4. **A daemon restart is survivable.** The socket file a killed daemon leaves behind does not
//!    stop the next one binding, and a client that reconnects is served.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;

use mesh_daemon::ipc::json::Json;
use mesh_daemon::ipc::message::{
    ClientMessage, DaemonMessage, CHUNK_DATA_BYTES, MAX_LINE_BYTES, MAX_MESSAGE_BYTES,
    MAX_METHOD_BYTES, MAX_SESSION_BYTES, PROTOCOL, SUPPORTED_VERSIONS, SURFACE_VERSION,
};
use mesh_daemon::ipc::server::{IpcServer, ServerHandle, MAX_CONNECTIONS};
use mesh_daemon::ipc::surface::{
    nothing_to_recover, Operations, RecoveredDaemon, StartupSummary, METHODS,
};
use mesh_daemon::ipc::CONTRACT_JSON;
use mesh_daemon::workspace::RecordFile;
use mesh_daemon::{
    counters::{
        counter_count, counters, Unit, ALLOCATIONS_PER_OBSERVATION, ATOMIC_WRITES_PER_GROUP,
        ATOMIC_WRITES_PER_OBSERVATION, CONDITIONS, INTEGER_ENCODING,
    },
    LiveDaemon, RecoveryDiagnostic, RecoveryOutcome, Severity, RECORD_FILE_NAME, RECOVERY_BUDGET,
};
use mesh_store::{
    frame_record, journal_records, no_session, OperationRecord, PeerRecord, RecordDigest,
    StoredRecord,
};

/// Three real records: two operations by one author and the peer they replicate with.
///
/// Written through `mesh_store::frame_record`, which is the same framing the daemon reads back, so
/// what this test measures is the daemon's path over real bytes rather than a fixture agreeing
/// with itself.
fn saved_records() -> Vec<StoredRecord> {
    vec![
        StoredRecord::Operation(OperationRecord {
            id: RecordDigest::from_bytes([1; 32]),
            actor: RecordDigest::from_bytes([2; 32]),
            actor_sequence: 1,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: RecordDigest::from_bytes([3; 32]),
            parents: Vec::new(),
        }),
        StoredRecord::Operation(OperationRecord {
            id: RecordDigest::from_bytes([4; 32]),
            actor: RecordDigest::from_bytes([2; 32]),
            actor_sequence: 2,
            hlc_millis: 1_700_000_000_001,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: RecordDigest::from_bytes([5; 32]),
            parents: vec![RecordDigest::from_bytes([1; 32])],
        }),
        StoredRecord::Peer(PeerRecord {
            peer: RecordDigest::from_bytes([30; 32]),
            joined_at: RecordDigest::from_bytes([1; 32]),
        }),
    ]
}

// --------------------------------------------------------------- the contract

fn contract() -> Json {
    Json::parse(CONTRACT_JSON).expect("the published contract is valid JSON")
}

fn text(value: &Json, key: &str) -> String {
    value
        .get(key)
        .and_then(Json::as_text)
        .unwrap_or_else(|| panic!("`{key}` is missing from the published contract"))
        .to_owned()
}

fn number(value: &Json, key: &str) -> u64 {
    value
        .get(key)
        .and_then(Json::as_u64)
        .unwrap_or_else(|| panic!("`{key}` is missing from the published contract"))
}

#[test]
fn the_catalogue_matches_the_published_contract() {
    let contract = contract();
    assert_eq!(text(&contract, "protocol"), PROTOCOL);
    assert_eq!(
        number(&contract, "surface_version"),
        u64::from(SURFACE_VERSION)
    );

    let supported: Vec<u32> = contract
        .get("supported_versions")
        .and_then(Json::as_array)
        .expect("supported_versions")
        .iter()
        .map(|entry| u32::try_from(entry.as_u64().expect("a version")).expect("fits"))
        .collect();
    assert_eq!(supported, SUPPORTED_VERSIONS);

    let methods = contract
        .get("methods")
        .and_then(Json::as_array)
        .expect("methods");
    assert_eq!(
        methods.len(),
        METHODS.len(),
        "the contract publishes {} methods, the crate has {}",
        methods.len(),
        METHODS.len()
    );
    for (published, ours) in methods.iter().zip(METHODS) {
        assert_eq!(text(published, "name"), ours.name);
        assert_eq!(number(published, "since"), u64::from(ours.since));
        assert_eq!(text(published, "summary"), ours.summary);
    }

    let counter_answer = contract
        .get("answers")
        .and_then(|answers| answers.get("performance.counters"))
        .expect("performance counter answer contract");
    assert_eq!(text(counter_answer, "conditions"), CONDITIONS);
    assert_eq!(text(counter_answer, "integer_encoding"), INTEGER_ENCODING);
    assert_eq!(
        number(counter_answer, "catalogue_count"),
        u64::try_from(counter_count()).expect("counter count fits")
    );
    let published_catalogue: Vec<&str> = counter_answer
        .get("catalogue")
        .and_then(Json::as_array)
        .expect("performance counter catalogue")
        .iter()
        .map(|entry| entry.as_text().expect("a counter key"))
        .collect();
    let mut live_catalogue: Vec<&str> = counters().map(|entry| entry.key).collect();
    live_catalogue.sort_unstable();
    assert_eq!(published_catalogue, live_catalogue);
    let unit_partitions = counter_answer
        .get("unit_partitions")
        .expect("performance counter unit partitions");
    let published_unit_keys = |unit: &str| -> Vec<&str> {
        unit_partitions
            .get(unit)
            .and_then(Json::as_array)
            .expect("a counter unit partition")
            .iter()
            .map(|entry| entry.as_text().expect("a counter key"))
            .collect()
    };
    let live_unit_keys = |unit: Unit| -> Vec<&str> {
        let mut keys: Vec<&str> = counters()
            .filter(|entry| entry.unit == unit)
            .map(|entry| entry.key)
            .collect();
        keys.sort_unstable();
        keys
    };
    assert_eq!(published_unit_keys("bytes"), live_unit_keys(Unit::Bytes));
    assert_eq!(
        published_unit_keys("nanoseconds"),
        live_unit_keys(Unit::Nanoseconds)
    );
    assert_eq!(
        unit_partitions
            .get("events_are_catalogue_remainder")
            .and_then(Json::as_bool),
        Some(true)
    );
    let partitioned: std::collections::BTreeSet<&str> = published_unit_keys("bytes")
        .into_iter()
        .chain(published_unit_keys("nanoseconds"))
        .collect();
    let published_events: Vec<&str> = published_catalogue
        .iter()
        .copied()
        .filter(|key| !partitioned.contains(key))
        .collect();
    assert_eq!(published_events, live_unit_keys(Unit::Events));

    let producer_partitions = counter_answer
        .get("producer_partitions")
        .expect("performance counter producer partitions");
    let published_wired: Vec<&str> = producer_partitions
        .get("wired")
        .and_then(Json::as_array)
        .expect("wired counter keys")
        .iter()
        .map(|entry| entry.as_text().expect("a wired counter key"))
        .collect();
    let mut live_wired: Vec<&str> = counters()
        .filter(|entry| entry.producer.is_wired())
        .map(|entry| entry.key)
        .collect();
    live_wired.sort_unstable();
    assert_eq!(published_wired, live_wired);

    let mut published_not_yet = std::collections::BTreeMap::new();
    for group in producer_partitions
        .get("not_yet_reason_groups")
        .and_then(Json::as_array)
        .expect("not-yet reason groups")
    {
        let reason = text(group, "reason");
        for key in group
            .get("keys")
            .and_then(Json::as_array)
            .expect("not-yet group keys")
        {
            let key = key.as_text().expect("a not-yet counter key");
            assert!(
                published_not_yet.insert(key, reason.clone()).is_none(),
                "counter {key} appears in two not-yet reason groups"
            );
        }
    }
    let live_not_yet: std::collections::BTreeMap<&str, String> = counters()
        .filter(|entry| !entry.producer.is_wired())
        .map(|entry| (entry.key, entry.producer.reason().to_owned()))
        .collect();
    assert_eq!(published_not_yet, live_not_yet);
    assert_eq!(
        number(counter_answer, "atomic_writes_per_observation"),
        ATOMIC_WRITES_PER_OBSERVATION
    );
    assert_eq!(
        number(counter_answer, "atomic_writes_per_group"),
        ATOMIC_WRITES_PER_GROUP
    );
    assert_eq!(
        number(counter_answer, "allocations_per_observation"),
        ALLOCATIONS_PER_OBSERVATION
    );
}

#[test]
fn the_published_limits_are_the_limits_the_code_enforces() {
    let contract = contract();
    let framing = contract.get("framing").expect("framing").clone();
    assert_eq!(text(&framing, "kind"), "newline-delimited-json");
    assert_eq!(
        number(&framing, "max_line_bytes"),
        u64::try_from(MAX_LINE_BYTES).expect("fits")
    );
    assert_eq!(
        number(&framing, "max_message_bytes"),
        u64::try_from(MAX_MESSAGE_BYTES).expect("fits")
    );
    assert_eq!(
        number(&framing, "chunk_data_bytes"),
        u64::try_from(CHUNK_DATA_BYTES).expect("fits")
    );
    assert_eq!(number(&framing, "chunk_since"), 7);

    let limits = contract.get("limits").expect("limits").clone();
    assert_eq!(
        number(&limits, "max_method_bytes"),
        u64::try_from(MAX_METHOD_BYTES).expect("fits")
    );
    assert_eq!(
        number(&limits, "max_session_bytes"),
        u64::try_from(MAX_SESSION_BYTES).expect("fits")
    );
    assert_eq!(
        number(&limits, "max_json_depth"),
        u64::try_from(mesh_daemon::ipc::json::MAX_DEPTH).expect("fits")
    );
    assert_eq!(
        number(&limits, "max_connections"),
        u64::try_from(MAX_CONNECTIONS).expect("fits")
    );

    let transport = contract.get("transport").expect("transport").clone();
    assert_eq!(text(&transport, "kind"), "unix-domain-socket");
    assert_eq!(
        transport.get("network_listener").and_then(Json::as_bool),
        Some(false)
    );
}

#[test]
fn every_published_vector_round_trips_byte_for_byte() {
    let contract = contract();
    let vectors = contract
        .get("vectors")
        .and_then(Json::as_array)
        .expect("vectors");
    assert!(vectors.len() >= 7, "the corpus lost entries");

    for vector in vectors {
        let name = text(vector, "name");
        let line = text(vector, "line");
        let direction = text(vector, "direction");
        let re_encoded = match direction.as_str() {
            "client-to-daemon" => ClientMessage::decode(&line)
                .unwrap_or_else(|error| panic!("{name}: {error}"))
                .encode(),
            "daemon-to-client" => DaemonMessage::decode(&line)
                .unwrap_or_else(|error| panic!("{name}: {error}"))
                .encode(),
            other => panic!("{name}: unknown direction `{other}`"),
        };
        assert_eq!(
            re_encoded, line,
            "{name} does not re-encode to its own bytes"
        );
    }
}

// ------------------------------------------------------- the live socket

/// A server bound under a fresh owner-only directory, plus its endpoint.
fn start(name: &str) -> (ServerHandle, PathBuf) {
    start_with(name, RecoveredDaemon::new(nothing_to_recover()))
}

fn start_with<O: Operations + 'static>(name: &str, operations: O) -> (ServerHandle, PathBuf) {
    start_shared(name, Arc::new(operations) as Arc<dyn Operations>)
}

/// The same, for a test that has to keep its own handle on the daemon — because the thing it is
/// checking is what happens on the socket when something changes somewhere else.
fn start_shared(name: &str, operations: Arc<dyn Operations>) -> (ServerHandle, PathBuf) {
    let mut directory = std::env::temp_dir();
    directory.push(format!(
        "mesh-ipc-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("the endpoint directory");
    let path = directory.join("daemon.sock");
    let server = IpcServer::bind(&path).expect("bind");
    let handle = server.spawn(operations).expect("spawn");
    (handle, path)
}

/// One connection, with a `hello` already exchanged.
struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Client {
    fn connect(path: &std::path::Path) -> Self {
        let stream = UnixStream::connect(path).expect("connect");
        Self {
            writer: stream.try_clone().expect("clone"),
            reader: BufReader::new(stream),
            next_id: 1,
        }
    }

    fn send(&mut self, message: &ClientMessage) -> DaemonMessage {
        writeln!(self.writer, "{}", message.encode()).expect("write");
        self.writer.flush().expect("flush");
        self.read()
    }

    fn send_raw(&mut self, line: &str) -> DaemonMessage {
        writeln!(self.writer, "{line}").expect("write");
        self.writer.flush().expect("flush");
        self.read()
    }

    fn read(&mut self) -> DaemonMessage {
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("read");
        DaemonMessage::decode(line.trim_end()).expect("a well-formed reply")
    }

    fn hello(&mut self, session: &str) -> DaemonMessage {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&ClientMessage::Hello {
            id,
            versions: SUPPORTED_VERSIONS.to_vec(),
            session: session.to_owned(),
        })
    }

    fn call(&mut self, method: &str) -> DaemonMessage {
        self.call_with(method, Json::empty_object())
    }

    fn call_with(&mut self, method: &str, params: Json) -> DaemonMessage {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&ClientMessage::Call {
            id,
            method: method.to_owned(),
            version: SURFACE_VERSION,
            params,
        })
    }
}

/// A fresh, empty folder to open as a workspace.
fn scratch_folder(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "mesh-ipc-workspace-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace folder");
    path
}

/// Parameters that satisfy each method, so a sweep over [`METHODS`] reaches the operation behind
/// every one rather than stopping at a missing field.
fn parameters_for(method: &str, folder: &std::path::Path) -> Json {
    match method {
        "workspace.open" => Json::object([("path", Json::text(folder.display().to_string()))]),
        "review.open" => Json::object([
            ("bundle", Json::text("09".repeat(32))),
            ("target", Json::text("04".repeat(32))),
            ("opened_by", Json::text("08".repeat(32))),
        ]),
        "review.open-current" => Json::object([("opened_by", Json::text("08".repeat(32)))]),
        "review.approve" => Json::object([
            ("bundle", Json::text("09".repeat(32))),
            ("target", Json::text("04".repeat(32))),
            ("receipt", Json::text("00")),
        ]),
        "workspace.version.fork" => Json::object([
            ("operation", Json::text("11".repeat(32))),
            (
                "destination",
                Json::text(folder.join("historical.mesh").display().to_string()),
            ),
            ("expected_root", Json::text(folder.display().to_string())),
            ("expected_digest", Json::text("00".repeat(16))),
            ("expected_installation", Json::text("blake3:placeholder")),
        ]),
        _ => Json::empty_object(),
    }
}

#[test]
fn a_client_negotiates_and_then_calls_every_catalogue_method() {
    let (handle, path) = start_with("full", live_daemon());
    let folder = scratch_folder("full");
    let mut client = Client::connect(&path);

    let welcome = client.hello("desktop-01");
    match &welcome {
        DaemonMessage::Welcome {
            version,
            session,
            resumed,
            surface_version,
            ..
        } => {
            assert_eq!(*version, SURFACE_VERSION);
            assert_eq!(session, "desktop-01");
            assert!(!resumed, "a fresh daemon has never seen this session");
            assert_eq!(*surface_version, SURFACE_VERSION);
        }
        other => panic!("expected a welcome, got {other:?}"),
    }

    // Subscribe last: a successful subscription immediately emits an unsolicited event, which
    // must not be mistaken for the next catalogue call's correlated answer.
    for entry in METHODS
        .iter()
        .filter(|entry| entry.name != "events.subscribe")
        .chain(
            METHODS
                .iter()
                .filter(|entry| entry.name == "events.subscribe"),
        )
    {
        match client.call_with(entry.name, parameters_for(entry.name, &folder)) {
            DaemonMessage::Result { value, .. } => {
                assert!(value.is_object(), "{} answered a non-object", entry.name);
                assert_ne!(
                    value,
                    Json::empty_object(),
                    "{} answered an empty object, which is the dispatch fall-through",
                    entry.name
                );
            }
            DaemonMessage::Failed { code, .. }
                if (entry.name == "review.open" && code == "publication-target-absent")
                    || (entry.name == "review.open-current"
                        && code == "publication-review-not-computable")
                    || (entry.name == "review.approve" && code == "publication-trust-absent")
                    || (entry.name == "workspace.version.fork"
                        && matches!(
                            code.as_str(),
                            "workspace-version-no-workspace"
                                | "workspace-version-history-incomplete"
                                | "workspace-version-source-changed"
                        ))
                    || (entry.since == 3 && code.ends_with("-required")) =>
            {
                // These are mutating methods, and this catalogue sweep intentionally opened an
                // empty workspace with no trust or folder-management parameters. Reaching their
                // typed refusal proves dispatch reached the method without inventing durable
                // fixture state; the desktop-management suite exercises the v3 happy path.
            }
            other => panic!("{} answered {other:?}", entry.name),
        }
    }

    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

/// A daemon with a real workspace behind it — the one the binary serves.
fn live_daemon() -> LiveDaemon {
    LiveDaemon::new(StartupSummary::from(&nothing_to_recover()))
}

#[test]
fn a_version_one_client_is_still_served_and_cannot_reach_a_version_two_method() {
    let (handle, path) = start_with("old-client", live_daemon());
    let mut client = Client::connect(&path);
    let welcome = client.send(&ClientMessage::Hello {
        id: 1,
        versions: vec![1],
        session: "old".to_owned(),
    });
    match welcome {
        DaemonMessage::Welcome { version, .. } => assert_eq!(version, 1),
        other => panic!("a version 1 client was not served: {other:?}"),
    }

    // The version 1 methods still answer, byte-shape unchanged.
    for name in ["daemon.status", "startup.report", "surface.describe"] {
        let reply = client.send(&ClientMessage::Call {
            id: 9,
            method: name.to_owned(),
            version: 1,
            params: Json::empty_object(),
        });
        assert!(
            matches!(reply, DaemonMessage::Result { .. }),
            "{name} stopped answering a version 1 client: {reply:?}"
        );
    }

    let reply = client.send(&ClientMessage::Call {
        id: 10,
        method: "workspace.open".to_owned(),
        version: 1,
        params: Json::object([("path", Json::text("/tmp"))]),
    });
    assert!(
        matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "method-newer-than-surface"),
        "{reply:?}"
    );

    drop(client);
    handle.shutdown();
}

#[test]
fn live_performance_counters_are_queryable_without_a_benchmark_or_workspace() {
    let (handle, path) = start_with("counters", live_daemon());
    let mut client = Client::connect(&path);
    client.hello("counter-reader");

    let reply = client.call("performance.counters");
    let DaemonMessage::Result { value, .. } = reply else {
        panic!("performance.counters answered {reply:?}");
    };
    let counters = value
        .get("counters")
        .and_then(Json::as_array)
        .expect("the complete counter catalogue");
    assert_eq!(counters.len(), mesh_daemon::counters::counter_count());
    assert_eq!(
        value.get("integer_encoding").and_then(Json::as_text),
        Some(mesh_daemon::counters::INTEGER_ENCODING)
    );
    assert_eq!(
        counters[0].get("observations").and_then(Json::as_text),
        Some("0"),
        "v4 counter integers stay exact for JavaScript clients by using decimal strings"
    );
    assert_eq!(counters[0].get("total").and_then(Json::as_text), Some("0"));
    assert!(value.get("conditions").and_then(Json::as_text).is_some());
    assert!(value.get("collection").is_some());
    assert!(value.get("not_yet").and_then(Json::as_array).is_some());

    drop(client);
    handle.shutdown();
}

#[test]
fn a_workspace_is_opened_over_the_socket_and_its_records_are_read_back() {
    let folder = scratch_folder("read-back");
    // Three real records, framed by `mesh-store` and forced to disk, before the daemon sees them.
    let mut file = RecordFile::open(&folder.join(RECORD_FILE_NAME)).expect("the record file");
    journal_records(&mut file, saved_records().iter()).expect("append");
    drop(file);

    let (handle, path) = start_with("read-back", live_daemon());
    let mut client = Client::connect(&path);
    client.hello("desktop-01");

    let opened = client.call_with(
        "workspace.open",
        Json::object([("path", Json::text(folder.display().to_string()))]),
    );
    let DaemonMessage::Result { value, .. } = opened else {
        panic!("workspace.open answered {opened:?}");
    };
    assert_eq!(value.get("records").and_then(Json::as_u64), Some(3));
    assert_eq!(value.get("operations").and_then(Json::as_u64), Some(2));
    assert_eq!(value.get("peers").and_then(Json::as_u64), Some(1));
    assert_eq!(
        value.get("unfinished_bytes").and_then(Json::as_u64),
        Some(0)
    );
    let digest = value
        .get("digest")
        .and_then(Json::as_text)
        .expect("a digest")
        .to_owned();
    assert!(!digest.is_empty());

    // `workspace.state` answers the same facts about the same workspace, from the same fold.
    let state = client.call("workspace.state");
    let DaemonMessage::Result { value: after, .. } = state else {
        panic!("workspace.state answered {state:?}");
    };
    assert_eq!(after.get("records").and_then(Json::as_u64), Some(3));
    assert_eq!(after.get("digest").and_then(Json::as_text), Some(&*digest));

    // The refusals are published rather than left for a reader to discover.
    let not_yet = after
        .get("not_yet")
        .and_then(Json::as_array)
        .expect("not_yet");
    assert!(!not_yet.is_empty(), "the honest refusals were dropped");
    for entry in not_yet {
        assert!(entry.get("subject").and_then(Json::as_text).is_some());
        assert!(entry.get("reason").and_then(Json::as_text).is_some());
    }

    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_answer_carries_a_version_derived_from_the_records_and_never_a_missing_crate() {
    // The whole path, end to end: framed records on disk, a real socket, and the JSON a client
    // reads. The unit tests prove the fold; this proves the fold reaches the wire.
    let folder = scratch_folder("pv");
    let mut file = RecordFile::open(&folder.join(RECORD_FILE_NAME)).expect("the record file");
    journal_records(&mut file, saved_records().iter()).expect("append");
    drop(file);

    let (handle, path) = start_with("pv", live_daemon());
    let mut client = Client::connect(&path);
    client.hello("desktop-01");
    let opened = client.call_with(
        "workspace.open",
        Json::object([("path", Json::text(folder.display().to_string()))]),
    );
    let DaemonMessage::Result { value, .. } = opened else {
        panic!("workspace.open answered {opened:?}");
    };

    let version = value.get("private_version").expect("private_version");
    // Two saved changes, one following the other, so one line of work and nothing waiting.
    assert_eq!(
        version.get("changes_applied").and_then(Json::as_u64),
        Some(2)
    );
    assert_eq!(
        version.get("concurrent_changes").and_then(Json::as_u64),
        Some(1)
    );
    assert_eq!(
        version
            .get("waiting")
            .and_then(Json::as_array)
            .map(<[Json]>::len),
        Some(0)
    );
    assert_eq!(
        version.get("waiting_not_listed").and_then(Json::as_u64),
        Some(0)
    );
    assert_eq!(
        version.get("state").and_then(Json::as_text),
        Some("working")
    );
    assert_eq!(
        version.get("derivation").and_then(Json::as_text),
        Some("blake3/mesh.v0.actor-head")
    );
    assert_eq!(
        version.get("apply_order_agrees").and_then(Json::as_bool),
        Some(true),
        "mesh-state and mesh-materializer ordered the same records differently"
    );
    assert_eq!(
        version
            .get("checked_against_author_claim")
            .and_then(Json::as_bool),
        Some(false),
        "a derived version must never be published as a verified one"
    );

    let identifier = version
        .get("version")
        .and_then(Json::as_text)
        .expect("a version identifier");
    assert_eq!(identifier.len(), 64, "a 32-byte identifier, in hexadecimal");
    assert!(identifier.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(
        identifier,
        "0".repeat(64),
        "a zero identifier would be a plausible-looking absence"
    );

    // The refusals that are left name a record or a check, never a crate this build now has.
    let not_yet = value
        .get("not_yet")
        .and_then(Json::as_array)
        .expect("not_yet");
    assert!(!not_yet.is_empty(), "the honest refusals were dropped");
    for entry in not_yet {
        let subject = entry
            .get("subject")
            .and_then(Json::as_text)
            .expect("subject");
        let reason = entry.get("reason").and_then(Json::as_text).expect("reason");
        assert!(
            !reason.contains("not a dependency"),
            "`{subject}` still refuses by naming a dependency edge"
        );
        for crate_name in ["mesh-state", "mesh-materializer", "mesh-types"] {
            assert!(
                !reason.contains(crate_name),
                "`{subject}` blames `{crate_name}`, which this build depends on"
            );
        }
    }
    let refused: Vec<&str> = not_yet
        .iter()
        .filter_map(|entry| entry.get("subject").and_then(Json::as_text))
        .collect();
    assert!(
        !refused.contains(&"shared and private version state"),
        "the private half is answered, so the joined subject cannot still be refused"
    );

    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn an_empty_workspace_reports_no_head_over_the_socket() {
    let folder = scratch_folder("empty-pv");
    let (handle, path) = start_with("empty-pv", live_daemon());
    let mut client = Client::connect(&path);
    client.hello("desktop-01");

    let opened = client.call_with(
        "workspace.open",
        Json::object([("path", Json::text(folder.display().to_string()))]),
    );
    let DaemonMessage::Result { value, .. } = opened else {
        panic!("workspace.open answered {opened:?}");
    };
    let version = value.get("private_version").expect("private_version");
    assert_eq!(version.get("version"), Some(&Json::Null));
    assert_eq!(
        version.get("changes_applied").and_then(Json::as_u64),
        Some(0)
    );
    assert_eq!(
        version
            .get("waiting")
            .and_then(Json::as_array)
            .map(<[Json]>::len),
        Some(0)
    );

    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_daemon_with_no_workspace_open_refuses_state_rather_than_answering_an_empty_one() {
    let (handle, path) = start_with("nothing-open", live_daemon());
    let mut client = Client::connect(&path);
    client.hello("desktop-01");
    let reply = client.call("workspace.state");
    assert!(
        matches!(reply, DaemonMessage::Failed { ref code, .. } if code == "no-workspace-open"),
        "an empty answer would read like an empty workspace: {reply:?}"
    );
    drop(client);
    handle.shutdown();
}

#[test]
fn a_folder_whose_records_are_wrong_is_refused_with_a_code_and_a_sentence() {
    let folder = scratch_folder("damaged");
    let mut framed = frame_record(&StoredRecord::Peer(PeerRecord {
        peer: RecordDigest::from_bytes([7; 32]),
        joined_at: RecordDigest::from_bytes([1; 32]),
    }));
    let last = framed.len() - 1;
    framed[last] ^= 0xFF;
    std::fs::write(folder.join(RECORD_FILE_NAME), &framed).expect("write");

    let (handle, path) = start_with("damaged", live_daemon());
    let mut client = Client::connect(&path);
    client.hello("desktop-01");
    let reply = client.call_with(
        "workspace.open",
        Json::object([("path", Json::text(folder.display().to_string()))]),
    );
    match reply {
        DaemonMessage::Failed { code, message, .. } => {
            assert_eq!(code, "workspace-damaged");
            assert!(
                message.contains("changed nothing"),
                "a refusal must say what happened to the person's work: {message}"
            );
        }
        other => panic!("a damaged workspace answered {other:?}"),
    }
    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_subscribed_client_is_pushed_what_happens_next_without_asking_again() {
    let folder = scratch_folder("subscribe");
    let daemon = Arc::new(live_daemon());
    let (handle, path) = start_shared("subscribe", Arc::clone(&daemon) as Arc<dyn Operations>);
    let mut client = Client::connect(&path);
    client.hello("desktop-01");

    let subscribed = client.call("events.subscribe");
    let DaemonMessage::Result { value, .. } = subscribed else {
        panic!("events.subscribe answered {subscribed:?}");
    };
    assert_eq!(value.get("subscribed").and_then(Json::as_bool), Some(true));

    // Everything the daemon has already done is delivered first: the `serving` entry it published
    // when it started. That is the point of a cursor — a client that connects late is not blind.
    let first = client.read();
    match &first {
        DaemonMessage::Event { sequence, kind, .. } => {
            assert_eq!(*sequence, 1);
            assert_eq!(kind, "serving");
        }
        other => panic!("expected the backlog, got {other:?}"),
    }

    // Now make something happen on ANOTHER thread's connection and read the push on this one.
    daemon
        .open_at_start(&folder)
        .expect("the daemon opens the folder");
    let pushed = client.read();
    match pushed {
        DaemonMessage::Event {
            id,
            sequence,
            kind,
            value,
        } => {
            assert_eq!(id, 2, "the push carries the subscription's identifier");
            assert_eq!(sequence, 2);
            assert_eq!(kind, "workspace-opened");
            assert_eq!(value.get("records").and_then(Json::as_u64), Some(0));
        }
        other => panic!("nothing was pushed: {other:?}"),
    }

    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_client_that_never_subscribes_is_never_sent_an_unsolicited_line() {
    let folder = scratch_folder("quiet");
    let daemon = Arc::new(live_daemon());
    let (handle, path) = start_shared("quiet", Arc::clone(&daemon) as Arc<dyn Operations>);
    let mut client = Client::connect(&path);
    client.hello("desktop-01");
    daemon.open_at_start(&folder).expect("open");
    // Long enough for several read polls to have come round with entries waiting.
    std::thread::sleep(std::time::Duration::from_millis(200));

    // The next line on this socket is the answer to the next call, not a push before it.
    let reply = client.call("daemon.status");
    assert!(
        matches!(reply, DaemonMessage::Result { id: 2, .. }),
        "a version 2 daemon pushed to a client that never subscribed: {reply:?}"
    );

    drop(client);
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_reply_identifier_is_the_one_the_call_carried() {
    let (handle, path) = start("correlate");
    let mut client = Client::connect(&path);
    assert_eq!(client.hello("desktop-01").id(), 1);
    assert_eq!(client.call("daemon.status").id(), 2);
    assert_eq!(client.call("surface.describe").id(), 3);
    drop(client);
    handle.shutdown();
}

#[test]
fn a_client_with_no_shared_version_is_refused_and_told_what_is_supported() {
    let (handle, path) = start("refuse");
    let mut client = Client::connect(&path);
    let reply = client.send(&ClientMessage::Hello {
        id: 1,
        versions: vec![99],
        session: "desktop-01".to_owned(),
    });
    match reply {
        DaemonMessage::Refused {
            code,
            message,
            supported,
            ..
        } => {
            assert_eq!(code, "unsupported-version");
            assert_eq!(supported, SUPPORTED_VERSIONS);
            assert!(message.contains("Update both"), "{message}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    drop(client);
    handle.shutdown();
}

#[test]
fn an_unreadable_line_is_answered_and_the_connection_survives() {
    let (handle, path) = start("garbage");
    let mut client = Client::connect(&path);
    client.hello("desktop-01");

    for garbage in [
        "{ not json",
        r#"{"t":"nope","id":9}"#,
        r#"{"t":"call","id":9}"#,
    ] {
        let reply = client.send_raw(garbage);
        assert!(
            matches!(reply, DaemonMessage::Failed { .. }),
            "{garbage} got {reply:?}"
        );
        assert_eq!(reply.id(), 0, "an unreadable line correlates with nothing");
    }

    // The connection is still usable: a fault in one request is not an outage.
    assert!(matches!(
        client.call("daemon.status"),
        DaemonMessage::Result { .. }
    ));
    drop(client);
    handle.shutdown();
}

#[test]
fn a_method_outside_the_catalogue_is_refused_rather_than_served() {
    let (handle, path) = start("closed");
    let mut client = Client::connect(&path);
    client.hello("desktop-01");
    for forbidden in ["store.write", "database.query", "sql.execute"] {
        match client.call(forbidden) {
            DaemonMessage::Failed { code, .. } => assert_eq!(code, "unknown-method"),
            other => panic!("{forbidden} was answered with {other:?}"),
        }
    }
    drop(client);
    handle.shutdown();
}

#[test]
fn a_returning_session_on_the_same_daemon_is_recognised() {
    let (handle, path) = start("resume");

    let mut first = Client::connect(&path);
    assert!(matches!(
        first.hello("desktop-01"),
        DaemonMessage::Welcome { resumed: false, .. }
    ));
    drop(first);

    let mut second = Client::connect(&path);
    assert!(matches!(
        second.hello("desktop-01"),
        DaemonMessage::Welcome { resumed: true, .. }
    ));
    drop(second);

    handle.shutdown();
}

#[test]
fn a_restarted_daemon_binds_the_same_endpoint_and_serves_the_client_again() {
    let (handle, path) = start("restart");
    let mut before = Client::connect(&path);
    assert!(matches!(
        before.hello("desktop-01"),
        DaemonMessage::Welcome { .. }
    ));

    // A daemon that is killed rather than shut down leaves a Unix socket behind. Simulate the
    // filesystem state exactly: shut down the first server, bind and drop a listener without
    // unlinking its socket, then prove the restarted daemon recovers that stale socket. An
    // ordinary file at this path must never be treated as daemon-owned cleanup material.
    handle.shutdown();
    let stale = std::os::unix::net::UnixListener::bind(&path).expect("bind stale socket");
    drop(stale);
    assert!(path.exists());

    let server = IpcServer::bind(&path).expect("the restart binds over the stale file");
    let restarted = server
        .spawn(Arc::new(RecoveredDaemon::new(nothing_to_recover())) as Arc<dyn Operations>)
        .expect("spawn");

    let mut after = Client::connect(&path);
    match after.hello("desktop-01") {
        DaemonMessage::Welcome {
            resumed, session, ..
        } => {
            assert_eq!(
                session, "desktop-01",
                "the session name survives the restart"
            );
            assert!(
                !resumed,
                "the daemon's registry is in memory; the client's context is the client's own"
            );
        }
        other => panic!("expected a welcome after the restart, got {other:?}"),
    }
    assert!(matches!(
        after.call("daemon.status"),
        DaemonMessage::Result { .. }
    ));

    drop(after);
    restarted.shutdown();
}

#[test]
fn the_endpoint_is_a_filesystem_path_and_not_a_network_address() {
    let (handle, path) = start("local");
    let stream = UnixStream::connect(&path).expect("connect");
    let peer = stream.peer_addr().expect("peer address");
    assert_eq!(
        peer.as_pathname(),
        Some(path.as_path()),
        "the peer of a local connection is a filesystem path"
    );
    drop(stream);
    handle.shutdown();
}

#[test]
fn no_source_in_this_crate_names_a_network_type() {
    // The transport claim, checked against the code rather than against the module documentation.
    // `std::net::Shutdown` is deliberately tolerated: it is the half-close enum, it opens nothing,
    // and `UnixStream::shutdown` needs it.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let banned = [
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "ToSocketAddrs",
        "SocketAddrV4",
        "SocketAddrV6",
        "IpAddr",
    ];
    let mut scanned = 0usize;
    let mut stack = vec![root];
    while let Some(entry) = stack.pop() {
        for child in std::fs::read_dir(&entry).expect("readable") {
            let child = child.expect("entry").path();
            if child.is_dir() {
                stack.push(child);
            } else if child.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&child).expect("readable source");
                scanned += 1;
                for word in banned {
                    assert!(
                        !source.contains(word),
                        "{} names `{word}`: this crate's transport is local only",
                        child.display()
                    );
                }
            }
        }
    }
    assert!(
        scanned >= 5,
        "only {scanned} sources scanned — the walk broke"
    );
}

#[test]
fn concurrent_connections_are_all_served() {
    let (handle, path) = start("concurrent");
    let mut clients: Vec<Client> = (0..4).map(|_| Client::connect(&path)).collect();
    for (index, client) in clients.iter_mut().enumerate() {
        assert!(matches!(
            client.hello(&format!("desktop-{index}")),
            DaemonMessage::Welcome { .. }
        ));
    }
    for client in &mut clients {
        assert!(matches!(
            client.call("daemon.status"),
            DaemonMessage::Result { .. }
        ));
    }
    drop(clients);
    assert!(handle.connections_served() >= 4);
    handle.shutdown();
}

// ------------------------------------------------------- what the surface says

#[test]
fn the_startup_report_carries_the_product_sentence_and_not_the_internal_detail() {
    let diagnostic = RecoveryDiagnostic::new(
        RecoveryOutcome::Unrecoverable {
            detail: "sqlite: no such table: operation".to_owned(),
        },
        std::time::Duration::from_millis(12),
        RECOVERY_BUDGET,
    );
    let (handle, path) = start_with("sentence", RecoveredDaemon::new(diagnostic));
    let mut client = Client::connect(&path);
    client.hello("desktop-01");
    match client.call("startup.report") {
        DaemonMessage::Result { value, .. } => {
            let sentence = value
                .get("sentence")
                .and_then(Json::as_text)
                .expect("sentence");
            assert!(!sentence.contains("sqlite"), "{sentence}");
            assert!(sentence.contains("changed nothing"), "{sentence}");
            assert_eq!(
                value.get("severity").and_then(Json::as_text),
                Some("blocking")
            );
            assert_eq!(value.get("serving").and_then(Json::as_bool), Some(false));
        }
        other => panic!("startup.report answered {other:?}"),
    }
    drop(client);
    handle.shutdown();
}

#[test]
fn a_startup_summary_is_built_from_the_diagnostic_it_describes() {
    let diagnostic = RecoveryDiagnostic::new(
        RecoveryOutcome::Rebuilt {
            records: 7,
            rows: 9,
            digest: mesh_store::Digest16::from_bytes([1u8; 16]),
        },
        std::time::Duration::from_millis(4),
        RECOVERY_BUDGET,
    );
    let summary = StartupSummary::from(&diagnostic);
    assert!(summary.serving);
    assert_eq!(summary.severity, Severity::Routine);
    assert_eq!(summary.elapsed_ms, 4);
    assert!(summary.sentence.contains('7'), "{}", summary.sentence);
}

// ------------------------------------------- the product vocabulary, as a smoke alarm
//
// `node tools/program/vocab-lint/lint.mjs --user-facing` is the AUTHORITY on this rule and it
// scans `crates/mesh-daemon/src/user_messages.rs` with an exemption budget of zero. These two
// tests are the coarse second guard, so a lane running `cargo nextest` alone finds out at once
// rather than at gate time. They live HERE and not beside the copy because the lint cannot tell a
// `#[cfg(test)]` literal apart from shipped copy, and a test that names nine banned words inside
// the linted file makes that file fail its own gate.

/// Every sentence `user_messages` can produce.
fn every_user_sentence() -> Vec<String> {
    use mesh_daemon::user_messages as copy;
    let digest = mesh_store::Digest16::from_bytes([0u8; 16]);
    let outcomes = [
        RecoveryOutcome::Rebuilt {
            records: 3,
            rows: 4,
            digest,
        },
        RecoveryOutcome::RebuiltAfterAnInterruptedSave {
            records: 3,
            rows: 4,
            digest,
            discarded_bytes: 17,
        },
        RecoveryOutcome::NothingDurableToRecover {
            unfinished_bytes: 41,
        },
        RecoveryOutcome::Unrecoverable {
            detail: "the internal reason, which no sentence repeats".to_owned(),
        },
    ];
    [
        copy::NO_SHARED_VERSION,
        copy::ALREADY_OPEN,
        copy::NOT_OPEN,
        copy::VERSION_NOT_NEGOTIATED,
        copy::UNKNOWN_METHOD,
        copy::METHOD_TOO_NEW,
        copy::UNREADABLE_REQUEST,
        // The four sentences a refused open produces. They reach a person through
        // `live::refusal_for`, and leaving them out of this list left them unchecked here.
        copy::WORKSPACE_UNREACHABLE,
        copy::WORKSPACE_DAMAGED,
        copy::WORKSPACE_NOTHING_READABLE,
        copy::WORKSPACE_CONTRADICTORY,
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(
        outcomes
            .iter()
            .map(mesh_daemon::user_messages::startup_sentence),
    )
    .collect()
}

#[test]
fn no_user_facing_message_uses_a_word_from_the_internal_model() {
    let banned = [
        "dag",
        "frontier",
        "vector clock",
        "branch",
        "commit",
        "rebase",
        "staging",
        "oplog",
        "operation log",
    ];
    let sentences = every_user_sentence();
    assert!(sentences.len() >= 15, "the sentence list lost entries");
    for sentence in sentences {
        let lowered = sentence.to_lowercase();
        for word in banned {
            assert!(
                !lowered.contains(word),
                "user-facing text says `{word}`: {sentence}"
            );
        }
    }
}

#[test]
fn every_user_facing_message_says_what_happened_to_the_persons_work() {
    for sentence in every_user_sentence() {
        assert!(sentence.ends_with('.'), "not a sentence: {sentence}");
        assert!(sentence.len() > 40, "too terse to act on: {sentence}");
    }
}

#[test]
fn an_unrecoverable_start_up_never_leaks_the_internal_detail() {
    let sentence = mesh_daemon::user_messages::startup_sentence(&RecoveryOutcome::Unrecoverable {
        detail: "sqlite: no such table: operation".to_owned(),
    });
    assert!(!sentence.contains("sqlite"), "{sentence}");
    assert!(sentence.contains("changed nothing"), "{sentence}");
}
