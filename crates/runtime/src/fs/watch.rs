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
    use super::{io, RecursiveMode, WatchEvent, WatchEventKind};
    // BaseWatcher provides shared functionality for event-based file watching
    // Available on Unix-like platforms and any future platform with reactor-based-fs-watching feature
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    use crate::inhouse::{IoRegistration, ReactorRaw};
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    use std::collections::VecDeque;
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    use sys::reactor::Interest as ReactorInterest;

    /// Shared base watcher for platforms with event-based file watching (e.g., inotify, kqueue)
    /// Future platforms can use this by enabling the `reactor-based-fs-watching` feature
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    pub(super) struct BaseWatcher {
        registration: IoRegistration,
        pending: VecDeque<WatchEvent>,
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    impl BaseWatcher {
        fn new(registration: IoRegistration) -> Self {
            Self {
                registration,
                pending: VecDeque::new(),
            }
        }

        // Reactor-based waiting: polls the reactor for file descriptor readiness
        // Used by all Unix-like platforms with event-based file watching
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

    // Future for reactor-based file descriptor readiness waiting
    // Enables async integration with the reactor's polling mechanism
    // Used by all Unix-like platforms with event-based file watching
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    struct ReadReadyFuture<'a> {
        registration: &'a IoRegistration,
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    impl std::future::Future for ReadReadyFuture<'_> {
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

    /// Helper to register a file descriptor with the reactor for async polling
    /// Available for Unix-like platforms and future platforms with reactor-based-fs-watching feature
    #[cfg(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        feature = "reactor-based-fs-watching"
    ))]
    fn register_fd(
        runtime: &crate::inhouse::InHouseRuntime,
        fd: ReactorRaw,
        interest: ReactorInterest,
    ) -> std::io::Result<IoRegistration> {
        let reactor = runtime.reactor();
        IoRegistration::new(reactor, fd, interest)
    }

    // Linux inotify implementation: Kernel-level file notifications with reactor integration
    #[cfg(target_os = "linux")]
    mod linux {
        use super::{register_fd, BaseWatcher, RecursiveMode, WatchEvent, WatchEventKind};
        use crate::inhouse::InHouseRuntime;
        use std::collections::HashMap;
        use std::fs;
        use std::io;
        use std::os::fd::AsRawFd;
        use std::path::{Path, PathBuf};
        use sys::inotify::{Event as InotifyEvent, Inotify};
        use sys::reactor::Interest;

        const IN_ATTRIB: u32 = 0x0000_0004;
        const IN_CLOSE_WRITE: u32 = 0x0000_0008;
        const IN_CREATE: u32 = 0x0000_0100;
        const IN_DELETE: u32 = 0x0000_0200;
        const IN_DELETE_SELF: u32 = 0x0000_0400;
        const IN_MODIFY: u32 = 0x0000_0002;
        const IN_MOVE_SELF: u32 = 0x0000_0800;
        const IN_MOVED_FROM: u32 = 0x0000_0040;
        const IN_MOVED_TO: u32 = 0x0000_0080;
        const IN_IGNORED: u32 = 0x0000_8000;
        const IN_ISDIR: u32 = 0x4000_0000;
        const IN_Q_OVERFLOW: u32 = 0x0000_4000;

        const WATCH_MASK: u32 = IN_ATTRIB
            | IN_CLOSE_WRITE
            | IN_CREATE
            | IN_DELETE
            | IN_DELETE_SELF
            | IN_MODIFY
            | IN_MOVE_SELF
            | IN_MOVED_FROM
            | IN_MOVED_TO;

        type WatchDescriptor = i32;

        pub(crate) struct Watcher {
            base: BaseWatcher,
            inotify: Inotify,
            watches: HashMap<WatchDescriptor, PathBuf>,
            recursive: bool,
        }

        impl Watcher {
            pub(crate) fn new(
                runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let inotify = Inotify::new()?;
                let fd = inotify.as_raw_fd();
                let registration = register_fd(runtime, fd, Interest::READABLE)?;
                let mut watcher = Self {
                    base: BaseWatcher::new(registration),
                    inotify,
                    watches: HashMap::new(),
                    recursive: matches!(recursive, RecursiveMode::Recursive),
                };
                watcher.register_path(&path)?;
                Ok(watcher)
            }

            pub(crate) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                loop {
                    if let Some(event) = self.base.pop_event() {
                        // Critical: yield to allow other async tasks (like RPC servers) to run
                        // even when processing bursts of file events (e.g., during Settlement::init)
                        crate::yield_now().await;
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

                let descriptor = self.inotify.add_watch(path, WATCH_MASK)?;
                self.watches.insert(descriptor, path.to_path_buf());
                Ok(())
            }

            fn read_events(&mut self) -> io::Result<Vec<WatchEvent>> {
                let mut ready = Vec::new();
                for event in self.inotify.read_events()? {
                    if event.mask & IN_Q_OVERFLOW != 0 {
                        ready.push(WatchEvent::new(WatchEventKind::Other, Vec::new()));
                        continue;
                    }
                    if event.mask & IN_IGNORED != 0 {
                        self.watches.remove(&event.watch_descriptor);
                        continue;
                    }

                    if let Some(path) = self.resolve_path(&event) {
                        if self.recursive
                            && event.mask & IN_ISDIR != 0
                            && (event.mask & (IN_CREATE | IN_MOVED_TO)) != 0
                        {
                            let _ = self.register_path(&path);
                        }
                        if event.mask & (IN_DELETE_SELF | IN_MOVE_SELF) != 0 {
                            self.watches.remove(&event.watch_descriptor);
                        }
                        let kind = Self::classify(event.mask);
                        ready.push(WatchEvent::new(kind, vec![path]));
                    }
                }

                Ok(ready)
            }

            fn resolve_path(&self, event: &InotifyEvent) -> Option<PathBuf> {
                let base = self.watches.get(&event.watch_descriptor)?.clone();
                if let Some(name) = &event.name {
                    Some(base.join(name))
                } else {
                    Some(base)
                }
            }

            fn classify(mask: u32) -> WatchEventKind {
                if mask & (IN_DELETE | IN_DELETE_SELF | IN_MOVED_FROM) != 0 {
                    WatchEventKind::Removed
                } else if mask & (IN_CREATE | IN_MOVED_TO) != 0 {
                    WatchEventKind::Created
                } else if mask & (IN_ATTRIB | IN_CLOSE_WRITE | IN_MODIFY) != 0 {
                    WatchEventKind::Modified
                } else {
                    WatchEventKind::Other
                }
            }
        }

        impl Drop for Watcher {
            fn drop(&mut self) {
                let _ = self.base.registration().deregister();
            }
        }
    }
    // kqueue implementation: BSD/macOS kernel event notification with direct polling
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
        use std::collections::HashMap;
        use std::fs::{self, File};
        use std::io;
        use std::os::fd::{AsRawFd, RawFd};
        use std::path::{Path, PathBuf};
        use sys::kqueue::{self, Kqueue};
        use sys::reactor::Interest;

        const WATCH_FLAGS: u32 = kqueue::NOTE_WRITE
            | kqueue::NOTE_EXTEND
            | kqueue::NOTE_ATTRIB
            | kqueue::NOTE_RENAME
            | kqueue::NOTE_DELETE;

        /// Track file snapshots to detect which files changed in a directory.
        /// When kqueue reports a directory modification, we compare snapshots to find
        /// which specific files were added, removed, or modified.
        /// Uses content hashing (first 8KB) for reliable change detection,
        /// mirroring the polling backend's approach.
        #[derive(Clone)]
        struct FileSnapshot {
            len: u64,
            modified: Option<std::time::SystemTime>,
            /// Content hash of first 8KB for reliable change detection
            content_hash: Option<u64>,
        }

        /// Compute FNV-1a hash of file's first 8KB for content-based change detection
        fn compute_file_hash(path: &Path) -> Option<u64> {
            if let Ok(mut file) = std::fs::File::open(path) {
                use std::io::Read;
                let mut buf = [0u8; 8192];
                match file.read(&mut buf) {
                    Ok(n) => {
                        // FNV-1a algorithm
                        let mut hash = 0xcbf29ce484222325u64;
                        for &byte in &buf[..n] {
                            hash ^= byte as u64;
                            hash = hash.wrapping_mul(0x100000001b3);
                        }
                        Some(hash)
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        }

        pub(crate) struct Watcher {
            base: BaseWatcher,
            queue: Kqueue,
            handles: HashMap<RawFd, File>,
            paths: HashMap<RawFd, PathBuf>,
            recursive: bool,
            /// Store file snapshots for directories to detect which files changed
            dir_snapshots: HashMap<PathBuf, HashMap<PathBuf, FileSnapshot>>,
        }

        impl Watcher {
            pub(crate) fn new(
                runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let queue = Kqueue::new()?;
                let fd = queue.as_raw_fd();
                let registration = register_fd(runtime, fd, Interest::READABLE)?;
                let mut watcher = Self {
                    base: BaseWatcher::new(registration),
                    queue,
                    handles: HashMap::new(),
                    paths: HashMap::new(),
                    recursive: matches!(recursive, RecursiveMode::Recursive),
                    dir_snapshots: HashMap::new(),
                };
                watcher.register_path(&path)?;
                // Take initial snapshot of all watched directories for change detection
                watcher.snapshot_directory_contents(&path)?;
                Ok(watcher)
            }

            pub(crate) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                loop {
                    if let Some(event) = self.base.pop_event() {
                        // Critical: yield to allow other async tasks (like RPC servers) to run
                        // even when processing bursts of file events (e.g., during Settlement::init)
                        crate::yield_now().await;
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
                if self.paths.values().any(|existing| existing == path) {
                    return Ok(());
                }

                let file = File::open(path)?;
                let fd = file.as_raw_fd();
                self.queue.register(fd, WATCH_FLAGS)?;
                self.paths.insert(fd, path.to_path_buf());
                self.handles.insert(fd, file);
                Ok(())
            }

            fn read_events(&mut self) -> io::Result<Vec<WatchEvent>> {
                let events = self.queue.poll_events(64)?;
                let mut ready = Vec::new();
                for event in events {
                    if let Some(path) = self.paths.get(&event.fd).cloned() {
                        let kind = Self::classify(event.flags);
                        if self.recursive && path.is_dir() {
                            let _ = self.register_path(&path);
                        }
                        if event.flags & kqueue::NOTE_DELETE != 0 {
                            self.paths.remove(&event.fd);
                            self.handles.remove(&event.fd);
                        }

                        // When a directory is modified, scan it to detect which specific files changed
                        // Uses content hashing for reliable change detection, mirroring the polling backend.
                        // This ensures kqueue reports file-level changes instead of just directory modifications.
                        if path.is_dir() && matches!(kind, WatchEventKind::Modified) {
                            let file_changes = self.detect_file_changes(&path)?;
                            ready.extend(file_changes);
                        } else {
                            ready.push(WatchEvent::new(kind, vec![path]));
                        }
                    }
                }
                Ok(ready)
            }

            /// Snapshot the contents of a directory for change detection
            fn snapshot_directory_contents(&mut self, path: &Path) -> io::Result<()> {
                if path.is_dir() {
                    let mut snapshot = HashMap::new();
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let entry_path = entry.path();
                            if let Ok(metadata) = entry.metadata() {
                                // For files, compute content hash for reliable change detection
                                let content_hash = if metadata.is_file() {
                                    compute_file_hash(&entry_path)
                                } else {
                                    None
                                };
                                snapshot.insert(
                                    entry_path,
                                    FileSnapshot {
                                        len: metadata.len(),
                                        modified: metadata.modified().ok(),
                                        content_hash,
                                    },
                                );
                            }
                        }
                    }
                    self.dir_snapshots.insert(path.to_path_buf(), snapshot);

                    // Recursively snapshot subdirectories
                    if self.recursive {
                        if let Ok(entries) = fs::read_dir(path) {
                            for entry in entries.flatten() {
                                let entry_path = entry.path();
                                if entry_path.is_dir() {
                                    let _ = self.snapshot_directory_contents(&entry_path);
                                }
                            }
                        }
                    }
                }
                Ok(())
            }

            /// Detect which files changed in a directory by comparing with previous snapshot
            fn detect_file_changes(&mut self, dir_path: &Path) -> io::Result<Vec<WatchEvent>> {
                let mut current = HashMap::new();
                if let Ok(entries) = fs::read_dir(dir_path) {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if let Ok(metadata) = entry.metadata() {
                            // For files, compute content hash for reliable change detection
                            let content_hash = if metadata.is_file() {
                                compute_file_hash(&entry_path)
                            } else {
                                None
                            };
                            current.insert(
                                entry_path,
                                FileSnapshot {
                                    len: metadata.len(),
                                    modified: metadata.modified().ok(),
                                    content_hash,
                                },
                            );
                        }
                    }
                }

                let mut changes = Vec::new();
                let old_snapshot = self
                    .dir_snapshots
                    .get(dir_path)
                    .cloned()
                    .unwrap_or_default();

                // Detect modified and created files
                for (path, meta) in &current {
                    match old_snapshot.get(path) {
                        Some(old) => {
                            // File exists in both snapshots; check if it changed using multiple indicators:
                            // 1. File size changed
                            // 2. Content hash changed (more reliable than timestamp alone)
                            // 3. Modified time changed (for directories)
                            let is_changed = old.len != meta.len
                                || (old.content_hash.is_some()
                                    && old.content_hash != meta.content_hash)
                                || old.modified != meta.modified;

                            if is_changed {
                                changes.push(WatchEvent::new(
                                    WatchEventKind::Modified,
                                    vec![path.clone()],
                                ));
                            }
                        }
                        None => {
                            // New file
                            changes
                                .push(WatchEvent::new(WatchEventKind::Created, vec![path.clone()]));
                        }
                    }
                }

                // Detect removed files
                for path in old_snapshot.keys() {
                    if !current.contains_key(path) {
                        changes.push(WatchEvent::new(WatchEventKind::Removed, vec![path.clone()]));
                    }
                }

                // Update snapshot for next comparison
                self.dir_snapshots.insert(dir_path.to_path_buf(), current);

                Ok(changes)
            }
        }

        impl Watcher {
            fn classify(flags: u32) -> WatchEventKind {
                if flags & kqueue::NOTE_DELETE != 0 {
                    WatchEventKind::Removed
                } else if flags
                    & (kqueue::NOTE_RENAME
                        | kqueue::NOTE_WRITE
                        | kqueue::NOTE_EXTEND
                        | kqueue::NOTE_ATTRIB)
                    != 0
                {
                    WatchEventKind::Modified
                } else {
                    WatchEventKind::Other
                }
            }
        }

        impl Drop for Watcher {
            fn drop(&mut self) {
                let _ = self.base.registration().deregister();
            }
        }
    }
    // Windows implementation: Directory change notifications
    #[cfg(target_os = "windows")]
    mod windows {
        use super::{RecursiveMode, WatchEvent, WatchEventKind};
        use crate::inhouse::{InHouseJoinHandle, InHouseRuntime};
        use crate::sync::{mpsc, CancellationToken};
        use std::io::{self, ErrorKind};
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        use sys::fs::windows::{
            open_directory_handle, DirectoryAction, DirectoryChange, DirectoryChangeDriver,
            DirectoryChangeSignal,
        };

        const WATCH_BUFFER_SIZE: usize = 64 * 1024;
        const POLL_TIMEOUT: Duration = Duration::from_millis(100);

        #[derive(Clone)]
        enum WatchFilter {
            Any,
            File { normalized: String },
        }

        pub(crate) struct Watcher {
            receiver: mpsc::UnboundedReceiver<io::Result<WatchEvent>>,
            _task: InHouseJoinHandle<()>,
            cancel_token: CancellationToken,
            signal: DirectoryChangeSignal,
        }

        impl Watcher {
            pub(crate) fn new(
                runtime: &InHouseRuntime,
                path: PathBuf,
                recursive: RecursiveMode,
            ) -> io::Result<Self> {
                let (root, filter) = determine_target(&path)?;
                let directory = open_directory_handle(&root)?;
                let watch_recursive = matches!(recursive, RecursiveMode::Recursive);
                let (driver, signal) = DirectoryChangeDriver::new(
                    directory,
                    root.clone(),
                    watch_recursive,
                    WATCH_BUFFER_SIZE,
                )?;
                let (sender, receiver) = mpsc::unbounded_channel();
                let cancel_token = CancellationToken::new();
                let task = runtime.spawn_blocking({
                    let cancel = cancel_token.clone();
                    move || run_watch_loop(driver, filter, sender, cancel)
                });

                Ok(Self {
                    receiver,
                    _task: task,
                    cancel_token,
                    signal,
                })
            }

            pub(crate) async fn next_event(&mut self) -> io::Result<WatchEvent> {
                while let Some(result) = self.receiver.recv().await {
                    match result {
                        Ok(event) => return Ok(event),
                        Err(err) => return Err(err),
                    }
                }
                Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "file watcher terminated",
                ))
            }
        }

        impl Drop for Watcher {
            fn drop(&mut self) {
                self.cancel_token.cancel();
                let _ = self.signal.wake();
            }
        }

        fn run_watch_loop(
            mut driver: DirectoryChangeDriver,
            filter: WatchFilter,
            sender: mpsc::UnboundedSender<io::Result<WatchEvent>>,
            cancel: CancellationToken,
        ) {
            loop {
                if cancel.is_cancelled() {
                    let _ = driver.cancel();
                }
                match driver.poll(POLL_TIMEOUT) {
                    Ok(Some(changes)) => {
                        if !emit_changes(&sender, &filter, changes) {
                            break;
                        }
                    }
                    Ok(None) => continue,
                    Err(err) if err.kind() == ErrorKind::Interrupted => break,
                    Err(err) => {
                        let _ = sender.send(Err(err));
                        break;
                    }
                }
            }
            let _ = sender.send(Err(io::Error::new(
                ErrorKind::Interrupted,
                "file watcher stopped",
            )));
        }

        fn emit_changes(
            sender: &mpsc::UnboundedSender<io::Result<WatchEvent>>,
            filter: &WatchFilter,
            changes: Vec<DirectoryChange>,
        ) -> bool {
            for change in changes {
                if !matches_filter(filter, &change.path) {
                    continue;
                }
                let kind = match change.action {
                    DirectoryAction::Added | DirectoryAction::RenamedNew => WatchEventKind::Created,
                    DirectoryAction::Removed | DirectoryAction::RenamedOld => {
                        WatchEventKind::Removed
                    }
                    DirectoryAction::Modified => WatchEventKind::Modified,
                    DirectoryAction::Other => WatchEventKind::Other,
                };
                if sender
                    .send(Ok(WatchEvent::new(kind, vec![change.path])))
                    .is_err()
                {
                    return false;
                }
            }
            true
        }

        fn determine_target(path: &Path) -> io::Result<(PathBuf, WatchFilter)> {
            if path.metadata().map(|meta| meta.is_dir()).unwrap_or(false) {
                Ok((path.to_path_buf(), WatchFilter::Any))
            } else {
                let parent = path.parent().ok_or_else(|| {
                    io::Error::new(
                        ErrorKind::InvalidInput,
                        "watched file must have a parent directory",
                    )
                })?;
                Ok((
                    parent.to_path_buf(),
                    WatchFilter::File {
                        normalized: normalize_path(path),
                    },
                ))
            }
        }

        fn matches_filter(filter: &WatchFilter, candidate: &Path) -> bool {
            match filter {
                WatchFilter::Any => true,
                WatchFilter::File { normalized } => normalize_path(candidate) == *normalized,
            }
        }

        fn normalize_path(path: &Path) -> String {
            path.as_os_str().to_string_lossy().to_ascii_lowercase()
        }
    }
    // Polling implementation: Periodic filesystem scanning fallback for platforms without native file watching
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
            // For regular files, compute a quick hash of first 8KB to detect content changes
            // This provides more reliable change detection than timestamps alone
            content_hash: Option<u64>,
        }

        fn compute_content_hash(path: &Path) -> Option<u64> {
            // Only compute hash for files, not directories
            if let Ok(mut file) = fs::File::open(path) {
                use std::io::Read;
                let mut buf = [0u8; 8192]; // First 8KB is usually sufficient
                match file.read(&mut buf) {
                    Ok(n) => {
                        // Simple hash using FNV-1a algorithm
                        let mut hash = 0xcbf29ce484222325u64;
                        for &byte in &buf[..n] {
                            hash ^= byte as u64;
                            hash = hash.wrapping_mul(0x100000001b3);
                        }
                        Some(hash)
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
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
                            let is_dir = metadata.is_dir();
                            let record = EntryMeta {
                                modified: metadata.modified().ok(),
                                len: metadata.len(),
                                is_dir,
                                content_hash: if !is_dir {
                                    compute_content_hash(&entry_path)
                                } else {
                                    None
                                },
                            };
                            self.snapshot.insert(entry_path.clone(), record.clone());
                            if self.recursive && is_dir {
                                self.record_initial(&entry_path)?;
                            }
                        }
                    }
                } else if path.exists() {
                    let metadata = fs::metadata(path)?;
                    let is_dir = metadata.is_dir();
                    self.snapshot.insert(
                        path.to_path_buf(),
                        EntryMeta {
                            modified: metadata.modified().ok(),
                            len: metadata.len(),
                            is_dir,
                            content_hash: if !is_dir {
                                compute_content_hash(path)
                            } else {
                                None
                            },
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
                            // Detect changes using multiple indicators:
                            // 1. Directory status changed (file became directory or vice versa)
                            // 2. File size changed
                            // 3. File content hash changed (more reliable than timestamp)
                            // 4. For dirs: metadata.modified changed
                            let is_changed = if existing.is_dir != meta.is_dir {
                                true
                            } else if !meta.is_dir {
                                // For regular files, check size and content hash
                                existing.len != meta.len
                                    || existing.content_hash != meta.content_hash
                            } else {
                                // For directories, check timestamp
                                existing.modified != meta.modified
                            };

                            if is_changed {
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
                            let is_dir = metadata.is_dir();
                            out.insert(
                                entry_path.clone(),
                                EntryMeta {
                                    modified: metadata.modified().ok(),
                                    len: metadata.len(),
                                    is_dir,
                                    content_hash: if !is_dir {
                                        compute_content_hash(&entry_path)
                                    } else {
                                        None
                                    },
                                },
                            );
                            if self.recursive && is_dir {
                                self.collect(&entry_path, out)?;
                            }
                        }
                    }
                } else if path.exists() {
                    let metadata = fs::metadata(path)?;
                    let is_dir = metadata.is_dir();
                    out.insert(
                        path.to_path_buf(),
                        EntryMeta {
                            modified: metadata.modified().ok(),
                            len: metadata.len(),
                            is_dir,
                            content_hash: if !is_dir {
                                compute_content_hash(path)
                            } else {
                                None
                            },
                        },
                    );
                }
                Ok(())
            }
        }
    }

    // Export the appropriate Watcher implementation based on platform
    // Custom platforms should implement their own module following one of these patterns
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    pub(super) use kqueue::Watcher;
    #[cfg(target_os = "linux")]
    pub(super) use linux::Watcher;
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
    pub(super) use polling::Watcher;
    #[cfg(target_os = "windows")]
    pub(super) use windows::Watcher;
}

#[cfg(not(feature = "inhouse-backend"))]
mod inhouse {}
