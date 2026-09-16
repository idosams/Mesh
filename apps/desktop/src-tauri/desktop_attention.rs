//! A private, process-local way for a later Mesh launch to reveal the owning desktop window.
//!
//! The daemon endpoint remains the single authority lease. This second socket carries no workspace
//! method and no data: after the owner has acquired that lease it listens for one exact datagram,
//! then schedules the existing window-reveal action on Tauri's main thread. A contender that cannot
//! acquire the daemon lease may request attention without reading or changing workspace state.

use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// Never make this path longer than the authoritative `daemon.sock` beside it. A runtime directory
// valid for daemon ownership must also be valid for the non-authoritative attention endpoint.
const SOCKET_NAME: &str = "attn.sock";
const SHOW_WINDOW: &[u8] = b"mesh.desktop.show/1";
const LISTENER_POLL: Duration = Duration::from_millis(250);
const REQUEST_POLL: Duration = Duration::from_millis(25);
const REQUEST_DEADLINE: Duration = Duration::from_millis(750);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

/// The attention listener owned by the same process as the daemon endpoint.
pub struct DesktopAttentionServer {
    path: PathBuf,
    identity: SocketIdentity,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl DesktopAttentionServer {
    /// Bind below the already-secured runtime directory.
    ///
    /// The caller must hold the daemon endpoint lease. That lease is what makes removal of a stale
    /// socket safe; this socket is deliberately not a second authority mechanism.
    pub fn bind(
        runtime_directory: &Path,
        reveal: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> io::Result<Self> {
        let path = runtime_directory.join(SOCKET_NAME);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(&path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the desktop attention endpoint is not a socket",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let socket = UnixDatagram::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        socket.set_read_timeout(Some(LISTENER_POLL))?;
        let metadata = fs::symlink_metadata(&path)?;
        let identity = SocketIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            let mut message = [0_u8; 64];
            while !thread_stop.load(Ordering::SeqCst) {
                match socket.recv(&mut message) {
                    Ok(length) if &message[..length] == SHOW_WINDOW => reveal(),
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            path,
            identity,
            stop,
            join: Some(join),
        })
    }
}

impl Drop for DesktopAttentionServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        let remove = fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.file_type().is_socket()
                && metadata.dev() == self.identity.device
                && metadata.ino() == self.identity.inode
        });
        if remove {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Ask the process holding the daemon lease to reveal its existing window.
///
/// A short retry closes the startup interval after the owner binds the daemon endpoint but before
/// it finishes installing this non-authoritative listener. Older builds never create the listener;
/// they fall through to the caller's visible compatibility message after the same bounded wait.
pub fn request_existing_desktop_attention(runtime_directory: &Path) -> io::Result<()> {
    let path = runtime_directory.join(SOCKET_NAME);
    let started = Instant::now();
    loop {
        let result = (|| {
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_socket() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the desktop attention endpoint is not a socket",
                ));
            }
            let socket = UnixDatagram::unbound()?;
            socket.connect(&path)?;
            let written = socket.send(SHOW_WINDOW)?;
            if written != SHOW_WINDOW.len() {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "the desktop attention request was incomplete",
                ));
            }
            Ok(())
        })();
        match result {
            Ok(()) => return Ok(()),
            Err(error)
                if started.elapsed() < REQUEST_DEADLINE
                    && matches!(
                        error.kind(),
                        io::ErrorKind::NotFound
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::WouldBlock
                    ) =>
            {
                std::thread::sleep(REQUEST_POLL);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    fn scratch(name: &str) -> PathBuf {
        PathBuf::from(format!("/tmp/mdatt-{name}-{}", std::process::id()))
    }

    #[test]
    fn attention_never_has_a_stricter_unix_path_limit_than_daemon_ownership() {
        assert!(SOCKET_NAME.len() <= "daemon.sock".len());
    }

    #[test]
    fn a_contender_reveals_the_owner_without_carrying_workspace_data() {
        let root = scratch("reveal");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("runtime directory");
        let calls = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&calls);
        let server = DesktopAttentionServer::bind(
            &root,
            Arc::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .expect("attention server");

        request_existing_desktop_attention(&root).expect("request attention");
        for _ in 0..20 {
            if calls.load(Ordering::SeqCst) == 1 {
                break;
            }
            std::thread::sleep(REQUEST_POLL);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let metadata = fs::symlink_metadata(root.join(SOCKET_NAME)).expect("socket metadata");
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

        drop(server);
        assert!(!root.join(SOCKET_NAME).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_preserves_a_replacement_at_the_attention_path() {
        let root = scratch("replacement");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("runtime directory");
        let server = DesktopAttentionServer::bind(&root, Arc::new(|| {})).expect("server");
        let path = root.join(SOCKET_NAME);
        fs::remove_file(&path).expect("unlink owned socket name");
        fs::write(&path, b"foreign").expect("replacement");

        drop(server);
        assert_eq!(fs::read(&path).expect("replacement retained"), b"foreign");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_non_socket_attention_path_is_refused_without_removal() {
        let root = scratch("unsafe");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("runtime directory");
        let path = root.join(SOCKET_NAME);
        fs::write(&path, b"foreign").expect("foreign path");

        let error = DesktopAttentionServer::bind(&root, Arc::new(|| {}))
            .err()
            .expect("unsafe path refusal");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&path).expect("foreign path retained"), b"foreign");
        let _ = fs::remove_dir_all(root);
    }
}
