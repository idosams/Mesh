use super::*;
use std::os::unix::fs::{symlink, PermissionsExt as _};
pub(super) fn value() -> Json {
    Json::object([
        (
            "schema",
            Json::text("mesh.coordinator-observation-config/v1"),
        ),
        ("installation", Json::text("/private/tmp/operator-identity")),
        ("fleets", Json::text("/private/tmp/fleets")),
        ("objective", Json::text("objective")),
        ("lane", Json::text("lane")),
        ("run", Json::text("run")),
        ("host", Json::text("worker.example.test")),
        ("account", Json::text("mesh")),
        ("port", Json::Number(22)),
        ("identity", Json::text("/private/tmp/operator/key")),
        ("known_hosts", Json::text("/private/tmp/operator/hosts")),
        ("worker", Json::text("ab".repeat(32))),
    ])
}
#[test]
fn coordinator_arguments_are_explicit_and_bounded() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        parse(&args(&["--coordinator", "status", "/private/config"])).unwrap(),
        Some((Action::Status, PathBuf::from("/private/config")))
    );
    assert_eq!(
        parse(&args(&[
            "--coordinator",
            "results",
            "/private/config",
            "4096"
        ]))
        .unwrap(),
        Some((Action::Results(4096), PathBuf::from("/private/config")))
    );
    assert_eq!(
        parse(&args(&["--coordinator", "receive", "/private/config"])).unwrap(),
        Some((Action::Receive, PathBuf::from("/private/config")))
    );
    for (name, expected) in [
        ("start", Action::Start),
        ("created", Action::Created),
        ("recover-original", Action::RecoverOriginal),
    ] {
        assert_eq!(
            parse(&args(&["--coordinator", name, "/private/config"])).unwrap(),
            Some((expected, PathBuf::from("/private/config")))
        );
    }
    assert!(parse(&args(&["--worker", "identity", "/private/config"]))
        .unwrap()
        .is_none());
    for v in [
        vec!["--coordinator"],
        vec!["--coordinator", "launch", "/private/config"],
        vec!["--coordinator", "recover-original", "relative"],
        vec![
            "--coordinator",
            "recover-original",
            "/private/config",
            "retry",
        ],
        vec!["--coordinator", "status", "relative"],
        vec!["--coordinator", "status", "/private/config", "0"],
        vec!["--coordinator", "results", "/private/config", "01"],
        vec!["--coordinator", "results", "/private/config", "4097"],
        vec!["--coordinator", "results", "/private/config", "-1"],
    ] {
        assert!(parse(&args(&v)).is_err());
    }
}
#[test]
fn coordinator_config_refuses_unknown_fields_noncanonical_keys_and_unbounded_identity() {
    assert_eq!(config(value()).unwrap().connection.port, 22);
    for (field, replacement) in [
        (
            "schema",
            Json::text("mesh.coordinator-observation-config/v2"),
        ),
        ("installation", Json::text("relative")),
        ("fleets", Json::text("relative")),
        ("identity", Json::text("relative")),
        ("known_hosts", Json::text("relative")),
        ("worker", Json::text("AB".repeat(32))),
        ("port", Json::Number(0)),
        ("port", Json::Number(65536)),
        ("objective", Json::text("../other")),
        ("lane", Json::text("a".repeat(129))),
        ("run", Json::text("")),
    ] {
        let Json::Object(mut fields) = value() else {
            unreachable!()
        };
        fields.iter_mut().find(|(name, _)| name == field).unwrap().1 = replacement;
        assert!(config(Json::Object(fields)).is_err(), "{field}");
    }
    let Json::Object(mut fields) = value() else {
        unreachable!()
    };
    fields.push(("command".into(), Json::text("arbitrary shell")));
    assert!(config(Json::Object(fields)).is_err());
    let Json::Object(mut fields) = value() else {
        unreachable!()
    };
    fields[0].0 = "worker".into();
    assert!(config(Json::Object(fields)).is_err());
}
#[test]
fn coordinator_config_reader_preserves_private_file_and_refuses_links_or_public_permissions() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "mesh-coordinator-config-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("config.json");
    let bytes = value().encode();
    std::fs::write(&path, &bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(config(crate::worker_service::load_private_json(&path).unwrap()).is_ok());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    symlink(&path, root.join("link.json")).unwrap();
    assert!(crate::worker_service::load_private_json(&root.join("link.json")).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(crate::worker_service::load_private_json(&path).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, "x".repeat(16385)).unwrap();
    assert!(crate::worker_service::load_private_json(&path).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn coordinator_unknown_result_is_distinct_from_verified_empty_page() {
    let unknown = render(RemoteObservationOutcome::Results(None));
    assert_eq!(unknown.get("page"), Some(&Json::Null));
    let empty = render(RemoteObservationOutcome::Results(Some(
        mesh_daemon::fleet::RemoteSavedResultPage {
            revision: 0,
            after: 0,
            has_more: false,
            offers: vec![],
        },
    )));
    assert_eq!(
        empty.get("page").unwrap().get("offers"),
        Some(&Json::Array(vec![]))
    );
    assert_eq!(
        empty.get("page").unwrap().get("has_more"),
        Some(&Json::Bool(false))
    );
}

#[test]
fn unsigned_coordinator_refuses_before_loading_config_or_opening_custody() {
    assert!(AppleActorCustody::availability().is_err());
    for action in [
        Action::Status,
        Action::Results(0),
        Action::Receive,
        Action::Start,
        Action::Created,
        Action::ReconnectInput,
        Action::RecoverOriginal,
    ] {
        assert_eq!(
            run(action, Path::new("/unused-mesh-coordinator-config")).unwrap_err(),
            "Coordinator identity requires an eligible signed Mesh application"
        );
    }
}

struct OutputSink {
    bytes: Vec<u8>,
    remaining: Option<usize>,
    fail_flush: bool,
    flushes: usize,
}
impl io::Write for OutputSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len().min(2).min(self.remaining.unwrap_or(usize::MAX));
        if count == 0 && !bytes.is_empty() {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        self.bytes.extend_from_slice(&bytes[..count]);
        if let Some(remaining) = self.remaining.as_mut() {
            *remaining -= count;
        }
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        } else {
            Ok(())
        }
    }
}
#[test]
fn structured_result_is_preserved_across_short_cli_writes() {
    let result = render(RemoteObservationOutcome::Results(None));
    let original = result.encode();
    let mut output = OutputSink {
        bytes: vec![],
        remaining: None,
        fail_flush: false,
        flushes: 0,
    };
    write_result(&mut output, &result, output_failure(&Action::Results(0))).unwrap();
    assert_eq!(output.bytes, format!("{original}\n").as_bytes());
    assert_eq!(output.flushes, 1);
    assert_eq!(result.encode(), original);
}
#[test]
fn output_failure_retains_native_result_and_requires_explicit_reconciliation() {
    let result = render(RemoteObservationOutcome::Results(None));
    let original = result.encode();
    for action in [
        Action::Status,
        Action::Results(0),
        Action::Start,
        Action::Created,
        Action::Receive,
        Action::ReconnectInput,
        Action::RecoverOriginal,
    ] {
        for fail_flush in [false, true] {
            let mut output = OutputSink {
                bytes: vec![],
                remaining: if fail_flush { None } else { Some(7) },
                fail_flush,
                flushes: 0,
            };
            assert_eq!(
                write_result(&mut output, &result, output_failure(&action)).unwrap_err(),
                output_failure(&action)
            );
            assert_eq!(result.encode(), original);
            if fail_flush {
                assert_eq!(output.bytes, format!("{original}\n").as_bytes());
                assert_eq!(output.flushes, 1);
            } else {
                assert_eq!(output.bytes, original.as_bytes()[..7]);
                assert_eq!(output.flushes, 0);
            }
        }
    }
}
#[test]
fn direct_native_operation_retains_the_eligible_application_boundary() {
    assert!(AppleActorCustody::availability().is_err());
    for action in [
        Action::Status,
        Action::Results(0),
        Action::Start,
        Action::Created,
        Action::Receive,
        Action::ReconnectInput,
        Action::RecoverOriginal,
    ] {
        assert_eq!(
            execute(action, Path::new("/unused-mesh-coordinator-config")).unwrap_err(),
            "Coordinator identity requires an eligible signed Mesh application"
        );
    }
}
