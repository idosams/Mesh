//! Native-only SSH transport. Assignment authority remains in the signed Mesh protocol.
use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SUBSYSTEM: &str = "mesh-worker-v1";
fn refused() -> io::Error {
    io::Error::other("native SSH connection unavailable")
}

/// Explicit native host policy. This is never decoded from a renderer, task, or worker reply.
/// The operator must provision the fixed mesh-worker-v1 SSH subsystem separately.
pub struct NativeSshDestination {
    host: String,
    account: String,
    port: u16,
    identity: PathBuf,
    known_hosts: PathBuf,
    identity_metadata: Metadata,
    hosts_metadata: Metadata,
}
fn atom(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
#[allow(unsafe_code)]
fn private_file(path: &Path) -> io::Result<Metadata> {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    let text = path.to_str().ok_or_else(refused)?;
    if !path.is_absolute()
        || text.len() > 4096
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
    {
        return Err(refused());
    }
    let m = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid takes no pointers and only reads the effective native user identifier.
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { geteuid() }
        || m.mode() & 0o077 != 0
        || m.len() == 0
        || m.len() > 1_048_576
    {
        return Err(refused());
    }
    Ok(m)
}
fn unchanged(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
impl NativeSshDestination {
    /// Admit explicit DNS/IPv4 host, account and existing private files. IPv6 literals and arbitrary
    /// SSH aliases/options are not supported. Files remain operator-owned; no trust is enrolled here.
    pub fn admit(
        host: &str,
        account: &str,
        port: u16,
        identity: &Path,
        known_hosts: &Path,
    ) -> io::Result<Self> {
        if !atom(host)
            || !atom(account)
            || account.len() > 64
            || port == 0
            || identity == known_hosts
        {
            return Err(refused());
        }
        Ok(Self {
            host: host.into(),
            account: account.into(),
            port,
            identity: identity.into(),
            known_hosts: known_hosts.into(),
            identity_metadata: private_file(identity)?,
            hosts_metadata: private_file(known_hosts)?,
        })
    }
    fn verify(&self) -> io::Result<()> {
        if !unchanged(&self.identity_metadata, &private_file(&self.identity)?)
            || !unchanged(&self.hosts_metadata, &private_file(&self.known_hosts)?)
        {
            return Err(refused());
        }
        Ok(())
    }
    fn command(&self) -> Command {
        let mut c = Command::new("/usr/bin/ssh");
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C");
        c.args(["-F", "/dev/null", "-T", "-s"]);
        for option in [
            "BatchMode=yes",
            "StrictHostKeyChecking=yes",
            "UpdateHostKeys=no",
            "GlobalKnownHostsFile=/dev/null",
            "VerifyHostKeyDNS=no",
            "IdentityAgent=none",
            "IdentitiesOnly=yes",
            "PreferredAuthentications=publickey",
            "PasswordAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "GSSAPIAuthentication=no",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ClearAllForwardings=yes",
            "PermitLocalCommand=no",
            "ProxyCommand=none",
            "ProxyJump=none",
            "ControlMaster=no",
            "ControlPath=none",
            "ConnectionAttempts=1",
            "ConnectTimeout=10",
            "ServerAliveInterval=10",
            "ServerAliveCountMax=1",
        ] {
            c.args(["-o", option]);
        }
        c.arg("-o")
            .arg(format!("UserKnownHostsFile={}", self.known_hosts.display()));
        c.arg("-i")
            .arg(&self.identity)
            .arg("-p")
            .arg(self.port.to_string())
            .arg("-l")
            .arg(&self.account);
        c.arg("--").arg(&self.host).arg(SUBSYSTEM);
        c
    }
    /// Open one local SSH client. Host authentication is enforced by OpenSSH; Mesh peer proofs are
    /// still required before any input transfer. The budget bounds pipe I/O, not OS process spawn.
    /// Native configuration files must remain under operator control throughout the connection.
    pub fn connect(&self, budget: Duration) -> io::Result<NativeSshConnection> {
        self.verify()?;
        let connection = NativeSshConnection::spawn(self.command(), budget)?;
        self.verify()?;
        Ok(connection)
    }
}

struct Client(Child);
impl Drop for Client {
    fn drop(&mut self) {
        // Reap only the exact child owned by this handle. Never signal a remembered PID or worker.
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
/// One bounded pipe direction. The original absolute deadline is never renewed by progress.
pub struct SshPipe {
    file: File,
    deadline: Instant,
}
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    returned: i16,
}
#[allow(unsafe_code)]
fn nonblocking(fd: RawFd) -> io::Result<()> {
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    // SAFETY: Darwin F_GETFL/F_SETFL act on a live borrowed fd; O_NONBLOCK is 0x4.
    let flags = unsafe { fcntl(fd, 3) };
    if flags < 0 || unsafe { fcntl(fd, 4, flags | 0x4) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "SSH connection deadline"))
}
#[allow(unsafe_code)]
fn ready(fd: RawFd, event: i16, deadline: Instant) -> io::Result<()> {
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, count: u32, timeout: i32) -> i32;
    }
    loop {
        let ms = remaining(deadline)?
            .as_millis()
            .saturating_add(1)
            .min(300_000) as i32;
        let mut p = PollFd {
            fd,
            events: event,
            returned: 0,
        };
        // SAFETY: one initialized Darwin pollfd, valid borrowed fd and bounded timeout.
        let result = unsafe { poll(&mut p, 1, ms) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io::Error::last_os_error());
        }
    }
}
impl Read for SshPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            remaining(self.deadline)?;
            match self.file.read(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    ready(self.file.as_raw_fd(), 1, self.deadline)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                r => return r,
            }
        }
    }
}
impl Write for SshPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            remaining(self.deadline)?;
            match self.file.write(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    ready(self.file.as_raw_fd(), 4, self.deadline)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                r => return r,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        remaining(self.deadline)?;
        Ok(())
    }
}
/// Owns a local client and bounded streams, not the resident worker. Drop terminates/reaps only this
/// client. Neither its exit status nor EOF establishes assignment completion or permission to retry.
pub struct NativeSshConnection {
    client: Client,
    input: Option<SshPipe>,
    output: SshPipe,
    stop: Arc<AtomicBool>,
    drained: Arc<AtomicU64>,
    diagnostics: Option<JoinHandle<()>>,
}
impl NativeSshConnection {
    fn spawn(mut command: Command, budget: Duration) -> io::Result<Self> {
        if budget.is_zero() || budget > Duration::from_secs(300) {
            return Err(refused());
        }
        let deadline = Instant::now().checked_add(budget).ok_or_else(refused)?;
        let mut client = Client(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
        );
        let input = File::from(std::os::fd::OwnedFd::from(
            client.0.stdin.take().ok_or_else(refused)?,
        ));
        let output = File::from(std::os::fd::OwnedFd::from(
            client.0.stdout.take().ok_or_else(refused)?,
        ));
        let mut stderr = File::from(std::os::fd::OwnedFd::from(
            client.0.stderr.take().ok_or_else(refused)?,
        ));
        for fd in [input.as_raw_fd(), output.as_raw_fd(), stderr.as_raw_fd()] {
            nonblocking(fd)?;
        }
        remaining(deadline)?;
        let stop = Arc::new(AtomicBool::new(false));
        let drained = Arc::new(AtomicU64::new(0));
        let (stop_reader, count) = (stop.clone(), drained.clone());
        let diagnostics = std::thread::Builder::new()
            .name("mesh-ssh-diagnostics".into())
            .spawn(move || {
                // Retain no task/host/key text. Fixed memory even when a peer floods diagnostics.
                let mut bytes = [0; 4096];
                while !stop_reader.load(Ordering::Acquire) && Instant::now() < deadline {
                    match stderr.read(&mut bytes) {
                        Ok(0) => break,
                        Ok(n) => {
                            let _ = count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                                Some(v.saturating_add(n as u64))
                            });
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            client,
            input: Some(SshPipe {
                file: input,
                deadline,
            }),
            output: SshPipe {
                file: output,
                deadline,
            },
            stop,
            drained,
            diagnostics: Some(diagnostics),
        })
    }
    /// Borrow both pipe directions for existing bounded Mesh frame/transfer APIs.
    pub fn streams(&mut self) -> io::Result<(&mut SshPipe, &mut SshPipe)> {
        Ok((&mut self.output, self.input.as_mut().ok_or_else(refused)?))
    }
    /// Send EOF to this connection without sending worker cancellation or dropping output.
    pub fn close_input(&mut self) {
        self.input.take();
    }
    /// Output remains readable after closing input, until the same original deadline.
    pub fn output(&mut self) -> &mut SshPipe {
        &mut self.output
    }
    /// Diagnostic byte count only. No raw diagnostic contents are exposed or retained.
    pub fn discarded_diagnostic_bytes(&self) -> u64 {
        self.drained.load(Ordering::Relaxed)
    }
    /// Local SSH process status only; never interpreted as remote task disposition.
    pub fn client_status(&mut self) -> io::Result<Option<ExitStatus>> {
        self.client.0.try_wait()
    }
}
impl Drop for NativeSshConnection {
    fn drop(&mut self) {
        self.close_input();
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.diagnostics.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests;
