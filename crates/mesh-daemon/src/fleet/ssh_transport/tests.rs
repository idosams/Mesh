use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::sync::atomic::AtomicUsize;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-ssh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["identity", "known-hosts"] {
            let path = root.join(name);
            std::fs::write(&path, b"test placeholder, never a credential\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        Self(root)
    }
    fn admit(&self) -> NativeSshDestination {
        NativeSshDestination::admit(
            "worker.example.invalid",
            "worker",
            22,
            &self.0.join("identity"),
            &self.0.join("known-hosts"),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn native_configuration_refuses_injection_links_public_and_changed_files() {
    let f = Fixture::new();
    for host in [
        "-oProxyCommand=evil",
        "host name",
        "host;command",
        "host\noption",
        "user@host",
        "::1",
        "",
    ] {
        assert!(NativeSshDestination::admit(
            host,
            "worker",
            22,
            &f.0.join("identity"),
            &f.0.join("known-hosts")
        )
        .is_err());
    }
    let destination = f.admit();
    destination.verify().unwrap();
    std::fs::write(f.0.join("known-hosts"), b"changed host trust").unwrap();
    assert!(destination.verify().is_err());
    std::fs::set_permissions(f.0.join("identity"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(NativeSshDestination::admit(
        "host",
        "user",
        22,
        &f.0.join("identity"),
        &f.0.join("known-hosts")
    )
    .is_err());
    symlink(f.0.join("known-hosts"), f.0.join("link")).unwrap();
    assert!(private_file(&f.0.join("link")).is_err());
    assert!(private_file(Path::new("relative")).is_err());
}
#[test]
fn installed_ssh_effective_policy_is_noninteractive_and_fixed_subsystem() {
    let f = Fixture::new();
    let destination = f.admit();
    let command = destination.command();
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args.last().unwrap(), &std::ffi::OsStr::new(SUBSYSTEM));
    assert!(command
        .get_envs()
        .all(|(name, _)| name == "PATH" || name == "LC_ALL"));
    let mut inspection = Command::new("/usr/bin/ssh");
    inspection
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("-G")
        .args(args);
    // -G only prints effective configuration; this test never contacts a host or reads a key.
    let mut connection = NativeSshConnection::spawn(inspection, Duration::from_secs(10)).unwrap();
    connection.close_input();
    let mut output = String::new();
    connection.output().read_to_string(&mut output).unwrap();
    for expected in [
        "batchmode yes",
        "stricthostkeychecking true",
        "forwardagent no",
        "forwardx11 no",
        "clearallforwardings yes",
        "permitlocalcommand no",
        "identitiesonly yes",
        "identityagent none",
        "passwordauthentication no",
        "kbdinteractiveauthentication no",
        "sessiontype subsystem",
        "updatehostkeys false",
    ] {
        assert!(
            output.lines().any(|line| line == expected),
            "effective policy missing {expected}"
        );
    }
}
#[test]
fn private_paths_with_spaces_remain_single_literal_ssh_files() {
    let f = Fixture::new();
    let directory = f.0.join("Application Support");
    std::fs::create_dir(&directory).unwrap();
    let identity = directory.join("worker identity");
    let hosts = directory.join("known hosts");
    std::fs::rename(f.0.join("identity"), &identity).unwrap();
    std::fs::rename(f.0.join("known-hosts"), &hosts).unwrap();
    let destination =
        NativeSshDestination::admit("worker.example.invalid", "worker", 22, &identity, &hosts)
            .unwrap();
    destination.verify().unwrap();
    let command = destination.command();
    let args: Vec<_> = command.get_args().collect();
    let identity_argument = args.iter().position(|arg| *arg == "-i").unwrap() + 1;
    assert_eq!(args[identity_argument], identity.as_os_str());
    let hosts_option = format!("UserKnownHostsFile=\"{}\"", hosts.display());
    assert_eq!(
        args.iter()
            .filter(|arg| **arg == hosts_option.as_str())
            .count(),
        1
    );

    // Inspect the installed parser with existing placeholder files, without contacting any host.
    let mut inspection = Command::new("/usr/bin/ssh");
    inspection
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("-G")
        .args(args);
    let mut connection = NativeSshConnection::spawn(inspection, Duration::from_secs(10)).unwrap();
    connection.close_input();
    let mut output = String::new();
    connection.output().read_to_string(&mut output).unwrap();
    let identities: Vec<_> = output
        .lines()
        .filter_map(|line| line.strip_prefix("identityfile "))
        .collect();
    assert_eq!(identities, vec![identity.to_str().unwrap()]);
    let host_files: Vec<_> = output
        .lines()
        .filter_map(|line| line.strip_prefix("userknownhostsfile "))
        .collect();
    assert_eq!(host_files, vec![hosts.to_str().unwrap()]);
    std::fs::write(&hosts, b"changed trust after admission").unwrap();
    assert!(destination.verify().is_err());
}

#[test]
fn private_paths_still_refuse_ssh_expansion_quotes_and_control_characters() {
    let f = Fixture::new();
    for name in [
        "percent%h",
        "tilde~",
        "quote\"",
        "single'",
        "back\\slash",
        "tab\t",
        "newline\n",
        "dollar${HOME}",
    ] {
        let path = f.0.join(name);
        std::fs::write(&path, b"placeholder").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(private_file(&path).is_err(), "unsupported path admitted");
    }
}

fn pipe(budget: Duration) -> (SshPipe, UnixStream) {
    let (stream, peer) = UnixStream::pair().unwrap();
    let file = File::from(std::os::fd::OwnedFd::from(stream));
    nonblocking(file.as_raw_fd()).unwrap();
    (
        SshPipe {
            file,
            deadline: Instant::now() + budget,
        },
        peer,
    )
}
#[test]
fn idle_reads_and_blocked_writes_obey_absolute_deadline() {
    let (mut read, _peer) = pipe(Duration::from_millis(30));
    let start = Instant::now();
    assert_eq!(
        read.read(&mut [0]).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    let (mut write, _peer) = pipe(Duration::from_millis(30));
    let start = Instant::now();
    assert_eq!(
        write.write_all(&vec![0; 1_048_576]).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[test]
fn final_buffered_bytes_survive_close_and_progress_never_renews_budget() {
    let (mut stream, mut peer) = pipe(Duration::from_secs(1));
    peer.write_all(b"saved reply").unwrap();
    drop(peer);
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"saved reply");
    let (mut stream, mut peer) = pipe(Duration::from_millis(30));
    peer.write_all(b"x").unwrap();
    stream.read_exact(&mut [0]).unwrap();
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(
        stream.write(b"y").unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}
#[test]
fn real_child_drains_stderr_and_half_close_preserves_reply() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "i=0; while [ $i -lt 8192 ]; do printf 'private diagnostic text\n' >&2; i=$((i+1)); done; exec /bin/cat"]);
    let mut connection = NativeSshConnection::spawn(command, Duration::from_secs(10)).unwrap();
    let (read, write) = connection.streams().unwrap();
    write.write_all(b"immutable input").unwrap();
    let mut reply = [0; 15];
    read.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"immutable input");
    assert!(connection.discarded_diagnostic_bytes() > 65_536);
    connection.close_input();
    assert!(connection.streams().is_err());
    assert_eq!(connection.output().read(&mut [0]).unwrap(), 0);
}
#[test]
fn invalid_budget_refuses_before_process_spawn() {
    for budget in [Duration::ZERO, Duration::from_secs(301)] {
        assert!(
            NativeSshConnection::spawn(Command::new("/nonexistent/never-start"), budget).is_err()
        );
    }
}

#[test]
#[allow(unsafe_code)]
fn dropping_connection_reaps_exact_client_and_leaves_independent_process_alive() {
    unsafe extern "C" {
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }
    let mut independent = Client(
        Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let connection =
        NativeSshConnection::spawn(Command::new("/bin/cat"), Duration::from_secs(10)).unwrap();
    let pid = connection.client.0.id() as i32;
    drop(connection);
    let mut status = 0;
    // SAFETY: query only our former child; WNOHANG never signals or modifies another process.
    assert_eq!(unsafe { waitpid(pid, &mut status, 1) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(10)); // Darwin ECHILD: already reaped.
    assert!(independent.0.try_wait().unwrap().is_none());
}
