//! One cooperating native initializer per physical allocation, including within one installation.
use super::*;
use std::fs::File;

pub(super) struct InitializationOwner {
    root: PinnedWorkspaceRoot,
    // Independent open description: cloned descriptors would share and bypass flock ownership.
    _lock: File,
}
impl InitializationOwner {
    #[allow(unsafe_code)]
    pub(super) fn acquire(root: &PinnedWorkspaceRoot) -> io::Result<Self> {
        use std::os::fd::AsRawFd as _;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        private(root)?;
        let lock = root.independent_lock_directory()?;
        // SAFETY: lock owns a live directory descriptor. LOCK_EX|LOCK_NB refuses another
        // cooperating initializer immediately, without changing files or waiting on its work.
        if unsafe { flock(lock.as_raw_fd(), 2 | 4) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let owner = Self {
            root: root.clone(),
            _lock: lock,
        };
        owner.verify()?;
        Ok(owner)
    }
    pub(super) fn verify(&self) -> io::Result<()> {
        private(&self.root)
    }
}
