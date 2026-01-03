#![cfg(target_os = "windows")]

use super::{Event, Interest, Token};
use foundation_windows::foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use foundation_windows::io::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, PostQueuedCompletionStatus,
    OVERLAPPED_ENTRY,
};
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::mem::MaybeUninit;
use std::os::windows::io::RawSocket;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, Once};
use std::thread;
use std::time::Duration;

type Socket = RawSocket;
type WsaEvent = isize;
type Handle = HANDLE;

type CompletionKey = usize;

const SOCKET_ERROR: i32 = -1;
const FD_READ: i32 = 0x0001;
const FD_WRITE: i32 = 0x0002;
const FD_ACCEPT: i32 = 0x0008;
const FD_CONNECT: i32 = 0x0010;
const FD_CLOSE: i32 = 0x0020;

const FD_READ_BIT: usize = 0;
const FD_WRITE_BIT: usize = 1;
const FD_OOB_BIT: usize = 2;
const FD_ACCEPT_BIT: usize = 3;
const FD_CONNECT_BIT: usize = 4;
const FD_CLOSE_BIT: usize = 5;
const FD_MAX_EVENTS: usize = 10;

const WSA_INFINITE: u32 = 0xFFFF_FFFF;
const WSA_WAIT_FAILED: u32 = 0xFFFF_FFFF;
const WSA_WAIT_EVENT_0: u32 = 0;
const WSA_MAXIMUM_WAIT_EVENTS: usize = 64;
const MAX_COMPLETIONS: usize = 128;
const CONTROL_SLOT: usize = 1;

#[repr(C)]
struct Wsadata {
    data: [u8; 400],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WsanetworkEvents {
    l_network_events: i32,
    i_error_code: [i32; FD_MAX_EVENTS],
}

impl Default for WsanetworkEvents {
    fn default() -> Self {
        Self {
            l_network_events: 0,
            i_error_code: [0; FD_MAX_EVENTS],
        }
    }
}

fn zero_entry() -> OVERLAPPED_ENTRY {
    OVERLAPPED_ENTRY {
        lpCompletionKey: 0,
        lpOverlapped: std::ptr::null_mut(),
        Internal: 0,
        dwNumberOfBytesTransferred: 0,
    }
}

extern "system" {
    fn WSAStartup(version: u16, data: *mut Wsadata) -> i32;
    fn WSAGetLastError() -> i32;
    fn WSACreateEvent() -> WsaEvent;
    fn WSACloseEvent(event: WsaEvent) -> i32;
    fn WSASetEvent(event: WsaEvent) -> i32;
    fn WSAResetEvent(event: WsaEvent) -> i32;
    fn WSAEventSelect(socket: Socket, event: WsaEvent, network_events: i32) -> i32;
    fn WSAEnumNetworkEvents(
        socket: Socket,
        event: WsaEvent,
        network_events: *mut WsanetworkEvents,
    ) -> i32;
    fn WSAWaitForMultipleEvents(
        events: u32,
        handles: *const WsaEvent,
        wait_all: i32,
        timeout: u32,
        alertable: i32,
    ) -> u32;
}

fn ensure_wsa_started() -> io::Result<()> {
    static START: Once = Once::new();
    static mut INIT_RESULT: i32 = 0;
    START.call_once(|| unsafe {
        let mut data = MaybeUninit::<Wsadata>::uninit();
        INIT_RESULT = WSAStartup(0x0202, data.as_mut_ptr());
    });
    let code = unsafe { INIT_RESULT };
    if code != 0 {
        Err(io::Error::from_raw_os_error(code))
    } else {
        Ok(())
    }
}

fn last_wsa_error() -> io::Error {
    let code = unsafe { WSAGetLastError() };
    io::Error::from_raw_os_error(code)
}

fn last_os_error() -> io::Error {
    let code = unsafe { GetLastError() } as i32;
    io::Error::from_raw_os_error(code)
}

fn interest_to_network_events(interest: Interest) -> i32 {
    let mut mask = FD_CLOSE;
    if interest.contains(Interest::READABLE) {
        mask |= FD_READ | FD_ACCEPT;
    }
    if interest.contains(Interest::WRITABLE) {
        mask |= FD_WRITE | FD_CONNECT;
    }
    mask
}

fn duration_to_timeout(timeout: Option<Duration>) -> u32 {
    match timeout {
        Some(duration) => duration.as_millis().min(u32::MAX as u128) as u32,
        None => WSA_INFINITE,
    }
}

pub struct Poll {
    inner: Arc<Inner>,
}

impl Poll {
    pub fn new() -> io::Result<Self> {
        ensure_wsa_started()?;
        Inner::new().map(|inner| Self { inner })
    }

    pub fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        ensure_wsa_started()?;
        self.inner.poll(events, timeout)
    }

    pub fn register(&self, fd: Socket, token: Token, interest: Interest) -> io::Result<()> {
        ensure_wsa_started()?;
        self.inner.register(fd, token, interest)
    }

    pub fn update_interest(
        &self,
        fd: Socket,
        token: Token,
        _previous: Interest,
        current: Interest,
    ) -> io::Result<()> {
        ensure_wsa_started()?;
        self.inner.update_interest(fd, token, current)
    }

    pub fn deregister(&self, fd: Socket, token: Token) -> io::Result<()> {
        self.inner.deregister(fd, token)
    }

    pub fn create_waker(&self, token: Token) -> io::Result<super::Waker> {
        ensure_wsa_started()?;
        self.inner.create_waker(token)
    }
}

pub struct Events {
    events: Vec<Event>,
}

impl Events {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: Vec::with_capacity(capacity),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }

    fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    fn clear(&mut self) {
        self.events.clear();
    }
}

pub struct Waker {
    inner: Arc<Inner>,
    token: Token,
}

impl Waker {
    pub fn wake(&self) -> io::Result<()> {
        self.inner.post_event(Event::new(
            self.token, None, true, true, false, false, false, true,
        ))
    }
}

struct Inner {
    port: Handle,
    registry: Mutex<Registry>,
}

impl Inner {
    fn new() -> io::Result<Arc<Self>> {
        ensure_wsa_started()?;
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, 0) };
        if port == 0 {
            return Err(last_os_error());
        }
        Ok(Arc::new(Self {
            port,
            registry: Mutex::new(Registry::default()),
        }))
    }

    fn poll(&self, events: &mut Events, timeout: Option<Duration>) -> io::Result<()> {
        events.clear();
        let mut entries = vec![zero_entry(); MAX_COMPLETIONS];
        let mut removed: u32 = 0;
        let timeout_ms = duration_to_timeout(timeout);

        let result = unsafe {
            GetQueuedCompletionStatusEx(
                self.port,
                entries.as_mut_ptr(),
                entries.len() as u32,
                &mut removed,
                timeout_ms,
                0,
            )
        };

        if result == 0 {
            let err = last_os_error();
            if err.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                return Ok(());
            }
            if err.kind() == ErrorKind::WouldBlock {
                return Ok(());
            }
            return Err(err);
        }

        for entry in entries.iter().take(removed as usize) {
            if entry.lpOverlapped.is_null() {
                let completion = unsafe { Box::from_raw(entry.lpCompletionKey as *mut Completion) };
                events.push(completion.event);
            } else {
                let token = Token(entry.lpCompletionKey as usize);
                events.push(Event::new(token, None, true, true, false, false, false, true));
            }
        }

        Ok(())
    }

    fn register(&self, socket: Socket, token: Token, interest: Interest) -> io::Result<()> {
        self.associate_with_port(socket, token)?;

        let event = unsafe { WSACreateEvent() };
        if event == 0 {
            return Err(last_wsa_error());
        }

        let mask = interest_to_network_events(interest);
        let result = unsafe { WSAEventSelect(socket, event, mask) };
        if result == SOCKET_ERROR {
            let err = last_wsa_error();
            unsafe {
                WSACloseEvent(event);
            }
            return Err(err);
        }

        let shard = {
            let mut registry = self.registry.lock().expect("poll registry poisoned");
            registry.acquire_shard(self.port)?
        };

        if let Err(err) = shard.add(Watcher {
            socket,
            token,
            interest,
            event,
        }) {
            unsafe {
                let _ = WSAEventSelect(socket, event, 0);
                WSACloseEvent(event);
            }
            return Err(err);
        }

        let mut registry = self.registry.lock().expect("poll registry poisoned");
        registry.sockets.insert(
            socket,
            SocketEntry {
                socket,
                shard: shard.id,
                event,
            },
        );
        Ok(())
    }

    fn deregister(&self, socket: Socket, _token: Token) -> io::Result<()> {
        let entry = {
            let mut registry = self.registry.lock().expect("poll registry poisoned");
            match registry.sockets.remove(&socket) {
                Some(entry) => entry,
                None => return Ok(()),
            }
        };

        unsafe {
            let _ = WSAEventSelect(socket, entry.event, 0);
        }

        let shard = {
            let registry = self.registry.lock().expect("poll registry poisoned");
            registry
                .shards
                .iter()
                .find(|shard| shard.id == entry.shard)
                .cloned()
        };

        if let Some(shard) = shard {
            shard.remove(socket, entry.event)?;
        } else {
            unsafe {
                WSACloseEvent(entry.event);
            }
        }

        Ok(())
    }

    fn update_interest(&self, socket: Socket, token: Token, interest: Interest) -> io::Result<()> {
        self.deregister(socket, token)?;
        self.register(socket, token, interest)
    }

    fn create_waker(self: &Arc<Self>, token: Token) -> io::Result<super::Waker> {
        {
            let mut registry = self.registry.lock().expect("poll registry poisoned");
            registry.wakers.insert(token.0, token);
        }
        Ok(super::Waker {
            inner: Waker {
                inner: Arc::clone(self),
                token,
            },
        })
    }

    fn post_event(&self, event: Event) -> io::Result<()> {
        let completion = Box::new(Completion { event });
        let ptr = Box::into_raw(completion) as CompletionKey;
        let result = unsafe { PostQueuedCompletionStatus(self.port, 0, ptr, std::ptr::null_mut()) };
        if result == 0 {
            let err = last_os_error();
            unsafe {
                let _ = Box::from_raw(ptr as *mut Completion);
            }
            Err(err)
        } else {
            Ok(())
        }
    }

    fn associate_with_port(&self, socket: Socket, token: Token) -> io::Result<()> {
        let handle = socket as Handle;
        let result =
            unsafe { CreateIoCompletionPort(handle, self.port, token.0 as CompletionKey, 0) };
        if result == 0 {
            Err(last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.registry.lock() {
            for shard in registry.shards.drain(..) {
                let _ = shard.shutdown();
            }
            for entry in registry.sockets.values() {
                unsafe {
                    let _ = WSAEventSelect(entry.socket, entry.event, 0);
                    WSACloseEvent(entry.event);
                }
            }
            registry.sockets.clear();
        }
        unsafe {
            if self.port != 0 {
                CloseHandle(self.port);
            }
        }
    }
}

#[derive(Default)]
struct Registry {
    sockets: HashMap<Socket, SocketEntry>,
    shards: Vec<Arc<Shard>>,
    wakers: HashMap<usize, Token>,
}

impl Registry {
    fn acquire_shard(&mut self, port: Handle) -> io::Result<Arc<Shard>> {
        if let Some(shard) = self
            .shards
            .iter()
            .find(|shard| shard.load() < shard.capacity())
            .cloned()
        {
            return Ok(shard);
        }

        let id = self.shards.len();
        let shard = Arc::new(Shard::new(id, port)?);
        self.shards.push(Arc::clone(&shard));
        Ok(shard)
    }
}

struct SocketEntry {
    socket: Socket,
    shard: usize,
    event: WsaEvent,
}

struct Completion {
    event: Event,
}

struct Shard {
    id: usize,
    control: WsaEvent,
    tx: mpsc::Sender<Command>,
    count: AtomicUsize,
}

impl Shard {
    fn new(id: usize, port: Handle) -> io::Result<Self> {
        let control = unsafe { WSACreateEvent() };
        if control == 0 {
            return Err(last_wsa_error());
        }

        let (tx, rx) = mpsc::channel();
        let shard_control = control;
        thread::spawn(move || run_shard(id, port, shard_control, rx));

        Ok(Self {
            id,
            control,
            tx,
            count: AtomicUsize::new(0),
        })
    }

    fn capacity(&self) -> usize {
        WSA_MAXIMUM_WAIT_EVENTS.saturating_sub(CONTROL_SLOT)
    }

    fn load(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    fn add(&self, watcher: Watcher) -> io::Result<()> {
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Err(err) = self.send(Command::Add(watcher)) {
            self.count.fetch_sub(1, Ordering::SeqCst);
            return Err(err);
        }
        Ok(())
    }

    fn remove(&self, socket: Socket, event: WsaEvent) -> io::Result<()> {
        if let Err(err) = self.send(Command::Remove(Removal { socket, event })) {
            return Err(err);
        }
        self.count.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }

    fn shutdown(&self) -> io::Result<()> {
        self.send(Command::Shutdown)
    }

    fn send(&self, command: Command) -> io::Result<()> {
        self.tx
            .send(command)
            .map_err(|_| io::Error::new(ErrorKind::Other, "shard worker terminated"))?;
        let result = unsafe { WSASetEvent(self.control) };
        if result == 0 {
            Err(last_wsa_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for Shard {
    fn drop(&mut self) {
        unsafe {
            if self.control != 0 {
                WSACloseEvent(self.control);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Watcher {
    socket: Socket,
    token: Token,
    interest: Interest,
    event: WsaEvent,
}

struct Removal {
    socket: Socket,
    event: WsaEvent,
}

enum Command {
    Add(Watcher),
    Remove(Removal),
    Shutdown,
}

fn run_shard(id: usize, port: Handle, control: WsaEvent, rx: mpsc::Receiver<Command>) {
    let _ = id;
    let mut watchers: Vec<Watcher> = Vec::new();
    let mut handles: Vec<WsaEvent> = vec![control];
    let mut active = true;

    while active {
        let wait = unsafe {
            WSAWaitForMultipleEvents(handles.len() as u32, handles.as_ptr(), 0, WSA_INFINITE, 0)
        };

        if wait == WSA_WAIT_FAILED {
            break;
        }

        let index = (wait - WSA_WAIT_EVENT_0) as usize;

        if index == 0 {
            unsafe {
                WSAResetEvent(control);
            }
            while let Ok(command) = rx.try_recv() {
                match command {
                    Command::Add(watcher) => {
                        handles.push(watcher.event);
                        watchers.push(watcher);
                    }
                    Command::Remove(removal) => {
                        if let Some(pos) = watchers
                            .iter()
                            .position(|w| w.socket == removal.socket && w.event == removal.event)
                        {
                            let watcher = watchers.swap_remove(pos);
                            handles.swap_remove(pos + CONTROL_SLOT);
                            unsafe {
                                WSACloseEvent(watcher.event);
                            }
                        } else {
                            unsafe {
                                WSACloseEvent(removal.event);
                            }
                        }
                    }
                    Command::Shutdown => {
                        active = false;
                        break;
                    }
                }
            }
            continue;
        }

        let slot = index - CONTROL_SLOT;
        if slot >= watchers.len() {
            continue;
        }

        let watcher = watchers[slot];
        unsafe {
            WSAResetEvent(handles[index]);
        }

        let mut network = WsanetworkEvents::default();
        let res = unsafe { WSAEnumNetworkEvents(watcher.socket, watcher.event, &mut network) };
        if res == SOCKET_ERROR {
            let _ = post_completion(
                port,
                Event::new(watcher.token, None, false, false, true, true, true, true),
            );
            continue;
        }

        if let Some(event) = convert_network_events(&watcher, &network) {
            let _ = post_completion(port, event);
        }
    }

    for watcher in watchers {
        unsafe {
            WSACloseEvent(watcher.event);
        }
    }
}

fn post_completion(port: Handle, event: Event) -> io::Result<()> {
    let completion = Box::new(Completion { event });
    let ptr = Box::into_raw(completion) as CompletionKey;
    let result = unsafe { PostQueuedCompletionStatus(port, 0, ptr, std::ptr::null_mut()) };
    if result == 0 {
        unsafe {
            let _ = Box::from_raw(ptr as *mut Completion);
        }
        Err(last_os_error())
    } else {
        Ok(())
    }
}

fn convert_network_events(watcher: &Watcher, network: &WsanetworkEvents) -> Option<Event> {
    let mut readable = false;
    let mut writable = false;
    let mut read_closed = false;
    let mut write_closed = false;
    let mut error = false;

    if network.l_network_events & (FD_READ | FD_ACCEPT) != 0 {
        readable = watcher.interest.contains(Interest::READABLE);
        if network.i_error_code[FD_READ_BIT] != 0 || network.i_error_code[FD_ACCEPT_BIT] != 0 {
            error = true;
        }
    }

    if network.l_network_events & (FD_WRITE | FD_CONNECT) != 0 {
        writable = watcher.interest.contains(Interest::WRITABLE);
        if network.i_error_code[FD_WRITE_BIT] != 0 || network.i_error_code[FD_CONNECT_BIT] != 0 {
            error = true;
        }
    }

    if network.l_network_events & FD_CLOSE != 0 {
        read_closed = true;
        write_closed = true;
        if network.i_error_code[FD_CLOSE_BIT] != 0 {
            error = true;
        }
    }

    if network.i_error_code[FD_OOB_BIT] != 0 {
        error = true;
    }

    if readable || writable || read_closed || write_closed || error {
        Some(Event::new(
            watcher.token,
            Some(watcher.socket as usize),
            readable,
            writable,
            error,
            read_closed,
            write_closed,
            false,
        ))
    } else {
        None
    }
}
