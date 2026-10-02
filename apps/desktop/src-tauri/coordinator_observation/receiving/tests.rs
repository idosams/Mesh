use super::*;
fn value() -> Json {
    Json::object([
        ("schema", Json::text("mesh.coordinator-receive-config/v1")),
        ("connection", super::super::tests::value()),
        (
            "input",
            Json::object([
                ("kind", Json::text("project")),
                ("storage", Json::text("/private/metadata")),
                ("project", Json::text("a".repeat(64))),
                ("version", Json::text("b".repeat(64))),
            ]),
        ),
        ("store", Json::text("/private/receiving")),
        ("allocations", Json::text("/private/results")),
        ("allocation", Json::text("c".repeat(32))),
        // Parsing configuration grants no offer authority; native ingestion verifies the signature.
        ("offer", Json::text("{}")),
    ])
}
fn replace(value: &mut Json, name: &str, replacement: Json) {
    let Json::Object(fields) = value else {
        panic!("object")
    };
    fields.iter_mut().find(|(n, _)| n == name).unwrap().1 = replacement;
}
#[test]
fn receive_config_has_closed_selections_and_no_manifest_or_command_fields() {
    assert!(matches!(
        configuration(value()).unwrap().input,
        Input::Project { .. }
    ));
    let mut review = value();
    replace(
        &mut review,
        "input",
        Json::object([
            ("kind", Json::text("review")),
            ("lane", Json::text("parent")),
            ("checkpoint", Json::text("saved")),
            ("version", Json::text("b".repeat(64))),
            ("bundle", Json::text("c".repeat(64))),
        ]),
    );
    assert!(matches!(
        configuration(review).unwrap().input,
        Input::Review(_)
    ));
    for (name, replacement) in [
        ("schema", Json::text("mesh.coordinator-receive-config/v2")),
        ("store", Json::text("relative")),
        ("allocations", Json::text("relative")),
        ("allocation", Json::text("C".repeat(32))),
        ("allocation", Json::text("short")),
        ("offer", Json::text("not json")),
        ("offer", Json::text(" ".repeat(8193))),
        ("input", Json::object([("kind", Json::text("manifest"))])),
    ] {
        let mut v = value();
        replace(&mut v, name, replacement);
        assert!(configuration(v).is_err(), "{name}");
    }
    for name in ["manifest", "command", "output_path", "approve"] {
        let Json::Object(mut fields) = value() else {
            panic!()
        };
        fields.push((name.into(), Json::text("no")));
        assert!(configuration(Json::Object(fields)).is_err());
    }
    let Json::Object(mut fields) = value() else {
        panic!()
    };
    fields[0].0 = "offer".into();
    assert!(configuration(Json::Object(fields)).is_err());
}
#[test]
fn receive_output_reports_only_saved_review_identities() {
    let d = RecordDigest::from_bytes([1; 32]);
    assert_eq!(
        outcome(d, d, d),
        Json::object([
            ("schema", Json::text("mesh.coordinator-received-result/v1")),
            ("correlation", Json::text(d.to_string())),
            ("version", Json::text(d.to_string())),
            ("review", Json::text(d.to_string())),
        ])
    );
}
#[test]
fn project_input_uses_saved_bytes_and_protects_original_and_retained_storage() {
    use mesh_keychain::SoftwareActorCustody;
    use std::{fs, os::unix::fs::PermissionsExt as _};
    let root = std::env::temp_dir().join(format!(
        "mesh-coordinator-receive-input-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    for name in ["source", "metadata", "results"] {
        let p = root.join(name);
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(root.join("source/note.txt"), b"saved input\n").unwrap();
    let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
    let attachment = storage.provision(&root.join("source")).unwrap();
    let capture = attachment
        .project()
        .capture_inputs(mesh_daemon::project_attachment::ObservationLimits::default())
        .unwrap();
    let key = SoftwareActorCustody::generate().unwrap();
    let version = attachment
        .project()
        .save_capture(
            attachment.metadata_path(),
            &capture,
            key.public_key().public_key(),
            |p| key.sign(p).map_err(|e| e.to_string()),
        )
        .unwrap()
        .operation();
    fs::write(root.join("source/note.txt"), b"new live work\n").unwrap();
    let mut protected = Vec::new();
    let source = project_input(
        &root.join("metadata"),
        attachment.id(),
        &version.to_string(),
        &mut protected,
    )
    .unwrap();
    let file = source
        .manifest()
        .entries()
        .iter()
        .find_map(|e| match e {
            mesh_daemon::fleet::RemoteInputEntry::File { chunks, .. } => Some(chunks),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        file.iter()
            .flat_map(|c| source.read_chunk(c.digest).unwrap())
            .collect::<Vec<_>>(),
        b"saved input\n"
    );
    for store in [
        root.join("source"),
        attachment.metadata_path().to_path_buf(),
    ] {
        assert!(RemoteInputDestination::admit(
            &store,
            ProtectedWorkspaceRoot::inspect(&store).unwrap(),
            &root.join("results"),
            ProtectedWorkspaceRoot::inspect(&root.join("results")).unwrap(),
            &protected
        )
        .is_err());
    }
    assert_eq!(
        fs::read(root.join("source/note.txt")).unwrap(),
        b"new live work\n"
    );
    assert!(project_input(
        &root.join("metadata"),
        attachment.id(),
        &"f".repeat(64),
        &mut Vec::new()
    )
    .is_err());
    drop(source);
    drop(attachment);
    drop(storage);
    fs::remove_dir_all(root).unwrap();
}
