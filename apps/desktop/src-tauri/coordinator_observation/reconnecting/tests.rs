use super::*;
fn value() -> Json {
    Json::object([
        (
            "schema",
            Json::text("mesh.coordinator-input-reconnect-config/v1"),
        ),
        ("connection", super::super::tests::value()),
        (
            "input",
            Json::object([
                ("kind", Json::text("project")),
                ("storage", Json::text("/private/metadata")),
                ("project", Json::text("ab".repeat(32))),
                ("version", Json::text("cd".repeat(32))),
            ]),
        ),
    ])
}
#[test]
fn reconnect_configuration_refuses_new_authority_or_authored_manifest() {
    assert!(configuration(value()).is_ok());
    for key in [
        "assignment",
        "lease_until_ms",
        "command",
        "allocation",
        "manifest",
    ] {
        let Json::Object(mut fields) = value() else {
            unreachable!()
        };
        fields.push((key.into(), Json::text("replacement")));
        assert!(configuration(Json::Object(fields)).is_err());
    }
    let Json::Object(mut fields) = value() else {
        unreachable!()
    };
    fields[0].1 = Json::text("mesh.coordinator-start-config/v1");
    assert!(configuration(Json::Object(fields)).is_err());
}
#[test]
fn reconnect_argument_requires_explicit_operation_and_absolute_private_config() {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        parse(&args(&[
            "--coordinator",
            "reconnect-input",
            "/private/config"
        ]))
        .unwrap(),
        Some((Action::ReconnectInput, PathBuf::from("/private/config")))
    );
    assert!(parse(&args(&["--coordinator", "reconnect-input", "relative"])).is_err());
    assert!(parse(&args(&[
        "--coordinator",
        "reconnect-input",
        "/private/config",
        "retry"
    ]))
    .is_err());
}
