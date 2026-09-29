//! Private create-only resident endpoint. Dropping a listener never deletes retained evidence.
use super::*;
use std::os::unix::fs::FileTypeExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};
const SOCKET: &str = "connection.sock";

/// Native listener bound beneath a pinned, exclusively owned private directory. Existing entries
/// refuse setup; no stale-socket cleanup or replacement is attempted. Peer identity is still proven
/// by signed Mesh dispatch and worker replies, not by the socket pathname alone.
pub struct NativeWorkerEndpoint {
    owner: Arc<Owner>,
    listener: UnixListener,
    path: PathBuf,
    identity: String,
}
fn socket_identity(path: &Path, uid: u32) -> io::Result<String> {
    let m = std::fs::symlink_metadata(path)?;
    if !m.file_type().is_socket() || m.uid() != uid || m.nlink() != 1 || m.mode() & 0o077 != 0 {
        return Err(unavailable());
    }
    Ok(format!(
        "{}:{}:{}:{}",
        m.dev(),
        m.ino(),
        m.st_birthtime(),
        m.st_birthtime_nsec()
    ))
}
#[allow(unsafe_code)]
fn same_user(stream: &UnixStream, uid: u32) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;
    unsafe extern "C" {
        fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
    }
    let (mut peer, mut group) = (u32::MAX, u32::MAX);
    // SAFETY: live socket descriptor and initialized, valid output pointers; no retained pointers.
    if unsafe { getpeereid(stream.as_raw_fd(), &mut peer, &mut group) } != 0 || peer != uid {
        return Err(unavailable());
    }
    Ok(())
}
impl NativeWorkerEndpoint {
    /// Bind only an explicitly selected empty private metadata directory, outside protected roots.
    pub fn bind(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        protected: &[ProtectedWorkspaceRoot],
    ) -> io::Result<Self> {
        let owner = owner(path, expected, protected, None)?;
        if !owner
            .root
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 1)?
            .is_empty()
        {
            return Err(unavailable());
        }
        let path = expected.stable_reference()?.join(SOCKET);
        owner.verify()?;
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
        let identity = socket_identity(&path, owner.uid)?;
        owner.root.sync()?;
        listener.set_nonblocking(true)?;
        let endpoint = Self {
            owner,
            listener,
            path,
            identity,
        };
        endpoint.verify()?;
        Ok(endpoint)
    }
    /// Connect an explicitly configured local bridge to the resident socket. This never takes the
    /// resident ownership lock or creates files. The coordinator still must verify signed worker
    /// replies; same-user transport access alone does not authenticate a Mesh execution identity.
    #[allow(unsafe_code)]
    pub fn connect(
        path: &Path,
        expected: ProtectedWorkspaceRoot,
        budget: Duration,
    ) -> io::Result<NativeWorkerStream> {
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        // SAFETY: geteuid takes no pointers and has no side effects.
        let uid = unsafe { geteuid() };
        if !path.is_absolute() {
            return Err(unavailable());
        }
        let root = PinnedWorkspaceRoot::open(path.to_owned())?;
        let verify = || -> io::Result<()> {
            root.ensure_protected_identity(expected)?;
            root.ensure_namespace_identity()?;
            let m = root.try_clone_directory()?.metadata()?;
            if m.uid() != uid || m.mode() & 0o077 != 0 {
                return Err(unavailable());
            }
            Ok(())
        };
        verify()?;
        let socket = expected.stable_reference()?.join(SOCKET);
        let identity = socket_identity(&socket, uid)?;
        let stream = connect_before(&socket, budget)?;
        same_user(&stream.stream, uid)?;
        verify()?;
        if socket_identity(&socket, uid)? != identity {
            return Err(unavailable());
        }
        Ok(stream)
    }
    /// Revalidate root and exact socket without reconnecting, replacing, or unlinking anything.
    pub fn verify(&self) -> io::Result<()> {
        self.owner.verify()?;
        if socket_identity(&self.path, self.owner.uid)? != self.identity {
            return Err(unavailable());
        }
        self.owner.verify()
    }
    /// Accept at most one same-user connection; absence is nonblocking. The returned stream has a
    /// fixed wall-clock budget for all reads/writes, not a sliding timeout per incoming byte.
    pub fn accept(&self, budget: Duration) -> io::Result<Option<NativeWorkerStream>> {
        self.verify()?;
        match self.listener.accept() {
            Ok((stream, _)) => {
                same_user(&stream, self.owner.uid)?;
                self.verify()?;
                NativeWorkerStream::new(stream, budget).map(Some)
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(error),
        }
    }
}

// Darwin ABI from sys/un.h, sys/socket.h and sys/poll.h. This module is macOS-only.
#[repr(C)]
struct SocketAddress {
    length: u8,
    family: u8,
    path: [u8; 104],
}
#[repr(C)]
struct PollDescriptor {
    fd: i32,
    events: i16,
    returned: i16,
}
#[allow(unsafe_code)]
fn connect_before(path: &Path, budget: Duration) -> io::Result<NativeWorkerStream> {
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    use std::os::unix::ffi::OsStrExt as _;
    unsafe extern "C" {
        fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn connect(fd: i32, address: *const SocketAddress, length: u32) -> i32;
        fn poll(descriptors: *mut PollDescriptor, count: u32, milliseconds: i32) -> i32;
    }
    if budget.is_zero() || budget > Duration::from_secs(300) {
        return Err(unavailable());
    }
    let deadline = Instant::now().checked_add(budget).ok_or_else(unavailable)?;
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.len() >= 104 || bytes.contains(&0) {
        return Err(unavailable());
    }
    let mut address = SocketAddress {
        length: (3 + bytes.len()) as u8,
        family: 1,
        path: [0; 104],
    };
    address.path[..bytes.len()].copy_from_slice(bytes);
    // SAFETY: AF_UNIX/SOCK_STREAM allocate a new descriptor, immediately held by OwnedFd.
    let raw = unsafe { socket(1, 1, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw is a unique live descriptor returned above; ownership transfers exactly once.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: F_SETFD/FD_CLOEXEC act on the live descriptor and take no pointers.
    if unsafe { fcntl(fd.as_raw_fd(), 2, 1) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(true)?;
    // SAFETY: initialized Darwin sockaddr_un layout, correct bounded length, live descriptor.
    let connected = unsafe { connect(stream.as_raw_fd(), &address, u32::from(address.length)) };
    if connected < 0 {
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(35..=37)) {
            return Err(error);
        }
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "worker connect deadline")
                })?;
            let mut descriptor = PollDescriptor {
                fd: stream.as_raw_fd(),
                events: 4,
                returned: 0,
            };
            let milliseconds = remaining.as_millis().saturating_add(1).min(300_000) as i32;
            // SAFETY: one initialized writable pollfd, a live descriptor, finite timeout.
            let result = unsafe { poll(&mut descriptor, 1, milliseconds) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if result == 0 {
                continue;
            }
            if let Some(error) = stream.take_error()? {
                return Err(error);
            }
            stream.peer_addr()?;
            break;
        }
    }
    let result = NativeWorkerStream { stream, deadline };
    result.remaining()?;
    result.stream.set_nonblocking(true)?;
    Ok(result)
}

/// Deadline-bound local transport. Clones share the original absolute deadline.
pub struct NativeWorkerStream {
    stream: UnixStream,
    deadline: Instant,
}
impl NativeWorkerStream {
    fn new(stream: UnixStream, budget: Duration) -> io::Result<Self> {
        if budget.is_zero() || budget > Duration::from_secs(300) {
            return Err(unavailable());
        }
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            deadline: Instant::now().checked_add(budget).ok_or_else(unavailable)?,
        })
    }
    /// Clone only the transport descriptor, preserving the same connection deadline.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            stream: self.stream.try_clone()?,
            deadline: self.deadline,
        })
    }
    /// Close only this connection input after bridge stdin EOF, retaining the resident service.
    pub fn shutdown_write(&self) -> io::Result<()> {
        self.stream.shutdown(std::net::Shutdown::Write)
    }
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "worker connection deadline"))
    }
}
#[allow(unsafe_code)]
fn wait_ready(stream: &UnixStream, deadline: Instant, events: i16) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;
    unsafe extern "C" {
        fn poll(descriptors: *mut PollDescriptor, count: u32, milliseconds: i32) -> i32;
    }
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "worker connection deadline"))?;
        let mut descriptor = PollDescriptor {
            fd: stream.as_raw_fd(),
            events,
            returned: 0,
        };
        let milliseconds = remaining.as_millis().saturating_add(1).min(300_000) as i32;
        // SAFETY: initialized Darwin pollfd, live borrowed descriptor, finite timeout.
        let result = unsafe { poll(&mut descriptor, 1, milliseconds) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}
impl io::Read for NativeWorkerStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        // Darwin rejects SO_RCVTIMEO updates after full peer closure even while bytes remain
        // buffered. Nonblocking I/O plus readiness polling preserves those bytes and clean EOF.
        loop {
            self.remaining()?;
            match self.stream.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(&self.stream, self.deadline, 1)?
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}
impl io::Write for NativeWorkerStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            self.remaining()?;
            match io::Write::write(&mut self.stream, bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(&self.stream, self.deadline, 4)?
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.remaining()?;
        io::Write::flush(&mut self.stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mesh-w-endpoint-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn token(&self) -> ProtectedWorkspaceRoot {
            ProtectedWorkspaceRoot::inspect(&self.0).unwrap()
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn endpoint_is_exclusive_create_only_and_preserves_replacement() {
        let f = Folder::new();
        let endpoint = NativeWorkerEndpoint::bind(&f.0, f.token(), &[]).unwrap();
        assert!(NativeWorkerEndpoint::bind(&f.0, f.token(), &[]).is_err());
        assert!(endpoint.accept(Duration::from_secs(1)).unwrap().is_none());
        let _client =
            NativeWorkerEndpoint::connect(&f.0, f.token(), Duration::from_secs(1)).unwrap();
        assert!(endpoint.accept(Duration::from_secs(1)).unwrap().is_some());
        std::fs::rename(f.0.join(SOCKET), f.0.join("retained.sock")).unwrap();
        std::fs::write(f.0.join(SOCKET), b"unrelated bytes").unwrap();
        assert!(endpoint.accept(Duration::from_secs(1)).is_err());
        drop(endpoint);
        assert_eq!(std::fs::read(f.0.join(SOCKET)).unwrap(), b"unrelated bytes");
        assert!(NativeWorkerEndpoint::bind(&f.0, f.token(), &[]).is_err());
    }
    #[test]
    #[allow(unsafe_code)]
    fn connected_descriptor_is_close_on_exec() {
        use std::os::fd::AsRawFd as _;
        unsafe extern "C" {
            fn fcntl(fd: i32, command: i32, ...) -> i32;
        }
        let f = Folder::new();
        let endpoint = NativeWorkerEndpoint::bind(&f.0, f.token(), &[]).unwrap();
        let client =
            NativeWorkerEndpoint::connect(&f.0, f.token(), Duration::from_secs(1)).unwrap();
        assert!(endpoint.accept(Duration::from_secs(1)).unwrap().is_some());
        // SAFETY: Darwin F_GETFD reads descriptor flags and takes no variadic argument.
        let flags = unsafe { fcntl(client.stream.as_raw_fd(), 1) };
        assert!(flags >= 0);
        assert_eq!(
            flags & 1,
            1,
            "the connection must not leak across provider exec"
        );
    }
    #[test]
    fn buffered_final_reply_and_clean_eof_survive_peer_closure() {
        use std::io::Write as _;
        let (mut peer, stream) = UnixStream::pair().unwrap();
        let mut stream = NativeWorkerStream::new(stream, Duration::from_secs(1)).unwrap();
        peer.write_all(b"final reply").unwrap();
        drop(peer);
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"final reply");
    }
    #[test]
    fn idle_read_obeys_the_original_deadline() {
        let (_peer, stream) = UnixStream::pair().unwrap();
        let mut stream = NativeWorkerStream::new(stream, Duration::from_millis(20)).unwrap();
        let started = Instant::now();
        assert_eq!(
            stream.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn blocked_write_obeys_the_original_deadline() {
        use std::io::Write as _;
        let (_peer, stream) = UnixStream::pair().unwrap();
        let mut stream = NativeWorkerStream::new(stream, Duration::from_millis(20)).unwrap();
        let started = Instant::now();
        assert_eq!(
            stream.write_all(&vec![0; 1024 * 1024]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn stream_clones_do_not_renew_the_deadline() {
        let (left, _right) = UnixStream::pair().unwrap();
        let stream = NativeWorkerStream::new(left, Duration::from_millis(10)).unwrap();
        let mut clone = stream.try_clone().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            clone.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            io::Write::write(&mut clone, b"x").unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }
}
