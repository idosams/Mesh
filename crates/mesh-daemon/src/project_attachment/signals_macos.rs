//! FSEvents is a lossy wakeup source only. Paths, flags and IDs never authorize saved content.
#![allow(unsafe_code)]

use super::Shared;
use std::ffi::{c_char, c_void, CString};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

#[repr(C)]
struct Context {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    description: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
}
type Callback =
    unsafe extern "C" fn(*const c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: Callback,
        context: *mut Context,
        paths: *const c_void,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: c_void;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        text: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFArrayCreate(
        allocator: *const c_void,
        values: *const *const c_void,
        count: isize,
        callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
}
unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attributes: *const c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
}

// FSEvents retains the context on creation and releases it on stream deallocation. The caller's
// Arc remains live throughout creation; callback invocations borrow the framework's retained Arc.
unsafe extern "C" fn retain(info: *const c_void) -> *const c_void {
    unsafe {
        Arc::increment_strong_count(info.cast::<Shared>());
    }
    info
}
unsafe extern "C" fn release(info: *const c_void) {
    unsafe {
        Arc::decrement_strong_count(info.cast::<Shared>());
    }
}
unsafe extern "C" fn signal(
    _: *const c_void,
    info: *mut c_void,
    _: usize,
    _: *mut c_void,
    _: *const u32,
    _: *const u64,
) {
    // No Rust panic can cross the C callback boundary. Event batches, including drops/root changes,
    // only request a complete identity-checked rescan and cannot supply content or attribution.
    let shared = unsafe { &*info.cast::<Shared>() };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut state = shared.lock();
        if !state.stop {
            state.filesystem_pending = true;
            state.status.event_signals = state.status.event_signals.saturating_add(1);
            state.status.revision = state.status.revision.saturating_add(1);
            shared.changed.notify_all();
        }
    }));
}

pub(super) struct Events {
    stream: *mut c_void,
    queue: *mut c_void,
}
impl Events {
    pub(super) fn start(path: &Path, shared: &Arc<Shared>) -> io::Result<Self> {
        let path = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
        // SDK FSEvents.h/CoreFoundation ABI: UTF-8=0x08000100; SinceNow=u64::MAX; WatchRoot=4.
        // CF type array callbacks retain its string. Stream creation retains its paths/context.
        unsafe {
            let string = CFStringCreateWithCString(ptr::null(), path.as_ptr(), 0x08000100);
            if string.is_null() {
                return Err(io::Error::other("event path unavailable"));
            }
            let paths = CFArrayCreate(
                ptr::null(),
                &string,
                1,
                ptr::addr_of!(kCFTypeArrayCallBacks),
            );
            CFRelease(string);
            if paths.is_null() {
                return Err(io::Error::other("event paths unavailable"));
            }
            let mut context = Context {
                version: 0,
                info: Arc::as_ptr(shared).cast_mut().cast(),
                retain: Some(retain),
                release: Some(release),
                description: None,
            };
            let stream =
                FSEventStreamCreate(ptr::null(), signal, &mut context, paths, u64::MAX, 0.25, 4);
            CFRelease(paths);
            if stream.is_null() {
                return Err(io::Error::other("event stream unavailable"));
            }
            let queue = dispatch_queue_create(c"mesh.attachment.events".as_ptr(), ptr::null());
            if queue.is_null() {
                FSEventStreamRelease(stream);
                return Err(io::Error::other("event queue unavailable"));
            }
            FSEventStreamSetDispatchQueue(stream, queue);
            if FSEventStreamStart(stream) == 0 {
                FSEventStreamInvalidate(stream);
                FSEventStreamRelease(stream);
                dispatch_release(queue);
                return Err(io::Error::other("event stream could not start"));
            }
            Ok(Self { stream, queue })
        }
    }
}
impl Drop for Events {
    fn drop(&mut self) {
        // Stop guarantees no further callback. Invalidate unschedules before stream/context release.
        // This owner stays on the native signals helper; no raw native handle crosses Rust threads.
        unsafe {
            FSEventStreamStop(self.stream);
            FSEventStreamInvalidate(self.stream);
            FSEventStreamRelease(self.stream);
            dispatch_release(self.queue);
        }
    }
}
