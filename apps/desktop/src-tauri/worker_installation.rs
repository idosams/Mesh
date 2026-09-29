//! Explicit native worker provisioning before the graphical application starts.
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Provision,
    Identity,
}
fn parse(args: &[String]) -> Result<Option<(Action, PathBuf)>, String> {
    if args.first().map(String::as_str) != Some("--worker") {
        return Ok(None);
    }
    if args.len() != 3 {
        return Err("Use --worker provision|identity <existing-private-folder>".into());
    }
    let action = match args[1].as_str() {
        "provision" => Action::Provision,
        "identity" => Action::Identity,
        _ => return Err("Worker action must be provision or identity".into()),
    };
    let root = PathBuf::from(&args[2]);
    if !root.is_absolute() {
        return Err("The worker metadata folder must be absolute".into());
    }
    Ok(Some((action, root)))
}
pub fn run_if_requested() -> Option<Result<(), String>> {
    match parse(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(Some((action, root))) => Some(run(action, root)),
        Ok(None) => None,
        Err(error) => Some(Err(error)),
    }
}
#[cfg(target_os = "macos")]
fn run(action: Action, root: PathBuf) -> Result<(), String> {
    use mesh_crypto::KeyCustody as _;
    use mesh_daemon::fleet::NativeWorkerInstallation;
    use mesh_daemon::{ipc::Json, ProtectedWorkspaceRoot};
    use mesh_keychain::AppleActorCustody;
    use std::io::{self, Write as _};
    const UNAVAILABLE: &str = "The worker installation is unavailable or needs reconciliation";
    // Check the signed application before touching files. No ephemeral fallback and no automatic
    // directory creation: this explicit command operates only on the caller's private metadata root.
    AppleActorCustody::availability()
        .map_err(|_| "Worker identity requires an eligible signed Mesh application")?;
    let expected = ProtectedWorkspaceRoot::inspect(&root).map_err(|_| UNAVAILABLE)?;
    let custody_error = |_| io::Error::other("worker custody unavailable");
    let (installation, _custody) = match action {
        Action::Provision => NativeWorkerInstallation::provision(&root, expected, &[], |account| {
            let key = AppleActorCustody::create(account).map_err(custody_error)?;
            Ok((key.public_key().public_key(), key))
        }),
        Action::Identity => {
            NativeWorkerInstallation::reopen(&root, expected, &[], |account, key| {
                AppleActorCustody::open(account, key).map_err(custody_error)
            })
        }
    }
    .map_err(|_| UNAVAILABLE)?;
    let identity = installation.identity().map_err(|_| UNAVAILABLE)?;
    let encoded = Json::object([
        ("schema", Json::text("mesh.worker-public-identity/v1")),
        (
            "installation",
            Json::text(
                identity
                    .installation()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            ),
        ),
        (
            "worker",
            Json::text(
                identity
                    .worker()
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            ),
        ),
    ])
    .encode();
    let mut output = io::stdout().lock();
    writeln!(output, "{encoded}")
        .and_then(|()| output.flush())
        .map_err(|_| "The worker identity output closed; retained setup needs inspection".into())
}
#[cfg(not(target_os = "macos"))]
fn run(_: Action, _: PathBuf) -> Result<(), String> {
    Err("Persistent native worker provisioning is currently available only on macOS".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).into()).collect()
    }
    #[test]
    fn worker_mode_requires_exact_action_and_absolute_private_root() {
        assert_eq!(
            parse(&args(&["--worker", "provision", "/private/tmp/worker"])).unwrap(),
            Some((Action::Provision, PathBuf::from("/private/tmp/worker")))
        );
        assert_eq!(
            parse(&args(&["--worker", "identity", "/private/tmp/worker"])).unwrap(),
            Some((Action::Identity, PathBuf::from("/private/tmp/worker")))
        );
        for items in [
            vec!["--worker"],
            vec!["--worker", "serve", "/tmp/x"],
            vec!["--worker", "provision", "relative"],
            vec!["--worker", "identity", "/tmp/x", "extra"],
        ] {
            assert!(parse(&args(&items)).is_err());
        }
        assert!(parse(&args(&["--attachment", "watch", "/tmp/x"]))
            .unwrap()
            .is_none());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn unsigned_worker_mode_refuses_before_provisioning_files() {
        // The test binary has no enrolled Mesh application identity. Fail before filesystem work,
        // including when the requested path doesn't exist. This test creates no keychain item.
        assert!(mesh_keychain::AppleActorCustody::availability().is_err());
        for action in [Action::Provision, Action::Identity] {
            assert_eq!(
                run(action, PathBuf::from("/unused-mesh-worker-test-path")).unwrap_err(),
                "Worker identity requires an eligible signed Mesh application"
            );
        }
    }
}
