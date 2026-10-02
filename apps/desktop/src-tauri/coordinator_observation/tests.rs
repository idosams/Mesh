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
    assert!(parse(&args(&["--worker", "identity", "/private/config"]))
        .unwrap()
        .is_none());
    for v in [
        vec!["--coordinator"],
        vec!["--coordinator", "launch", "/private/config"],
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
    assert_eq!(config(value()).unwrap().port, 22);
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
    for action in [Action::Status, Action::Results(0), Action::Receive] {
        assert_eq!(
            run(action, Path::new("/unused-mesh-coordinator-config")).unwrap_err(),
            "Coordinator identity requires an eligible signed Mesh application"
        );
    }
}
