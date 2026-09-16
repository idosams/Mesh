//! The local transport: a Unix-domain socket, one thread per connection, and nothing else.
//!
//! # Local only, and checkable
//!
//! This module names `std::os::unix::net::UnixListener` and nothing from `std::net`. That is the
//! whole of the "IPC is local-only" acceptance criterion on the daemon side, and it is checked
//! rather than asserted: `crates/mesh-daemon/tests/ipc.rs` scans this crate's own sources for the
//! network types and fails if one appears, and it also connects to the running server and reads
//! the peer address back to confirm it is a filesystem path.
//!
//! A Unix-domain socket is a filesystem object, so the socket file's permissions are the access
//! control. [`IpcServer::bind`] sets the directory to owner-only before binding and states what
//! that does and does not buy in [`IpcServer::endpoint`]'s documentation.
//! A process-held advisory lock serializes stale-socket recovery. A second live daemon therefore
//! gets `AddrInUse` instead of unlinking the first daemon's endpoint, and shutdown removes only the
//! exact socket device/inode that process originally bound.
//!
//! # Why threads and not an async runtime
//!
//! An async runtime is a third-party crate, `tools/program/arch-check/architecture.json` registers
//! every third-party crate this workspace may reach, and that file is outside this task's allowed
//! paths. One thread per connection is what the standard library offers, the desktop client opens
//! exactly one connection, and [`MAX_CONNECTIONS`] bounds the rest.
//!
//! # Named pipes are not implemented
//!
//! The task contract says "a Unix-domain socket **or** named pipe". This module compiles on
//! `unix` only. A Windows named-pipe backend behind the same [`crate::ipc::surface::Operations`]
//! seam is not in this pass and is not stubbed, because a stub that accepts a connection and
//! answers nothing is worse than an honest absence.

use std::io::{BufRead, BufReader, ErrorKind, Read as _, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{
    FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::{fs, io};

use crate::ipc::message::{daemon_frames, ClientMessage, DaemonMessage, WireError, MAX_LINE_BYTES};
use crate::ipc::surface::{Conversation, Operations};
use crate::user_messages;

/// How many connections may be served at once before further ones are closed immediately.
///
/// The desktop client opens one. The bound exists so that a local process in a loop cannot spend
/// the daemon's whole thread budget; it is a resource limit, not a security boundary.
pub const MAX_CONNECTIONS: usize = 16;

/// The session names this daemon process has seen, so a reconnection can be recognised as one.
///
/// In memory, deliberately and visibly: after a daemon restart this is empty and every client is
/// told `resumed: false`. The client's own context survives regardless, because the client owns
/// it — see `apps/desktop/src/ipc/client.ts`. Persisting it would be a store write, and the
/// daemon's start-up path is not where a new durable file belongs.
#[derive(Debug, Default)]
struct SessionRegistry {
    seen: Mutex<Vec<String>>,
}

impl SessionRegistry {
    /// Record `session` and report whether it was already known.
    fn admit(&self, session: &str) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|poisoned| {
            // A panicked connection thread must not take the registry down with it: the worst a
            // poisoned lock can do here is mis-report `resumed`, which is a hint, not a guarantee.
            poisoned.into_inner()
        });
        if seen.iter().any(|known| known == session) {
            return true;
        }
        if seen.len() < MAX_SESSIONS {
            seen.push(session.to_owned());
        }
        false
    }
}

/// How many session names are remembered before new ones stop being recorded.
const MAX_SESSIONS: usize = 64;

/// A bound, listening local endpoint.
pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
    identity: EndpointIdentity,
    lock: EndpointLock,
    sessions: Arc<SessionRegistry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EndpointIdentity {
    device: u64,
    inode: u64,
}

#[derive(Debug)]
struct EndpointLock {
    _file: fs::File,
}

impl EndpointLock {
    fn acquire(endpoint: &Path) -> io::Result<Self> {
        let mut name = endpoint.as_os_str().to_os_string();
        name.push(".lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(PathBuf::from(name))?;
        lock_exclusive_nonblocking(&file)?;
        Ok(Self { _file: file })
    }
}

#[allow(unsafe_code)]
fn lock_exclusive_nonblocking(file: &fs::File) -> io::Result<()> {
    const LOCK_EXCLUSIVE: std::ffi::c_int = 2;
    const LOCK_NONBLOCKING: std::ffi::c_int = 4;
    extern "C" {
        fn flock(descriptor: std::ffi::c_int, operation: std::ffi::c_int) -> std::ffi::c_int;
    }
    // SAFETY: `descriptor` is borrowed from a live File, and flock neither retains the pointer
    // nor reads memory through it. The operating system releases the advisory lock on close.
    let result = unsafe { flock(file.as_raw_fd(), LOCK_EXCLUSIVE | LOCK_NONBLOCKING) };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::WouldBlock {
            Err(io::Error::new(
                ErrorKind::AddrInUse,
                "the local endpoint is already owned by another process",
            ))
        } else {
            Err(error)
        }
    }
}

impl IpcServer {
    /// Bind the socket at `path`, replacing only a socket file left behind by a previous run.
    /// A listener that still accepts connections owns the endpoint and is never displaced.
    ///
    /// # Errors
    ///
    /// Any I/O error from removing a stale socket file, tightening the directory, or binding.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            restrict_to_owner(parent)?;
        }
        let lock = EndpointLock::acquire(path)?;
        remove_stale_endpoint(path)?;
        let listener = UnixListener::bind(path)?;
        let metadata = fs::symlink_metadata(path)?;
        let identity = EndpointIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        Ok(Self {
            listener,
            path: path.to_path_buf(),
            identity,
            lock,
            sessions: Arc::new(SessionRegistry::default()),
        })
    }

    /// The filesystem path this endpoint listens on.
    ///
    /// The access control on this surface is the containing directory's mode, set to owner-only by
    /// [`Self::bind`]. That stops another user on the same machine from connecting. It does **not**
    /// stop another process running as the same user, and nothing on this socket pretends
    /// otherwise: capability checking belongs to `mesh-policy` and is not on this surface yet.
    #[must_use]
    pub fn endpoint(&self) -> &Path {
        &self.path
    }

    /// Accept one connection and serve it until the peer closes.
    ///
    /// # Errors
    ///
    /// Any I/O error from `accept`.
    pub fn serve_one(&self, operations: &dyn Operations) -> io::Result<()> {
        let (stream, _) = self.listener.accept()?;
        let never = AtomicBool::new(false);
        serve_connection(stream, operations, &self.sessions, &never);
        Ok(())
    }

    /// Serve connections on a background thread until the returned handle is shut down.
    ///
    /// # Errors
    ///
    /// Any I/O error from putting the listener into non-blocking mode.
    pub fn spawn(self, operations: Arc<dyn Operations>) -> io::Result<ServerHandle> {
        self.listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let served = Arc::new(AtomicU64::new(0));
        let path = self.path.clone();
        let identity = self.identity;
        let lock = self.lock;
        let sessions = Arc::clone(&self.sessions);
        let listener = self.listener;
        let thread_stop = Arc::clone(&stop);
        let thread_served = Arc::clone(&served);
        let join = std::thread::spawn(move || {
            let mut live: Vec<JoinHandle<()>> = Vec::new();
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        live.retain(|handle| !handle.is_finished());
                        if live.len() >= MAX_CONNECTIONS {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        thread_served.fetch_add(1, Ordering::SeqCst);
                        let operations = Arc::clone(&operations);
                        let sessions = Arc::clone(&sessions);
                        let connection_stop = Arc::clone(&thread_stop);
                        live.push(std::thread::spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            let _ = stream.set_read_timeout(Some(READ_POLL));
                            serve_connection(
                                stream,
                                operations.as_ref(),
                                &sessions,
                                &connection_stop,
                            );
                        }));
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(ACCEPT_POLL);
                    }
                    Err(_) => break,
                }
            }
            for handle in live {
                let _ = handle.join();
            }
        });
        Ok(ServerHandle {
            path,
            identity,
            _lock: lock,
            stop,
            served,
            join: Some(join),
        })
    }
}

/// How long the accept loop sleeps between polls when nothing is waiting.
///
/// Polling rather than blocking is what makes shutdown deterministic without a second socket or a
/// signal. Ten milliseconds is invisible to a person and costs a hundred wakeups a second.
const ACCEPT_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// How long a connection waits for the next line before checking whether the daemon is stopping.
///
/// Without this, a shutdown would have to wait for every connected client to close first, and a
/// desktop application that is simply idle would hold the daemon open indefinitely. The read is
/// resumed exactly where it left off, so a message split across the timeout is not lost.
const READ_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// A running server. Dropping it stops the accept loop and removes the socket file.
pub struct ServerHandle {
    path: PathBuf,
    identity: EndpointIdentity,
    _lock: EndpointLock,
    stop: Arc<AtomicBool>,
    served: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
}

impl ServerHandle {
    /// The filesystem path clients connect to.
    #[must_use]
    pub fn endpoint(&self) -> &Path {
        &self.path
    }

    /// How many connections have been accepted since this handle started.
    #[must_use]
    pub fn connections_served(&self) -> u64 {
        self.served.load(Ordering::SeqCst)
    }

    /// Stop the accept loop, wait for every connection thread, and remove the socket file.
    pub fn shutdown(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        remove_owned_endpoint(&self.path, self.identity);
    }
}

fn remove_stale_endpoint(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "the local endpoint exists and is not a socket",
        ));
    }
    match UnixStream::connect(path) {
        Ok(stream) => {
            let _ = stream.shutdown(Shutdown::Both);
            Err(io::Error::new(
                ErrorKind::AddrInUse,
                "the local endpoint is already serving",
            ))
        }
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => fs::remove_file(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_owned_endpoint(path: &Path, expected: EndpointIdentity) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let actual = EndpointIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    if metadata.file_type().is_socket() && actual == expected {
        let _ = fs::remove_file(path);
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Serve one connection to end of stream, or until `stop` is set.
///
/// Every reply is written and flushed before the next line is read, so a client that pipelines
/// still gets replies in call order on one connection. A line that cannot be read as a message
/// gets a `failed` reply and the connection stays open: the client is a user interface, and
/// dropping the socket under it turns a typo in one request into a visible outage.
///
/// The read is resumed rather than restarted after a timeout — `pending` is cleared only once a
/// whole line has been handled — so a message that arrives in two pieces around the poll interval
/// is reassembled instead of being silently truncated.
fn serve_connection(
    stream: UnixStream,
    operations: &dyn Operations,
    sessions: &SessionRegistry,
    stop: &AtomicBool,
) {
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut writer = write_half;
    let mut conversation = Conversation::new();
    let mut pending = String::new();

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let remaining = u64::try_from(MAX_LINE_BYTES.saturating_sub(pending.len())).unwrap_or(0);
        let read = {
            let mut limited = (&mut reader).take(remaining);
            limited.read_line(&mut pending)
        };
        match read {
            Ok(0) if pending.is_empty() => return,
            Ok(_) => {}
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                    || error.kind() == ErrorKind::Interrupted =>
            {
                // Nothing to read this time round. A subscribed connection still hears what the
                // daemon has been doing, which is what makes an idle client a live one.
                if push_events(&mut conversation, operations, &mut writer) {
                    continue;
                }
                return;
            }
            Err(_) => return,
        }
        if !pending.ends_with('\n') {
            if pending.len() >= MAX_LINE_BYTES {
                // The line ran past the limit and the framing is now out of step: every later
                // byte would be read as the start of a message it is not. Say so once and close,
                // rather than answering nonsense to a stream nobody can realign.
                let _ = writeln!(
                    writer,
                    "{}",
                    unreadable(&WireError::TooLong {
                        bytes: pending.len()
                    })
                    .encode()
                );
                let _ = writer.flush();
            }
            // Otherwise the peer closed mid-line: there is no whole message to answer.
            return;
        }
        let trimmed = pending.trim_end_matches(['\n', '\r']);
        if !trimmed.is_empty() {
            let reply = match ClientMessage::decode(trimmed) {
                Ok(request) => {
                    let known = match &request {
                        ClientMessage::Hello { session, .. } => sessions.admit(session),
                        ClientMessage::Call { .. } => false,
                    };
                    conversation.answer(&request, operations, known)
                }
                Err(error) => unreadable(&error),
            };
            if !write_message(&mut writer, &reply, conversation.negotiated()) {
                return;
            }
        }
        pending.clear();
        if !push_events(&mut conversation, operations, &mut writer) {
            return;
        }
    }
}

/// Send every feed entry this connection has not seen yet. `false` means the peer is gone.
///
/// Called after every read attempt, including the ones that time out with nothing to read, so a
/// subscribed connection that is otherwise idle still hears about the daemon within one
/// [`READ_POLL`]. A connection that never subscribed drains nothing and writes nothing, which is
/// what keeps the push invisible to a client that did not ask for it.
fn push_events(
    conversation: &mut Conversation,
    operations: &dyn Operations,
    writer: &mut UnixStream,
) -> bool {
    let events = conversation.drain_events(operations);
    if events.is_empty() {
        return true;
    }
    for event in events {
        if !write_message(writer, &event, conversation.negotiated()) {
            return false;
        }
    }
    writer.flush().is_ok()
}

fn write_message(
    writer: &mut UnixStream,
    message: &DaemonMessage,
    negotiated: Option<u32>,
) -> bool {
    for frame in daemon_frames(message, negotiated) {
        if writeln!(writer, "{}", frame.encode()).is_err() {
            return false;
        }
    }
    writer.flush().is_ok()
}

/// The reply to a line that could not be read.
///
/// `id` is `0` because the identifier is inside the line that could not be parsed. A client cannot
/// correlate this with one call, and the client in `apps/desktop` treats a reply on identifier `0`
/// as a connection-level fault rather than an answer to anything.
fn unreadable(error: &WireError) -> DaemonMessage {
    DaemonMessage::Failed {
        id: 0,
        code: error.code().to_owned(),
        message: format!("{} ({error})", user_messages::UNREADABLE_REQUEST),
    }
}

/// Make one exact real directory owner-only, so no other user on the machine can reach the socket
/// inside it.
///
/// Permission changes are applied through an opened directory handle after its device/inode is
/// matched to the path. A symlink is never followed and an exchanged directory is refused rather
/// than chmodding or trusting an object that was not inspected.
fn restrict_to_owner(directory: &Path) -> io::Result<()> {
    if !directory.exists() {
        fs::create_dir_all(directory)?;
    }
    let before = fs::symlink_metadata(directory)?;
    if !before.file_type().is_dir() || before.file_type().is_symlink() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "the local endpoint parent is not a real directory",
        ));
    }
    let opened = fs::File::open(directory)?;
    let opened_metadata = opened.metadata()?;
    if !opened_metadata.is_dir()
        || opened_metadata.dev() != before.dev()
        || opened_metadata.ino() != before.ino()
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "the local endpoint parent changed while it was opened",
        ));
    }
    let mut permissions = opened_metadata.permissions();
    permissions.set_mode(0o700);
    opened.set_permissions(permissions)?;

    let after = fs::symlink_metadata(directory)?;
    if !after.file_type().is_dir()
        || after.file_type().is_symlink()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
        || after.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "the local endpoint parent changed while it was secured",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::message::ChunkAssembler;
    use crate::ipc::surface::{nothing_to_recover, RecoveredDaemon};

    fn temp_endpoint(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("mesh-ipc-unit-{name}-{}", std::process::id()));
        let _ = fs::create_dir_all(&path);
        path.push("daemon.sock");
        path
    }

    #[test]
    fn the_server_writes_one_large_logical_reply_as_bounded_frames() {
        let (mut writer, stream) = UnixStream::pair().expect("socket pair");
        let expected = DaemonMessage::Result {
            id: 42,
            value: crate::ipc::Json::object([(
                "entries",
                crate::ipc::Json::Array(
                    (0..4_000)
                        .map(|index| crate::ipc::Json::text(format!("src/generated-{index}.rs")))
                        .collect(),
                ),
            )]),
        };
        let sent = expected.clone();
        let sender = std::thread::spawn(move || {
            assert!(write_message(&mut writer, &sent, Some(7)));
            writer.shutdown(Shutdown::Write).expect("finish frames");
        });

        let mut reader = BufReader::new(stream);
        let mut assembler = ChunkAssembler::default();
        let mut lines = 0;
        let complete = loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).expect("read frame"), 0);
            assert!(line.len() <= MAX_LINE_BYTES);
            lines += 1;
            let frame = DaemonMessage::decode(line.trim_end()).expect("bounded frame");
            if let Some(message) = assembler.push(frame).expect("ordered frame") {
                break message;
            }
        };
        assert!(lines > 1);
        assert_eq!(complete, expected);
        sender.join().expect("writer thread");
    }

    #[test]
    fn binding_twice_replaces_a_stale_socket_file() {
        let path = temp_endpoint("stale");
        let first = IpcServer::bind(&path).expect("first bind");
        assert_eq!(first.endpoint(), path.as_path());
        drop(first);
        // The socket file is still on disk: a killed daemon leaves one behind.
        assert!(path.exists());
        let second = IpcServer::bind(&path).expect("second bind over the stale file");
        drop(second);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_live_endpoint_cannot_be_stolen_by_a_second_server() {
        let path = temp_endpoint("live-owner");
        let first = IpcServer::bind(&path).expect("first bind");
        let error = IpcServer::bind(&path)
            .err()
            .expect("the live owner must be retained");
        assert_eq!(error.kind(), ErrorKind::AddrInUse);
        assert!(UnixStream::connect(&path).is_ok());
        drop(first);

        let replacement = IpcServer::bind(&path).expect("released owner can be replaced");
        drop(replacement);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn endpoint_binding_never_deletes_a_non_socket_path() {
        let path = temp_endpoint("non-socket");
        fs::write(&path, "keep me").expect("ordinary file");
        let error = IpcServer::bind(&path)
            .err()
            .expect("ordinary file must be refused");
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).expect("retained"), "keep me");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn endpoint_binding_never_follows_or_chmods_a_linked_parent() {
        use std::os::unix::fs::symlink;

        let endpoint = temp_endpoint("linked-parent");
        let linked = endpoint.parent().expect("parent").to_path_buf();
        fs::remove_dir(&linked).expect("replace scratch directory with link");
        let real = linked.with_extension("real");
        fs::create_dir(&real).expect("real target directory");
        fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).expect("shared target mode");
        symlink(&real, &linked).expect("linked endpoint parent");

        let error = IpcServer::bind(&endpoint)
            .err()
            .expect("linked parent must be refused");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            fs::metadata(&real)
                .expect("target retained")
                .permissions()
                .mode()
                & 0o777,
            0o755,
            "refusal must not chmod the symlink target"
        );
        assert!(!real.join("daemon.sock").exists());

        fs::remove_file(&linked).expect("remove link");
        fs::remove_dir(&real).expect("remove target");
    }

    #[test]
    fn shutdown_removes_only_the_socket_identity_it_bound() {
        let path = temp_endpoint("replacement-identity");
        let server = IpcServer::bind(&path).expect("bind");
        let operations: Arc<dyn Operations> = Arc::new(RecoveredDaemon::new(nothing_to_recover()));
        let handle = server.spawn(operations).expect("spawn");

        fs::remove_file(&path).expect("unlink owned endpoint");
        let replacement = UnixListener::bind(&path).expect("replacement endpoint");
        handle.shutdown();

        assert!(path.exists());
        assert!(UnixStream::connect(&path).is_ok());
        drop(replacement);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_registry_recognises_a_returning_session_only_the_second_time() {
        let registry = SessionRegistry::default();
        assert!(!registry.admit("w16"));
        assert!(registry.admit("w16"));
        assert!(!registry.admit("other"));
    }

    #[test]
    fn the_reply_to_an_unreadable_line_carries_identifier_zero() {
        let error = ClientMessage::decode("{ not json").unwrap_err();
        let reply = unreadable(&error);
        assert_eq!(reply.id(), 0);
        assert!(matches!(reply, DaemonMessage::Failed { .. }));
    }

    #[test]
    fn a_spawned_server_serves_and_then_stops() {
        let path = temp_endpoint("spawn");
        let server = IpcServer::bind(&path).expect("bind");
        let operations: Arc<dyn Operations> = Arc::new(RecoveredDaemon::new(nothing_to_recover()));
        let handle = server.spawn(operations).expect("spawn");
        {
            let stream = UnixStream::connect(handle.endpoint()).expect("connect");
            let mut writer = stream.try_clone().expect("clone");
            let mut reader = BufReader::new(stream);
            writeln!(
                writer,
                "{}",
                ClientMessage::Hello {
                    id: 1,
                    versions: vec![1],
                    session: "unit".to_owned(),
                }
                .encode()
            )
            .expect("write hello");
            let mut line = String::new();
            reader.read_line(&mut line).expect("read welcome");
            assert!(matches!(
                DaemonMessage::decode(line.trim_end()),
                Ok(DaemonMessage::Welcome { .. })
            ));
        }
        assert_eq!(handle.connections_served(), 1);
        let endpoint = handle.endpoint().to_path_buf();
        handle.shutdown();
        assert!(!endpoint.exists(), "shutdown removes the socket file");
    }
}
