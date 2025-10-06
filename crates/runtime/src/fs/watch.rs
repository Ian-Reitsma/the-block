use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchEventKind {
    Created,
    Modified,
    Removed,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchEvent {
    pub paths: Vec<PathBuf>,
    pub kind: WatchEventKind,
}

impl WatchEvent {
    pub fn new(kind: WatchEventKind, paths: Vec<PathBuf>) -> Self {
        Self { kind, paths }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecursiveMode {
    NonRecursive,
    Recursive,
}

pub struct Watcher {
    inner: WatcherInner,
}

enum WatcherInner {
    #[cfg(feature = "inhouse-backend")]
    InHouse(inhouse::Watcher),
    #[cfg(not(feature = "inhouse-backend"))]
    Unsupported,
}

impl Watcher {
    pub fn new(path: impl AsRef<Path>, recursive: RecursiveMode) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        #[cfg(feature = "inhouse-backend")]
        {
            if let Some(runtime) = crate::handle().inhouse_runtime() {
                let watcher = inhouse::Watcher::new(runtime.as_ref(), path, recursive)?;
                return Ok(Self {
                    inner: WatcherInner::InHouse(watcher),
                });
            }
        }

        Err(io::Error::new(
            ErrorKind::Unsupported,
            "no runtime watcher backend available",
        ))
    }

    pub async fn next(&mut self) -> io::Result<WatchEvent> {
        match &mut self.inner {
            #[cfg(feature = "inhouse-backend")]
            WatcherInner::InHouse(inner) => inner.next_event().await,
            #[cfg(not(feature = "inhouse-backend"))]
            WatcherInner::Unsupported => Err(io::Error::new(
                ErrorKind::Unsupported,
                "file watching requires the in-house runtime backend",
            )),
        }
    }
}

#[cfg(feature = "inhouse-backend")]
mod inhouse {
    use super::{RecursiveMode, WatchEvent, WatchEventKind};
    use crate::inhouse::{InHouseRuntime, IoRegistration};
    use mio::Interest;
    use std::collections::VecDeque;
    use std::io;
    use std::path::PathBuf;

    pub(super) struct BaseWatcher {
        registration: IoRegistration,
        pending: VecDeque<WatchEvent>,
    }

    impl BaseWatcher {
        fn new(registration: IoRegistration) -> Self {
            Self {
                registration,
                pending: VecDeque::new(),
            }
        }

        async fn wait_ready(&self) -> io::Result<()> {
            ReadReadyFuture {
                registration: &self.registration,
            }
            .await
        }

        fn push_events<I>(&mut self, events: I)
        where
            I: IntoIterator<Item = WatchEvent>,
        {
            self.pending.extend(events);
        }

        fn pop_event(&mut self) -> Option<WatchEvent> {
            self.pending.pop_front()
        }

        fn registration(&self) -> &IoRegistration {
            &self.registration
        }
    }

    struct ReadReadyFuture<'a> {
        registration: &'a IoRegistration,
    }

    impl<'a> std::future::Future for ReadReadyFuture<'a> {
        type Output = io::Result<()>;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            match self.registration.poll_read_ready(cx) {
                std::task::Poll::Ready(result) => std::task::Poll::Ready(result),
                std::task::Poll::Pending => std::task::Poll::Pending,
            }
        }
    }

    fn register_fd(
        runtime: &InHouseRuntime,
        source: &mut impl mio::event::Source,
        interest: Interest,
    ) -> io::Result<IoRegistration> {
        let reactor = runtime.reactor();
        IoRegistration::new(reactor, source, interest)
    }

    #[cfg(target_os = "linux")]
    type PlatformWatcher = linux::Watcher;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    type PlatformWatcher = kqueue::Watcher;
    #[cfg(target_os = "windows")]
    type PlatformWatcher = windows::Watcher;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "windows"
    )))]
    type PlatformWatcher = polling::Watcher;

    pub(super) struct Watcher {
        inner: PlatformWatcher,
    }

    impl Watcher {
        pub(super) fn new(
            runtime: &InHouseRuntime,
            path: PathBuf,
            recursive: RecursiveMode,
        ) -> io::Result<Self> {
            PlatformWatcher::new(runtime, path, recursive).map(|inner| Self { inner })
        }

        pub(super) async fn next_event(&mut self) -> io::Result<WatchEvent> {
            self.inner.next_event().await
        }
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::{register_fd, BaseWatcher, RecursiveMode, WatchEvent, WatchEventKind};
        use crate::inhouse::InHouseRuntime;
        use mio::unix::SourceFd;
        use mio::Interest;
        use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify, InotifyEvent, WatchDescriptor};
        use std::collections::HashMap;
        use std::fs;
        use std::io;
        use std::os::fd::{AsFd, AsRawFd};
        use std::path::{Path, PathBuf};

        pub(super) struct Watcher {
            base: BaseWatcher,
            inotify: Inotify,
            watches: HashMap<WatchDescriptor, PathBuf>,
            recursive: bool,
        }

        impl Watcher {
            pub(super) fn new(
                runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let inotify = Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC)?;
                let raw_fd = inotify.as_fd().as_raw_fd();
                let mut source = SourceFd(&raw_fd);
                let registration = register_fd(runtime, &mut source, Interest::READABLE)?;
                let mut watcher = Self {
                    base: BaseWatcher::new(registration),
                    inotify,
                    watches: HashMap::new(),
                    recursive: matches!(recursive, RecursiveMode::Recursive),
                };
                watcher.register_path(&path)?;
                Ok(watcher)
            }

            pub(super) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                loop {
                    if let Some(event) = self.base.pop_event() {
                        return Ok(event);
                    }

                    self.base.wait_ready().await?;
                    let events = self.read_events()?;
                    if events.is_empty() {
                        continue;
                    }
                    self.base.push_events(events);
                }
            }

            fn register_path(&mut self, path: &Path) -> io::Result<()> {
                if !path.exists() {
                    return Ok(());
                }

                if path.is_dir() {
                    self.add_watch(path)?;
                    if self.recursive {
                        for entry in fs::read_dir(path)? {
                            let entry = entry?;
                            let entry_path = entry.path();
                            if entry_path.is_dir() {
                                self.register_path(&entry_path)?;
                            }
                        }
                    }
                } else if let Some(parent) = path.parent() {
                    self.add_watch(parent)?;
                }

                Ok(())
            }

            fn add_watch(&mut self, path: &Path) -> io::Result<()> {
                if self.watches.values().any(|existing| existing == path) {
                    return Ok(());
                }

                let descriptor = self.inotify.add_watch(path, Self::watch_mask())?;
                self.watches.insert(descriptor, path.to_path_buf());
                Ok(())
            }

            fn read_events(&mut self) -> io::Result<Vec<WatchEvent>> {
                let mut ready = Vec::new();
                let events = self.inotify.read_events()?;
                for event in events {
                    if event.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                        ready.push(WatchEvent::new(WatchEventKind::Other, Vec::new()));
                        continue;
                    }
                    if event.mask.contains(AddWatchFlags::IN_IGNORED) {
                        self.watches.remove(&event.wd);
                        continue;
                    }

                    if let Some(path) = self.resolve_path(&event) {
                        if self.recursive
                            && event.mask.contains(AddWatchFlags::IN_ISDIR)
                            && (event.mask.contains(AddWatchFlags::IN_CREATE)
                                || event.mask.contains(AddWatchFlags::IN_MOVED_TO))
                        {
                            let _ = self.register_path(&path);
                        }
                        if event
                            .mask
                            .intersects(AddWatchFlags::IN_DELETE_SELF | AddWatchFlags::IN_MOVE_SELF)
                        {
                            self.watches.remove(&event.wd);
                        }
                        let kind = Self::classify(event.mask);
                        let out_path = path.clone();
                        ready.push(WatchEvent::new(kind, vec![out_path]));
                    }
                }

                Ok(ready)
            }

            fn resolve_path(&self, event: &InotifyEvent) -> Option<PathBuf> {
                let base = self.watches.get(&event.wd)?.clone();
                if let Some(name) = &event.name {
                    Some(base.join(name))
                } else {
                    Some(base)
                }
            }

            fn watch_mask() -> AddWatchFlags {
                AddWatchFlags::IN_ATTRIB
                    | AddWatchFlags::IN_CLOSE_WRITE
                    | AddWatchFlags::IN_CREATE
                    | AddWatchFlags::IN_DELETE
                    | AddWatchFlags::IN_DELETE_SELF
                    | AddWatchFlags::IN_MODIFY
                    | AddWatchFlags::IN_MOVE_SELF
                    | AddWatchFlags::IN_MOVED_FROM
                    | AddWatchFlags::IN_MOVED_TO
            }

            fn classify(mask: AddWatchFlags) -> WatchEventKind {
                if mask.intersects(
                    AddWatchFlags::IN_DELETE
                        | AddWatchFlags::IN_DELETE_SELF
                        | AddWatchFlags::IN_MOVED_FROM,
                ) {
                    WatchEventKind::Removed
                } else if mask.intersects(AddWatchFlags::IN_CREATE | AddWatchFlags::IN_MOVED_TO) {
                    WatchEventKind::Created
                } else if mask.intersects(
                    AddWatchFlags::IN_ATTRIB
                        | AddWatchFlags::IN_CLOSE_WRITE
                        | AddWatchFlags::IN_MODIFY,
                ) {
                    WatchEventKind::Modified
                } else {
                    WatchEventKind::Other
                }
            }
        }

        impl Drop for Watcher {
            fn drop(&mut self) {
                let raw_fd = self.inotify.as_fd().as_raw_fd();
                let mut source = SourceFd(&raw_fd);
                let _ = self.base.registration().deregister(&mut source);
            }
        }
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    mod kqueue {
        use super::{register_fd, BaseWatcher, RecursiveMode, WatchEvent, WatchEventKind};
        use crate::inhouse::InHouseRuntime;
        use libc::timespec;
        use mio::unix::SourceFd;
        use mio::Interest;
        use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent, Kqueue};
        use std::collections::HashMap;
        use std::fs::{self, File};
        use std::io;
        use std::os::fd::{AsFd, AsRawFd};
        use std::path::{Path, PathBuf};

        pub(super) struct Watcher {
            base: BaseWatcher,
            queue: Kqueue,
            handles: HashMap<i32, File>,
            paths: HashMap<i32, PathBuf>,
            recursive: bool,
        }

        impl Watcher {
            pub(super) fn new(
                runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let queue = Kqueue::new()?;
                let raw_fd = queue.as_fd().as_raw_fd();
                let mut source = SourceFd(&raw_fd);
                let registration = register_fd(runtime, &mut source, Interest::READABLE)?;
                let mut watcher = Self {
                    base: BaseWatcher::new(registration),
                    queue,
                    handles: HashMap::new(),
                    paths: HashMap::new(),
                    recursive: matches!(recursive, RecursiveMode::Recursive),
                };
                watcher.register_path(&path)?;
                Ok(watcher)
            }

            pub(super) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                loop {
                    if let Some(event) = self.base.pop_event() {
                        return Ok(event);
                    }
                    self.base.wait_ready().await?;
                    let events = self.read_events()?;
                    if events.is_empty() {
                        continue;
                    }
                    self.base.push_events(events);
                }
            }

            fn register_path(&mut self, path: &Path) -> io::Result<()> {
                if !path.exists() {
                    return Ok(());
                }

                if path.is_dir() {
                    self.add_descriptor(path)?;
                    if self.recursive {
                        if let Ok(entries) = fs::read_dir(path) {
                            for entry in entries.flatten() {
                                let entry_path = entry.path();
                                if entry_path.is_dir() {
                                    self.register_path(&entry_path)?;
                                }
                            }
                        }
                    }
                } else if let Some(parent) = path.parent() {
                    self.add_descriptor(parent)?;
                }

                Ok(())
            }

            fn add_descriptor(&mut self, path: &Path) -> io::Result<()> {
                let file = File::open(path)?;
                let fd = file.as_raw_fd();
                if self.paths.contains_key(&fd) {
                    return Ok(());
                }

                let flags = EventFlag::EV_ADD | EventFlag::EV_CLEAR | EventFlag::EV_ENABLE;
                let fflags = FilterFlag::NOTE_WRITE
                    | FilterFlag::NOTE_EXTEND
                    | FilterFlag::NOTE_DELETE
                    | FilterFlag::NOTE_RENAME
                    | FilterFlag::NOTE_ATTRIB;
                let changelist = [KEvent::new(
                    fd as usize,
                    EventFilter::EVFILT_VNODE,
                    flags,
                    fflags,
                    0,
                    0,
                )];
                let mut scratch: [KEvent; 0] = [];
                let timeout = timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                let _ = self
                    .queue
                    .kevent(&changelist, &mut scratch, Some(timeout))?;
                self.paths.insert(fd, path.to_path_buf());
                self.handles.insert(fd, file);
                Ok(())
            }

            fn read_events(&mut self) -> io::Result<Vec<WatchEvent>> {
                let mut eventlist = vec![
                    KEvent::new(
                        0,
                        EventFilter::EVFILT_VNODE,
                        EventFlag::empty(),
                        FilterFlag::empty(),
                        0,
                        0,
                    );
                    64
                ];
                let timeout = timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                let count = self.queue.kevent(&[], &mut eventlist, Some(timeout))?;
                let mut ready = Vec::new();
                for event in eventlist.into_iter().take(count) {
                    if let Some(path) = self.resolve_path(event.ident() as i32) {
                        let kind = Self::classify(event.fflags());
                        if self.recursive && path.is_dir() {
                            let _ = self.register_path(&path);
                        }
                        if event.fflags().contains(FilterFlag::NOTE_DELETE) {
                            let fd = event.ident() as i32;
                            self.paths.remove(&fd);
                            self.handles.remove(&fd);
                        }
                        ready.push(WatchEvent::new(kind, vec![path]));
                    }
                }
                Ok(ready)
            }

            fn resolve_path(&self, fd: i32) -> Option<PathBuf> {
                self.paths.get(&fd).cloned()
            }

            fn classify(flags: FilterFlag) -> WatchEventKind {
                if flags.intersects(FilterFlag::NOTE_DELETE) {
                    WatchEventKind::Removed
                } else if flags.intersects(FilterFlag::NOTE_RENAME) {
                    WatchEventKind::Modified
                } else if flags.intersects(
                    FilterFlag::NOTE_WRITE | FilterFlag::NOTE_EXTEND | FilterFlag::NOTE_ATTRIB,
                ) {
                    WatchEventKind::Modified
                } else {
                    WatchEventKind::Other
                }
            }
        }

        impl Drop for Watcher {
            fn drop(&mut self) {
                let raw_fd = self.queue.as_fd().as_raw_fd();
                let mut source = SourceFd(&raw_fd);
                let _ = self.base.registration().deregister(&mut source);
            }
        }
    }

    #[cfg(target_os = "windows")]
    mod windows {
        use super::{RecursiveMode, WatchEvent, WatchEventKind};
        use crate::inhouse::{InHouseJoinHandle, InHouseRuntime};
        use crate::sync::{mpsc, CancellationToken};
        use std::ffi::OsString;
        use std::io::{self, ErrorKind};
        use std::mem::size_of;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::ffi::OsStringExt;
        use std::os::windows::io::{AsRawHandle, OwnedHandle, RawHandle};
        use std::path::{Path, PathBuf};
        use std::ptr;
        use windows_sys::Win32::Foundation::{
            HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        };
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED,
            FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
            FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION,
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SECURITY, FILE_NOTIFY_CHANGE_SIZE,
            FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };
        use windows_sys::Win32::System::Threading::{
            CreateEventW, SetEvent, WaitForMultipleObjects,
        };
        use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

        const WATCH_BUFFER_SIZE: usize = 64 * 1024;
        const INFINITE: u32 = 0xFFFFFFFF;
        const ERROR_IO_PENDING: i32 = 997;
        const ERROR_OPERATION_ABORTED: i32 = 995;

        enum WatchFilter {
            Any,
            File { normalized: String },
        }

        pub(super) struct Watcher {
            receiver: mpsc::UnboundedReceiver<io::Result<WatchEvent>>,
            task: InHouseJoinHandle<()>,
            cancel_token: CancellationToken,
            cancel_event: OwnedHandle,
        }

        impl Watcher {
            pub(super) fn new(
                runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let (root, filter) = determine_target(&path)?;
                let directory_handle = open_directory_handle(&root)?;
                let change_event = create_event(false)?;
                let cancel_event = create_event(true)?;
                let cancel_event_for_thread = cancel_event.try_clone()?;
                let (sender, receiver) = mpsc::unbounded_channel();
                let cancel_token = CancellationToken::new();
                let watch_recursive = matches!(recursive, RecursiveMode::Recursive);
                let task = runtime.spawn_blocking({
                    let root = root.clone();
                    let cancel_token = cancel_token.clone();
                    let filter = filter;
                    move || {
                        run_watch_loop(
                            directory_handle,
                            change_event,
                            cancel_event_for_thread,
                            root,
                            filter,
                            watch_recursive,
                            sender,
                            cancel_token,
                        )
                    }
                });

                Ok(Self {
                    receiver,
                    task,
                    cancel_token,
                    cancel_event,
                })
            }

            pub(super) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                loop {
                    match self.receiver.recv().await {
                        Some(Ok(event)) => return Ok(event),
                        Some(Err(err)) => return Err(err),
                        None => {
                            return Err(io::Error::new(
                                ErrorKind::UnexpectedEof,
                                "file watcher terminated",
                            ))
                        }
                    }
                }
            }
        }

        impl Drop for Watcher {
            fn drop(&mut self) {
                self.cancel_token.cancel();
                unsafe {
                    let _ = SetEvent(self.cancel_event.as_raw_handle() as HANDLE);
                }
                self.task.abort();
                while self.receiver.try_recv().is_ok() {}
            }
        }

        fn determine_target(path: &Path) -> io::Result<(PathBuf, WatchFilter)> {
            let metadata = path.metadata();
            if metadata.as_ref().map(|meta| meta.is_dir()).unwrap_or(false) {
                Ok((path.to_path_buf(), WatchFilter::Any))
            } else {
                let parent = path.parent().ok_or_else(|| {
                    io::Error::new(
                        ErrorKind::InvalidInput,
                        "watched file must have a parent directory",
                    )
                })?;
                let normalized = normalize_path(path);
                Ok((parent.to_path_buf(), WatchFilter::File { normalized }))
            }
        }

        fn open_directory_handle(path: &Path) -> io::Result<OwnedHandle> {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            if !wide.ends_with(&[0]) {
                wide.push(0);
            }
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    FILE_LIST_DIRECTORY,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    0,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
        }

        fn create_event(manual_reset: bool) -> io::Result<OwnedHandle> {
            let handle =
                unsafe { CreateEventW(ptr::null_mut(), manual_reset as i32, 0, ptr::null()) };
            if handle == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
        }

        fn run_watch_loop(
            directory_handle: OwnedHandle,
            change_event: OwnedHandle,
            cancel_event: OwnedHandle,
            root: PathBuf,
            filter: WatchFilter,
            recursive: bool,
            sender: mpsc::UnboundedSender<io::Result<WatchEvent>>,
            cancel_token: CancellationToken,
        ) {
            let mut buffer = vec![0u8; WATCH_BUFFER_SIZE];
            let mut overlapped = Box::new(unsafe { std::mem::zeroed::<OVERLAPPED>() });

            loop {
                if cancel_token.is_cancelled() {
                    break;
                }

                unsafe {
                    overlapped.Internal = 0;
                    overlapped.InternalHigh = 0;
                    overlapped.Anonymous.Anonymous.Offset = 0;
                    overlapped.Anonymous.Anonymous.OffsetHigh = 0;
                    overlapped.hEvent = change_event.as_raw_handle() as HANDLE;
                }

                let read_result = unsafe {
                    ReadDirectoryChangesW(
                        directory_handle.as_raw_handle() as HANDLE,
                        buffer.as_mut_ptr().cast(),
                        buffer.len() as u32,
                        recursive as i32,
                        FILE_NOTIFY_CHANGE_FILE_NAME
                            | FILE_NOTIFY_CHANGE_DIR_NAME
                            | FILE_NOTIFY_CHANGE_ATTRIBUTES
                            | FILE_NOTIFY_CHANGE_LAST_WRITE
                            | FILE_NOTIFY_CHANGE_CREATION
                            | FILE_NOTIFY_CHANGE_SIZE
                            | FILE_NOTIFY_CHANGE_SECURITY,
                        ptr::null_mut(),
                        overlapped.as_mut(),
                        None,
                    )
                };

                if read_result == 0 {
                    let err = io::Error::last_os_error();
                    if err.raw_os_error() != Some(ERROR_IO_PENDING) {
                        let _ = sender.send(Err(err));
                        break;
                    }
                }

                let handles = [
                    change_event.as_raw_handle() as HANDLE,
                    cancel_event.as_raw_handle() as HANDLE,
                ];

                let wait_result = unsafe {
                    WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, INFINITE)
                };

                match wait_result {
                    WAIT_OBJECT_0 => {}
                    n if n == WAIT_OBJECT_0 + 1 => {
                        unsafe {
                            CancelIoEx(
                                directory_handle.as_raw_handle() as HANDLE,
                                overlapped.as_mut(),
                            );
                        }
                        break;
                    }
                    WAIT_TIMEOUT => continue,
                    WAIT_FAILED => {
                        let _ = sender.send(Err(io::Error::last_os_error()));
                        break;
                    }
                    _ => {
                        let _ = sender.send(Err(io::Error::new(
                            ErrorKind::Other,
                            "unexpected wait result",
                        )));
                        break;
                    }
                }

                let mut bytes_transferred = 0u32;
                let get_result = unsafe {
                    GetOverlappedResult(
                        directory_handle.as_raw_handle() as HANDLE,
                        overlapped.as_mut(),
                        &mut bytes_transferred,
                        0,
                    )
                };

                if get_result == 0 {
                    let err = io::Error::last_os_error();
                    if cancel_token.is_cancelled()
                        && err.raw_os_error() == Some(ERROR_OPERATION_ABORTED)
                    {
                        break;
                    }
                    let _ = sender.send(Err(err));
                    continue;
                }

                if bytes_transferred == 0 {
                    continue;
                }

                if !dispatch_events(
                    &sender,
                    &root,
                    &filter,
                    &buffer[..bytes_transferred as usize],
                ) {
                    break;
                }
            }

            let _ = sender.send(Err(io::Error::new(
                ErrorKind::Interrupted,
                "file watcher stopped",
            )));
        }

        fn dispatch_events(
            sender: &mpsc::UnboundedSender<io::Result<WatchEvent>>,
            root: &Path,
            filter: &WatchFilter,
            buffer: &[u8],
        ) -> bool {
            let mut offset = 0usize;

            while offset < buffer.len() {
                if buffer.len() - offset < size_of::<FILE_NOTIFY_INFORMATION>() {
                    break;
                }
                let record =
                    unsafe { &*(buffer[offset..].as_ptr() as *const FILE_NOTIFY_INFORMATION) };

                let name_len = (record.FileNameLength as usize) / 2;
                let name_slice =
                    unsafe { std::slice::from_raw_parts(record.FileName.as_ptr(), name_len) };
                let name = OsString::from_wide(name_slice);
                let candidate = root.join(PathBuf::from(name));

                if !matches_filter(filter, &candidate) {
                    if record.NextEntryOffset == 0 {
                        break;
                    }
                    offset += record.NextEntryOffset as usize;
                    continue;
                }

                let kind = map_action(record.Action as u32);
                if sender
                    .send(Ok(WatchEvent::new(kind, vec![candidate])))
                    .is_err()
                {
                    return false;
                }

                if record.NextEntryOffset == 0 {
                    break;
                }
                offset += record.NextEntryOffset as usize;
            }

            true
        }

        fn matches_filter(filter: &WatchFilter, candidate: &Path) -> bool {
            match filter {
                WatchFilter::Any => true,
                WatchFilter::File { normalized } => normalize_path(candidate) == *normalized,
            }
        }

        fn map_action(action: u32) -> WatchEventKind {
            match action {
                FILE_ACTION_ADDED | FILE_ACTION_RENAMED_NEW_NAME => WatchEventKind::Created,
                FILE_ACTION_REMOVED | FILE_ACTION_RENAMED_OLD_NAME => WatchEventKind::Removed,
                FILE_ACTION_MODIFIED => WatchEventKind::Modified,
                _ => WatchEventKind::Other,
            }
        }

        fn normalize_path(path: &Path) -> String {
            path.as_os_str().to_string_lossy().to_ascii_lowercase()
        }
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "windows"
    )))]
    mod polling {
        use super::{RecursiveMode, WatchEvent, WatchEventKind};
        use crate::inhouse::InHouseRuntime;
        use std::collections::{HashMap, VecDeque};
        use std::fs;
        use std::io;
        use std::path::{Path, PathBuf};
        use std::time::{Duration, SystemTime};

        #[derive(Clone)]
        struct EntryMeta {
            modified: Option<SystemTime>,
            len: u64,
            is_dir: bool,
        }

        pub(super) struct Watcher {
            path: PathBuf,
            recursive: bool,
            snapshot: HashMap<PathBuf, EntryMeta>,
            pending: VecDeque<WatchEvent>,
        }

        impl Watcher {
            pub(super) fn new(
                _runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let mut watcher = Self {
                    path: path.clone(),
                    recursive: matches!(recursive, RecursiveMode::Recursive),
                    snapshot: HashMap::new(),
                    pending: VecDeque::new(),
                };
                watcher.record_initial(&path)?;
                Ok(watcher)
            }

            pub(super) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                loop {
                    if let Some(event) = self.pending.pop_front() {
                        return Ok(event);
                    }
                    crate::sleep(Duration::from_millis(250)).await;
                    let events = self.scan()?;
                    if events.is_empty() {
                        continue;
                    }
                    self.pending.extend(events);
                }
            }

            fn record_initial(&mut self, path: &Path) -> io::Result<()> {
                if path.is_dir() {
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            let metadata = entry.metadata()?;
                            let record = EntryMeta {
                                modified: metadata.modified().ok(),
                                len: metadata.len(),
                                is_dir: metadata.is_dir(),
                            };
                            self.snapshot.insert(entry_path.clone(), record.clone());
                            if self.recursive && metadata.is_dir() {
                                self.record_initial(&entry_path)?;
                            }
                        }
                    }
                } else if path.exists() {
                    let metadata = fs::metadata(path)?;
                    self.snapshot.insert(
                        path.to_path_buf(),
                        EntryMeta {
                            modified: metadata.modified().ok(),
                            len: metadata.len(),
                            is_dir: metadata.is_dir(),
                        },
                    );
                }
                Ok(())
            }

            fn scan(&mut self) -> io::Result<Vec<WatchEvent>> {
                let mut current = HashMap::new();
                self.collect(&self.path, &mut current)?;
                let mut events = Vec::new();

                for (path, meta) in current.iter() {
                    match self.snapshot.get(path) {
                        Some(existing) => {
                            if existing.is_dir != meta.is_dir
                                || existing.len != meta.len
                                || existing.modified != meta.modified
                            {
                                events.push(WatchEvent::new(
                                    WatchEventKind::Modified,
                                    vec![path.clone()],
                                ));
                            }
                        }
                        None => {
                            events
                                .push(WatchEvent::new(WatchEventKind::Created, vec![path.clone()]));
                        }
                    }
                }

                for path in self.snapshot.keys() {
                    if !current.contains_key(path) {
                        events.push(WatchEvent::new(WatchEventKind::Removed, vec![path.clone()]));
                    }
                }

                self.snapshot = current;
                Ok(events)
            }

            fn collect(
                &self,
                path: &Path,
                out: &mut HashMap<PathBuf, EntryMeta>,
            ) -> io::Result<()> {
                if path.is_dir() {
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            let metadata = entry.metadata()?;
                            out.insert(
                                entry_path.clone(),
                                EntryMeta {
                                    modified: metadata.modified().ok(),
                                    len: metadata.len(),
                                    is_dir: metadata.is_dir(),
                                },
                            );
                            if self.recursive && metadata.is_dir() {
                                self.collect(&entry_path, out)?;
                            }
                        }
                    }
                } else if path.exists() {
                    let metadata = fs::metadata(path)?;
                    out.insert(
                        path.to_path_buf(),
                        EntryMeta {
                            modified: metadata.modified().ok(),
                            len: metadata.len(),
                            is_dir: metadata.is_dir(),
                        },
                    );
                }
                Ok(())
            }
        }
    }
}

#[cfg(not(feature = "inhouse-backend"))]
mod inhouse {}
