use super::*;
fn value() -> Json {
    let Json::Object(mut peer) = super::super::tests::value() else {
        unreachable!()
    };
    peer.retain(|(key, _)| !matches!(key.as_str(), "objective" | "lane" | "run"));
    peer.iter_mut().find(|(key, _)| key == "schema").unwrap().1 =
        Json::text("mesh.coordinator-peer-config/v1");
    Json::object([
        ("schema", Json::text("mesh.coordinator-start-config/v1")),
        ("connection", Json::Object(peer)),
        ("storage", Json::text("/private/tmp/attachments")),
        ("project", Json::text("ab".repeat(32))),
        ("request", Json::text("ab".repeat(16))),
        ("version", Json::text("cd".repeat(32))),
        ("goal", Json::text("Work on this saved version")),
        ("provider", Json::text("codex")),
        (
            "limits",
            Json::object([
                ("lanes", Json::Number(2)),
                ("concurrency", Json::Number(1)),
                ("depth", Json::Number(1)),
                ("retries", Json::Number(0)),
            ]),
        ),
        ("lease_until_ms", Json::Number(123456789)),
    ])
}
#[test]
fn start_configuration_is_closed_and_admits_only_native_saved_project_policy() {
    let selected = configuration(value()).unwrap();
    assert_eq!(selected.policy.coordinator(), "codex");
    for (field, replacement) in [
        ("schema", Json::text("unknown")),
        ("storage", Json::text("relative")),
        ("project", Json::text("AB".repeat(32))),
        ("request", Json::text("bad")),
        ("version", Json::text("arbitrary")),
        ("provider", Json::text("shell")),
        ("goal", Json::text("")),
        ("lease_until_ms", Json::Number(0)),
    ] {
        let Json::Object(mut fields) = value() else {
            unreachable!()
        };
        fields.iter_mut().find(|(k, _)| k == field).unwrap().1 = replacement;
        assert!(configuration(Json::Object(fields)).is_err(), "{field}");
    }
    let Json::Object(mut fields) = value() else {
        unreachable!()
    };
    fields.push(("manifest".into(), Json::Null));
    assert!(configuration(Json::Object(fields)).is_err());
}
#[test]
fn fixed_deadline_refuses_expired_or_overlong_without_refreshing_retry_authority() {
    assert!(validate_deadline(60001, 1).is_ok());
    for (until, now) in [(1, 1), (1, 2), (3_600_002, 1)] {
        assert!(validate_deadline(until, now).is_err());
    }
}

#[test]
fn creation_inputs_are_retained_before_any_connection_and_cannot_change_on_retry() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = std::env::temp_dir().join(format!(
        "mesh-start-config-retention-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let storage = AttachmentStorage::open(&root).unwrap();
    let original = value();
    let selected = configuration(original.clone()).unwrap();
    retain_configuration(&storage, &selected, &original).unwrap();
    // None of the placeholder project, key or peer paths need exist for this private journal.
    drop(storage);
    let storage = AttachmentStorage::open(&root).unwrap();
    assert_eq!(storage.remote_start_requests().unwrap()[0].value, original);
    for field in ["lease_until_ms", "goal"] {
        let Json::Object(mut fields) = original.clone() else {
            unreachable!()
        };
        fields.iter_mut().find(|(k, _)| k == field).unwrap().1 = if field == "goal" {
            Json::text("Different goal")
        } else {
            Json::Number(123456790)
        };
        let changed = Json::Object(fields);
        let changed_selection = configuration(changed.clone()).unwrap();
        assert!(retain_configuration(&storage, &changed_selection, &changed).is_err());
    }
    assert_eq!(storage.remote_start_requests().unwrap()[0].value, original);
    std::fs::remove_dir_all(root).unwrap();
}
