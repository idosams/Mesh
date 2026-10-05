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
    let review = RecordDigest::parse_hex(text("review")).unwrap();
    let public = fs::read(root.join("restart-public-key")).unwrap();
    let credential =
        HumanApprovalCredential::from_public_key(public.as_slice().try_into().unwrap()).unwrap();
    let receipt = fs::read(root.join("restart-receipt")).unwrap();
    let mode = text("mode");
    let trust = if mode == "missing-trust" {
        crate::TrustedReviewers::default()
    } else {
        crate::TrustedReviewers::with_human_credentials([credential])
    };
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
    };
    let before = fs::read(journal).unwrap();
    run("staged", 75);
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
