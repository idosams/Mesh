//! Separate-process publication crash fixtures. Software credentials are test data only.
use super::super::super::dependency_private_context::publication::Step;
use super::*;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const ENV: &str = "MESH_PUBLICATION_RESTART_FIXTURE";
const CHILD: &str =
    "project_attachment::dependency_publication_replay::tests::restart::publication_restart_child";

#[test]
fn publication_restart_child() {
    let Ok(raw) = std::env::var(ENV) else {
        return;
    };
    let v = Json::parse(&raw).unwrap();
    let text = |key| v.get(key).and_then(Json::as_text).unwrap();
    let root = Path::new(text("root"));
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let request = RecordDigest::parse_hex(text("request")).unwrap();
    let public = fs::read(root.join("restart-public-key")).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap()).unwrap();
    let mode = text("mode");
    let trust = if mode == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([credential])
    };
    if matches!(
        v.get("kind").and_then(Json::as_text),
        Some("input-decision" | "saved-review" | "snapshot")
    ) {
        use super::super::super::dependency_private_context::control::Step as ControlStep;
        let operation = RecordDigest::parse_hex(text("operation")).unwrap();
        let previous = RecordDigest::parse_hex(text("previous")).unwrap();
        let hook = |step, file: &mut fs::File, frame: &[u8]| {
            if matches!(step, ControlStep::Staged) {
                if mode == "staged" {
                    std::process::exit(85);
                }
                if mode == "partial" {
                    let cut = match text("cut") {
                        "first" => 1,
                        "last" => frame.len() - 1,
                        _ => frame.len() / 2,
                    };
                    file.write_all(&frame[..cut])?;
                    file.sync_all()?;
                    std::process::exit(86);
                }
            }
            if matches!(step, ControlStep::Appended) && mode == "lost-ack" {
                std::process::exit(87);
            }
            Ok(())
        };
        let exact_request = if mode == "foreign-request" {
            RecordDigest::from_bytes([96; 32])
        } else {
            request
        };
        let result = if text("kind") == "snapshot" {
            storage
                .save_native_review_snapshot_with_io(
                    text("work"),
                    operation,
                    exact_request,
                    &trust,
                    hook,
                    |f| f.sync_all(),
                )
                .map(|result| {
                    Json::object([
                        ("record", Json::text(result.record().to_hex())),
                        ("graph", Json::text(result.graph().to_hex())),
                        ("validation", Json::text(result.validation().to_hex())),
                    ])
                })
        } else if text("kind") == "saved-review" {
            storage
                .save_native_review_with_io(
                    text("work"),
                    operation,
                    previous,
                    exact_request,
                    &trust,
                    hook,
                    |f| f.sync_all(),
                )
                .map(|result| {
                    Json::object([
                        ("record", Json::text(result.record().to_hex())),
                        ("bundle", Json::text(result.bundle().to_hex())),
                    ])
                })
        } else {
            storage
                .decide_native_saved_input_with_io(
                    text("work"),
                    operation,
                    crate::project_attachment::NativeSavedInputDecision::Rejected,
                    Some(previous),
                    exact_request,
                    &trust,
                    hook,
                    |f| f.sync_all(),
                )
                .map(|result| {
                    Json::object([
                        ("record", Json::text(result.record().to_hex())),
                        ("revision", Json::Number(result.revision())),
                    ])
                })
        };
        if mode == "missing-trust" || mode == "foreign-request" {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            fs::write(root.join("decision-restart-result"), result.encode()).unwrap();
        }
        return;
    }
    let review = RecordDigest::parse_hex(text("review")).unwrap();
    let receipt = fs::read(root.join("restart-receipt")).unwrap();
    let result = storage.commit_native_publication_with_io(
        text("work"),
        if mode == "foreign-request" {
            RecordDigest::from_bytes([99; 32])
        } else {
            request
        },
        review,
        &receipt,
        &trust,
        |step, file, frame| {
            if matches!(step, Step::Staged) {
                if mode == "staged" {
                    std::process::exit(75);
                }
                if mode == "partial" {
                    let cut = match text("cut") {
                        "first" => 1,
                        "last" => frame.len() - 1,
                        _ => frame.len() / 2,
                    };
                    file.write_all(&frame[..cut])?;
                    file.sync_all()?;
                    std::process::exit(76);
                }
            }
            if matches!(step, Step::Appended) && mode == "lost-ack" {
                std::process::exit(77);
            }
            Ok(())
        },
        |file| file.sync_all(),
    );
    if mode == "missing-trust" || mode == "foreign-request" {
        assert!(result.is_err());
    } else {
        let result = result.unwrap();
        fs::write(
            root.join("restart-result"),
            Json::object([
                ("record", Json::text(result.record().to_hex())),
                ("head", Json::text(result.head().to_hex())),
                ("revision", Json::Number(result.revision())),
            ])
            .encode(),
        )
        .unwrap();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_publication(
    root: &Path,
    work: &str,
    request: RecordDigest,
    review: RecordDigest,
    receipt: &[u8],
    public: &[u8],
    journal: &Path,
    cut: &str,
    after_staging: impl FnOnce(),
) -> Json {
    fs::write(root.join("restart-public-key"), public).unwrap();
    fs::write(root.join("restart-receipt"), receipt).unwrap();
    let run = |mode: &str, expected: i32| {
        let value = Json::object([
            ("root", Json::text(root.to_string_lossy())),
            ("work", Json::text(work)),
            ("request", Json::text(request.to_hex())),
            ("review", Json::text(review.to_hex())),
            ("mode", Json::text(mode)),
            ("cut", Json::text(cut)),
        ]);
        run_child(root, mode, value, expected);
    };
    let before = fs::read(journal).unwrap();
    run("staged", 75);
    assert_eq!(fs::read(journal).unwrap(), before);
    after_staging();
    assert_eq!(fs::read(journal).unwrap(), before);
    run("partial", 76);
    let torn = fs::read(journal).unwrap();
    assert!(torn.starts_with(&before) && torn.len() > before.len());
    run("missing-trust", 0);
    assert_eq!(fs::read(journal).unwrap(), torn);
    run("foreign-request", 0);
    assert_eq!(fs::read(journal).unwrap(), torn);
    run("lost-ack", 77);
    let complete = fs::read(journal).unwrap();
    assert!(complete.starts_with(&torn) && complete.len() > torn.len());
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    let result = fs::read_to_string(root.join("restart-result")).unwrap();
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    assert_eq!(
        fs::read_to_string(root.join("restart-result")).unwrap(),
        result
    );
    Json::parse(&result).unwrap()
}

fn run_child(root: &Path, mode: &str, value: Json, expected: i32) {
    let output = root.join(format!("restart-{mode}.log"));
    let stdout = fs::File::create(&output).unwrap();
    let stderr = stdout.try_clone().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD, "--nocapture"])
        .env(ENV, value.encode())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "publication restart {mode} timed out: {}",
                fs::read_to_string(&output).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(
        status.code(),
        Some(expected),
        "{}",
        fs::read_to_string(&output).unwrap()
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_native_decision(
    root: &Path,
    work: &str,
    operation: RecordDigest,
    previous: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
    published: bool,
    cut: &str,
) -> Json {
    interrupted_native_control(
        root,
        work,
        operation,
        previous,
        request,
        public,
        journal,
        published,
        cut,
        "input-decision",
        |_| {},
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_native_review(
    root: &Path,
    work: &str,
    snapshot: RecordDigest,
    opener: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
) -> Json {
    interrupted_native_control(
        root,
        work,
        snapshot,
        opener,
        request,
        public,
        journal,
        true,
        "mid",
        "saved-review",
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_native_snapshot(
    root: &Path,
    work: &str,
    operation: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
    inspect_prefix: impl Fn(&str),
) -> Json {
    interrupted_native_control(
        root,
        work,
        operation,
        RecordDigest::from_bytes([0; 32]),
        request,
        public,
        journal,
        true,
        "last",
        "snapshot",
        inspect_prefix,
    )
}

#[allow(clippy::too_many_arguments)]
fn interrupted_native_control(
    root: &Path,
    work: &str,
    operation: RecordDigest,
    previous: RecordDigest,
    request: RecordDigest,
    public: &[u8],
    journal: &Path,
    published: bool,
    cut: &str,
    kind: &str,
    inspect_prefix: impl Fn(&str),
) -> Json {
    fs::write(root.join("restart-public-key"), public).unwrap();
    let run = |mode: &str, expected: i32| {
        let value = Json::object([
            ("kind", Json::text(kind)),
            ("root", Json::text(root.to_string_lossy())),
            ("work", Json::text(work)),
            ("operation", Json::text(operation.to_hex())),
            ("previous", Json::text(previous.to_hex())),
            ("request", Json::text(request.to_hex())),
            ("mode", Json::text(mode)),
            ("cut", Json::text(cut)),
        ]);
        run_child(root, mode, value, expected);
    };
    let before = fs::read(journal).unwrap();
    run("staged", 85);
    assert_eq!(fs::read(journal).unwrap(), before);
    inspect_prefix("staged");
    assert_eq!(fs::read(journal).unwrap(), before);
    run("partial", 86);
    let torn = fs::read(journal).unwrap();
    assert!(torn.starts_with(&before) && torn.len() > before.len());
    inspect_prefix("partial");
    assert_eq!(fs::read(journal).unwrap(), torn);
    if published {
        run("missing-trust", 0);
        assert_eq!(fs::read(journal).unwrap(), torn);
    }
    run("foreign-request", 0);
    assert_eq!(fs::read(journal).unwrap(), torn);
    run("lost-ack", 87);
    let complete = fs::read(journal).unwrap();
    assert!(complete.starts_with(&torn) && complete.len() > torn.len());
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    let result = fs::read_to_string(root.join("decision-restart-result")).unwrap();
    run("retry", 0);
    assert_eq!(fs::read(journal).unwrap(), complete);
    assert_eq!(
        fs::read_to_string(root.join("decision-restart-result")).unwrap(),
        result
    );
    Json::parse(&result).unwrap()
}
