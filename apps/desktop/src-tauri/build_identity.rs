//! Side-effect-free build identity for exact packaged-executable verification.
use mesh_daemon::ipc::Json;

fn response(args: &[String]) -> Result<Option<String>, &'static str> {
    if args.first().map(String::as_str) != Some("--mesh-build-identity") {
        return Ok(None);
    }
    if args.len() != 1 {
        return Err("--mesh-build-identity accepts no additional arguments");
    }
    Ok(Some(
        Json::object([
            ("schema", Json::text("mesh.desktop-build-identity/v1")),
            ("revision", Json::text(env!("MESH_BUILD_REVISION"))),
            (
                "exact",
                Json::Bool(env!("MESH_BUILD_REVISION") != "development"),
            ),
        ])
        .encode(),
    ))
}

pub fn run_if_requested() -> bool {
    match response(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(None) => false,
        Ok(Some(identity)) => {
            println!("{identity}");
            true
        }
        Err(problem) => {
            eprintln!("Mesh build identity: {problem}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_compiled_identity_and_refuses_ambiguous_invocations() {
        let value = response(&["--mesh-build-identity".into()])
            .unwrap()
            .unwrap();
        let identity = Json::parse(&value).unwrap();
        assert_eq!(
            identity.get("revision"),
            Some(&Json::text(env!("MESH_BUILD_REVISION")))
        );
        assert_eq!(
            identity.get("exact"),
            Some(&Json::Bool(env!("MESH_BUILD_REVISION") != "development"))
        );
        assert_eq!(
            identity.get("schema"),
            Some(&Json::text("mesh.desktop-build-identity/v1"))
        );
        assert!(response(&["--mesh-build-identity".into(), "extra".into()]).is_err());
        assert_eq!(response(&[]).unwrap(), None);
        assert_eq!(response(&["--mesh-attachment".into()]).unwrap(), None);
    }
}
