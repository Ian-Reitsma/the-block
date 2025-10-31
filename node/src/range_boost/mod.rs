use concurrency::{Lazy, MutexExt};
use foundation_serialization::json;
use foundation_serialization::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc, Mutex, Weak,
};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(feature = "telemetry")]
use crate::telemetry::{
    MESH_PEER_CONNECTED_TOTAL, MESH_PEER_LATENCY_MS, RANGE_BOOST_ENQUEUE_ERROR_TOTAL,
    RANGE_BOOST_FORWARDER_FAIL_TOTAL, RANGE_BOOST_TOGGLE_LATENCY_SECONDS,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopProof {
    pub relay: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub payload: Vec<u8>,
    pub proofs: Vec<HopProof>,
}

#[derive(Clone, Debug)]
struct QueueEntry {
    bundle: Bundle,
    enqueued_at: Instant,
}

pub struct RangeBoost {
    queue: VecDeque<QueueEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshPeer {
    pub addr: String,
    pub latency_ms: u128,
}

static PEER_LATENCY: Lazy<Mutex<HashMap<String, u128>>> = Lazy::new(|| Mutex::new(HashMap::new()));
static MESH_TASK_ACTIVE: AtomicBool = AtomicBool::new(false);
static RANGE_BOOST_ENABLED: AtomicBool = AtomicBool::new(false);
static FORWARDER_FAULT_MODE: AtomicU8 = AtomicU8::new(FaultMode::None as u8);
static ENQUEUE_ERROR: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "telemetry")]
static LAST_TOGGLE: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));

fn record_queue_metrics(queue: &VecDeque<QueueEntry>) {
    #[cfg(feature = "telemetry")]
    {
        crate::telemetry::RANGE_BOOST_QUEUE_DEPTH.set(queue.len() as i64);
        let oldest = queue
            .front()
            .map(|entry| entry.enqueued_at.elapsed().as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0);
        crate::telemetry::RANGE_BOOST_QUEUE_OLDEST_SECONDS.set(oldest);
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = queue;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultMode {
    None = 0,
    ForceDisabled = 1,
    ForceNoPeers = 2,
    ForceEncode = 3,
    ForceIo = 4,
}

#[derive(Default)]
struct ForwarderState {
    queue: Option<Weak<Mutex<RangeBoost>>>,
    handle: Option<ForwarderHandle>,
}

struct ForwarderHandle {
    shutdown: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ForwarderHandle {
    fn spawn(queue: Weak<Mutex<RangeBoost>>) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let handle = thread::spawn(move || forwarder_loop(thread_shutdown, queue));
        Self {
            shutdown,
            thread: Some(handle),
        }
    }

    fn stop(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            if let Err(err) = handle.join() {
                diagnostics::log::warn!(format!("range_boost_forwarder_join_failed: {:?}", err));
            }
        }
    }
}

static FORWARDER_STATE: Lazy<Mutex<ForwarderState>> =
    Lazy::new(|| Mutex::new(ForwarderState::default()));

fn ensure_forwarder_running(state: &mut ForwarderState, queue: Weak<Mutex<RangeBoost>>) {
    if let Some(handle) = state.handle.as_mut() {
        if let Some(thread) = handle.thread.as_mut() {
            if !thread.is_finished() {
                return;
            }
        }
        if let Some(old) = state.handle.take() {
            old.stop();
        }
    }
    state.handle = Some(ForwarderHandle::spawn(queue));
}

fn update_forwarder_state(enabled: bool) {
    if enabled {
        let mut state = FORWARDER_STATE.lock().unwrap();
        if let Some(queue) = state.queue.clone() {
            ensure_forwarder_running(&mut state, queue);
        }
    } else {
        let handle = {
            let mut state = FORWARDER_STATE.lock().unwrap();
            state.handle.take()
        };
        if let Some(handle) = handle {
            handle.stop();
        }
    }
}

fn sleep_with_shutdown(shutdown: &Arc<AtomicBool>, duration: Duration) {
    const CHECK_INTERVAL: Duration = Duration::from_millis(50);
    let mut elapsed = Duration::from_millis(0);
    while elapsed < duration {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let remaining = duration.saturating_sub(elapsed);
        let step = if remaining > CHECK_INTERVAL {
            CHECK_INTERVAL
        } else {
            remaining
        };
        thread::sleep(step);
        elapsed += step;
    }
}

fn forwarder_loop(shutdown: Arc<AtomicBool>, weak_queue: Weak<Mutex<RangeBoost>>) {
    while !shutdown.load(Ordering::SeqCst) {
        let Some(queue) = Weak::upgrade(&weak_queue) else {
            break;
        };
        let entry = {
            let mut guard = queue.lock().unwrap();
            guard.dequeue()
        };
        match entry {
            Some(entry) => match forward_bundle(&entry.bundle) {
                Ok(()) => {}
                Err(err) => {
                    {
                        let mut guard = queue.lock().unwrap();
                        guard.requeue_front(entry);
                    }
                    #[cfg(feature = "telemetry")]
                    RANGE_BOOST_FORWARDER_FAIL_TOTAL.inc();
                    match &err {
                        ForwardError::Disabled => sleep_with_shutdown(&shutdown, DISABLED_SLEEP),
                        ForwardError::NoPeers => sleep_with_shutdown(&shutdown, RETRY_SLEEP),
                        ForwardError::UnsupportedTransport(addr) => {
                            diagnostics::log::warn!(
                                "range_boost_forward_unsupported transport={addr}"
                            );
                            sleep_with_shutdown(&shutdown, RETRY_SLEEP);
                        }
                        ForwardError::Encode => {
                            diagnostics::log::warn!("range_boost_forward_encode_failed");
                            sleep_with_shutdown(&shutdown, RETRY_SLEEP);
                        }
                        ForwardError::Io(err) => {
                            diagnostics::log::warn!(format!("range_boost_forward_io_error: {err}"));
                            sleep_with_shutdown(&shutdown, RETRY_SLEEP);
                        }
                    }
                }
            },
            None => sleep_with_shutdown(&shutdown, IDLE_SLEEP),
        }
    }
}

pub fn mesh_active() -> bool {
    MESH_TASK_ACTIVE.load(Ordering::SeqCst)
}

pub fn set_enabled(v: bool) {
    #[cfg(feature = "telemetry")]
    {
        let now = Instant::now();
        let mut guard = LAST_TOGGLE.lock().unwrap();
        if let Some(previous) = *guard {
            let delta = now.saturating_duration_since(previous);
            RANGE_BOOST_TOGGLE_LATENCY_SECONDS.observe(delta.as_secs_f64());
        }
        *guard = Some(now);
    }
    RANGE_BOOST_ENABLED.store(v, Ordering::SeqCst);
    update_forwarder_state(v);
}

pub fn is_enabled() -> bool {
    RANGE_BOOST_ENABLED.load(Ordering::SeqCst)
}

pub fn set_forwarder_fault_mode(mode: FaultMode) {
    FORWARDER_FAULT_MODE.store(mode as u8, Ordering::SeqCst);
}

pub fn inject_enqueue_error() {
    ENQUEUE_ERROR.store(true, Ordering::SeqCst);
}

fn current_fault_mode() -> FaultMode {
    match FORWARDER_FAULT_MODE.load(Ordering::SeqCst) {
        1 => FaultMode::ForceDisabled,
        2 => FaultMode::ForceNoPeers,
        3 => FaultMode::ForceEncode,
        4 => FaultMode::ForceIo,
        _ => FaultMode::None,
    }
}

fn record_latency(addr: String, latency: u128) {
    let mut guard = PEER_LATENCY.guard();
    let is_new = !guard.contains_key(&addr);
    guard.insert(addr.clone(), latency);
    #[cfg(feature = "telemetry")]
    {
        if is_new {
            MESH_PEER_CONNECTED_TOTAL
                .ensure_handle_for_label_values(&[&addr])
                .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
                .inc();
        }
        MESH_PEER_LATENCY_MS
            .ensure_handle_for_label_values(&[&addr])
            .expect(crate::telemetry::LABEL_REGISTRATION_ERR)
            .set(latency as i64);
    }
    #[cfg(not(feature = "telemetry"))]
    let _ = is_new;
}

pub fn peer_latency(addr: &SocketAddr) -> Option<u128> {
    let map = PEER_LATENCY.guard();
    map.get(&addr.to_string()).cloned()
}

pub fn best_peer() -> Option<MeshPeer> {
    let map = PEER_LATENCY.guard();
    map.iter().min_by_key(|(_, l)| **l).map(|(a, l)| MeshPeer {
        addr: a.clone(),
        latency_ms: *l,
    })
}

pub fn peers() -> Vec<MeshPeer> {
    let map = PEER_LATENCY.guard();
    let mut peers: Vec<MeshPeer> = map
        .iter()
        .map(|(a, l)| MeshPeer {
            addr: a.clone(),
            latency_ms: *l,
        })
        .collect();
    peers.sort_by_key(|p| p.latency_ms);
    peers
}

pub fn discover_peers() -> Vec<MeshPeer> {
    MESH_TASK_ACTIVE.store(true, Ordering::SeqCst);
    let peers_env = std::env::var("TB_MESH_STATIC_PEERS").unwrap_or_default();
    let mut peers = Vec::new();
    for addr in peers_env.split(',').filter(|a| !a.is_empty()) {
        if addr.starts_with("unix:") {
            #[cfg(unix)]
            {
                let path = &addr[5..];
                let start = Instant::now();
                if let Ok(mut stream) = UnixStream::connect(path) {
                    let _ = stream.write_all(&[0u8]);
                    let mut buf = [0u8; 1];
                    let _ = stream.read(&mut buf);
                    let latency = start.elapsed().as_millis();
                    record_latency(addr.to_string(), latency);
                    peers.push(MeshPeer {
                        addr: addr.to_string(),
                        latency_ms: latency,
                    });
                }
            }
        } else if let Ok(sock) = addr.parse::<SocketAddr>() {
            let start = Instant::now();
            if let Ok(mut stream) = TcpStream::connect_timeout(&sock, Duration::from_millis(100)) {
                let _ = stream.write_all(&[0u8]);
                let mut buf = [0u8; 1];
                let _ = stream.read(&mut buf);
                let latency = start.elapsed().as_millis();
                record_latency(sock.to_string(), latency);
                peers.push(MeshPeer {
                    addr: sock.to_string(),
                    latency_ms: latency,
                });
            }
        } else if addr.starts_with("bt:") {
            #[cfg(all(feature = "bluetooth", any(target_os = "linux", target_os = "macos")))]
            {
                let latency = 0;
                record_latency(addr.to_string(), latency);
                peers.push(MeshPeer {
                    addr: addr.to_string(),
                    latency_ms: latency,
                });
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        peers.extend(discover_wifi_peers());
        peers.extend(discover_bt_peers());
    }
    peers.sort_by_key(|p| p.latency_ms);
    MESH_TASK_ACTIVE.store(false, Ordering::SeqCst);
    peers
}

#[cfg(target_os = "linux")]
fn discover_bt_peers() -> Vec<MeshPeer> {
    let mut out = Vec::new();
    if let Ok(res) = Command::new("hcitool").arg("scan").output() {
        let text = String::from_utf8_lossy(&res.stdout);
        for line in text.lines().skip(1) {
            if let Some(addr) = line.split_whitespace().next() {
                let peer = format!("bt:{}", addr);
                record_latency(peer.clone(), 0);
                out.push(MeshPeer {
                    addr: peer,
                    latency_ms: 0,
                });
            }
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn discover_bt_peers() -> Vec<MeshPeer> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn discover_wifi_peers() -> Vec<MeshPeer> {
    let mut out = Vec::new();
    if let Ok(res) = Command::new("iwlist").arg("scan").output() {
        let text = String::from_utf8_lossy(&res.stdout);
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with("Cell") {
                if let Some(addr) = l.split_whitespace().nth(4) {
                    let peer = format!("wifi:{}", addr);
                    record_latency(peer.clone(), 0);
                    out.push(MeshPeer {
                        addr: peer,
                        latency_ms: 0,
                    });
                }
            }
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn discover_wifi_peers() -> Vec<MeshPeer> {
    Vec::new()
}

pub fn parse_discovery_packet(data: &[u8]) -> Option<MeshPeer> {
    let s = std::str::from_utf8(data).ok()?;
    let mut parts = s.split(',');
    let addr = parts.next()?.to_string();
    let latency_ms = parts.next()?.parse().ok()?;
    Some(MeshPeer { addr, latency_ms })
}

impl RangeBoost {
    pub fn new() -> Self {
        let queue = VecDeque::new();
        record_queue_metrics(&queue);
        Self { queue }
    }

    pub fn enqueue(&mut self, payload: Vec<u8>) {
        if ENQUEUE_ERROR.swap(false, Ordering::SeqCst) {
            #[cfg(feature = "telemetry")]
            RANGE_BOOST_ENQUEUE_ERROR_TOTAL.inc();
            return;
        }
        self.queue.push_back(QueueEntry {
            bundle: Bundle {
                payload,
                proofs: vec![],
            },
            enqueued_at: Instant::now(),
        });
        record_queue_metrics(&self.queue);
    }

    pub fn record_proof(&mut self, idx: usize, proof: HopProof) {
        if let Some(entry) = self.queue.get_mut(idx) {
            entry.bundle.proofs.push(proof);
        }
    }

    pub fn dequeue(&mut self) -> Option<QueueEntry> {
        let entry = self.queue.pop_front();
        record_queue_metrics(&self.queue);
        entry
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn requeue_front(&mut self, entry: QueueEntry) {
        self.queue.push_front(entry);
        record_queue_metrics(&self.queue);
    }
}

#[derive(Debug)]
enum ForwardError {
    Disabled,
    NoPeers,
    UnsupportedTransport(String),
    Encode,
    Io(std::io::Error),
}

const IDLE_SLEEP: Duration = Duration::from_millis(200);
const RETRY_SLEEP: Duration = Duration::from_millis(250);
const DISABLED_SLEEP: Duration = Duration::from_millis(1000);

fn forward_bundle(bundle: &Bundle) -> Result<(), ForwardError> {
    match current_fault_mode() {
        FaultMode::None => {}
        FaultMode::ForceDisabled => return Err(ForwardError::Disabled),
        FaultMode::ForceNoPeers => return Err(ForwardError::NoPeers),
        FaultMode::ForceEncode => return Err(ForwardError::Encode),
        FaultMode::ForceIo => {
            return Err(ForwardError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "range boost fault injection",
            )))
        }
    }
    if !is_enabled() {
        return Err(ForwardError::Disabled);
    }
    let peer = best_peer().ok_or(ForwardError::NoPeers)?;
    let payload = json::to_vec(bundle).map_err(|_| ForwardError::Encode)?;
    send_to_peer(&peer.addr, &payload).map_err(|err| {
        if let std::io::ErrorKind::Unsupported = err.kind() {
            ForwardError::UnsupportedTransport(peer.addr)
        } else {
            ForwardError::Io(err)
        }
    })
}

fn send_to_peer(addr: &str, payload: &[u8]) -> std::io::Result<()> {
    if let Some(path) = addr.strip_prefix("unix:") {
        #[cfg(unix)]
        {
            let mut stream = UnixStream::connect(path)?;
            let len = payload.len() as u32;
            stream.write_all(&len.to_le_bytes())?;
            stream.write_all(payload)?;
            stream.flush()?;
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "unix sockets not supported on this platform",
            ));
        }
    }
    if addr.starts_with("bt:") || addr.starts_with("wifi:") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "range boost transport not implemented",
        ));
    }
    let socket: SocketAddr = addr.parse().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid mesh peer address: {addr}"),
        )
    })?;
    let mut stream = TcpStream::connect_timeout(&socket, Duration::from_millis(500))?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

pub fn spawn_forwarder(queue: &Arc<Mutex<RangeBoost>>) {
    let weak_queue = Arc::downgrade(queue);
    {
        let mut state = FORWARDER_STATE.lock().unwrap();
        state.queue = Some(weak_queue.clone());
        if is_enabled() {
            ensure_forwarder_running(&mut state, weak_queue);
        } else {
            diagnostics::log::info!("range_boost_forwarder_skipped disabled=true");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_roundtrip() {
        let mut rb = RangeBoost::new();
        rb.enqueue(vec![1, 2, 3]);
        assert_eq!(rb.pending(), 1);
        rb.record_proof(0, HopProof { relay: "r1".into() });
        let b = rb.dequeue().unwrap();
        assert_eq!(b.payload, vec![1, 2, 3]);
        assert_eq!(b.proofs.len(), 1);
    }
}
