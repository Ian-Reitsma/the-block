#![cfg(feature = "integration-tests")]
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use sys::tempfile::tempdir;
#[cfg(feature = "telemetry")]
use the_block::telemetry;
use the_block::{net::Node, Blockchain, ShutdownFlag};

static NODE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn set_net_key_env(path: &Path, seed: &str) -> (Option<String>, Option<String>) {
    let prev_key_path = std::env::var("TB_NET_KEY_PATH").ok();
    let prev_key_seed = std::env::var("TB_NET_KEY_SEED").ok();
    std::env::set_var("TB_NET_KEY_PATH", path);
    std::env::set_var("TB_NET_KEY_SEED", seed);
    (prev_key_path, prev_key_seed)
}

fn restore_net_key_env(prev: (Option<String>, Option<String>)) {
    match prev.0 {
        Some(value) => std::env::set_var("TB_NET_KEY_PATH", value),
        None => std::env::remove_var("TB_NET_KEY_PATH"),
    }
    match prev.1 {
        Some(value) => std::env::set_var("TB_NET_KEY_SEED", value),
        None => std::env::remove_var("TB_NET_KEY_SEED"),
    }
}

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn init_env() -> sys::tempfile::TempDir {
    cleanup_env();
    let dir = tempdir().unwrap();
    the_block::net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key"));
    std::env::set_var("TB_NET_KEY_SEED", "chaos");
    std::env::set_var("TB_PEER_SEED", "1");
    // Relax rate limits to avoid drops during handshake storm
    std::env::set_var("TB_P2P_MAX_PER_SEC", "100000");
    // Disable periodic chain sync to reduce noise
    std::env::set_var("TB_P2P_CHAIN_SYNC_INTERVAL_MS", "0");
    // Use fast mining
    std::env::set_var("TB_FAST_MINE", "1");
    // Light chaos: 1% packet loss, 50ms jitter
    std::env::set_var("TB_NET_PACKET_LOSS", "0.01");
    std::env::set_var("TB_NET_JITTER_MS", "50");
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers_default"));
    std::fs::write(dir.path().join("seed"), b"chaos").unwrap();
    dir
}

fn timeout_factor() -> u64 {
    let mult = std::env::var("TB_TEST_TIMEOUT_MULT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    mult.clamp(1, 8)
}

fn cleanup_env() {
    std::env::remove_var("TB_NET_PACKET_LOSS");
    std::env::remove_var("TB_NET_JITTER_MS");
    std::env::remove_var("TB_NET_KEY_PATH");
    std::env::remove_var("TB_NET_KEY_SEED");
    std::env::remove_var("TB_PEER_DB_PATH");
    std::env::remove_var("TB_PEER_SEED");
    std::env::remove_var("TB_FAST_MINE");
    std::env::remove_var("TB_P2P_MAX_PER_SEC");
    std::env::remove_var("TB_P2P_CHAIN_SYNC_INTERVAL_MS");
}

async fn wait_until_converged(nodes: &[&Node], max: Duration) -> bool {
    let start = Instant::now();
    let mut last_broadcast = Instant::now();
    let mut last_request: HashMap<SocketAddr, Instant> = HashMap::new();
    loop {
        let heights: Vec<_> = nodes.iter().map(|n| n.blockchain().block_height).collect();
        let first = heights[0];
        if heights.iter().all(|h| *h == first) {
            return true;
        }
        if start.elapsed() > max {
            eprintln!("Convergence timeout: heights={:?}", heights);
            return false;
        }
        // Under packet loss, actively broadcast longest chain every 200ms
        if last_broadcast.elapsed() > Duration::from_millis(200) {
            if let Some((idx, _)) = heights.iter().enumerate().max_by_key(|(_, h)| *h) {
                let leader = nodes[idx];
                leader.broadcast_chain();
                let leader_addr = leader.addr();
                for (peer_idx, node) in nodes.iter().enumerate() {
                    if peer_idx != idx && heights[peer_idx] < heights[idx] {
                        let now = Instant::now();
                        let addr = node.addr();
                        let should_request = last_request
                            .get(&addr)
                            .map(|ts| now.duration_since(*ts) > Duration::from_millis(500))
                            .unwrap_or(true);
                        if should_request {
                            node.request_chain_from(leader_addr, heights[peer_idx]);
                            last_request.insert(addr, now);
                        }
                    }
                }
            }
            last_broadcast = Instant::now();
        }
        the_block::sleep(Duration::from_millis(20)).await;
    }
}

struct TestNode {
    addr: SocketAddr,
    dir: sys::tempfile::TempDir,
    node: Node,
    net_key_path: std::path::PathBuf,
    net_key_seed: String,
    flag: ShutdownFlag,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestNode {
    fn new(addr: SocketAddr, peers: Vec<SocketAddr>) -> Self {
        let dir = tempdir().unwrap();
        let node_id = NODE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let key_path = dir.path().join(format!("net_key_{node_id}"));
        let key_seed = format!("chaos-{node_id}");
        let prev_env = set_net_key_env(&key_path, &key_seed);
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));
        let bc = Blockchain::open(dir.path().to_str().unwrap()).expect("open bc");
        let node = Node::new(addr, peers, bc);
        restore_net_key_env(prev_env);
        let flag = ShutdownFlag::new();
        let handle = node.start_with_flag(&flag).expect("start gossip node");
        node.discover_peers();
        Self {
            addr,
            dir,
            node,
            net_key_path: key_path,
            net_key_seed: key_seed,
            flag,
            handle: Some(handle),
        }
    }

    fn shutdown(&mut self) {
        self.flag.trigger();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[testkit::tb_serial]
fn converges_under_loss() {
    runtime::block_on(async {
        let _env = init_env();
        let addr1 = free_addr();
        let addr2 = free_addr();
        let addr3 = free_addr();
        let mut node1 = TestNode::new(addr1, vec![addr2, addr3]);
        let mut node2 = TestNode::new(addr2, vec![addr1, addr3]);
        let mut node3 = TestNode::new(addr3, vec![addr1, addr2]);

        // Wait for handshakes to complete
        the_block::sleep(Duration::from_millis(1000)).await;

        // Ensure all peers know each other
        node1.node.add_peer(addr2);
        node1.node.add_peer(addr3);
        node2.node.add_peer(addr1);
        node2.node.add_peer(addr3);
        node3.node.add_peer(addr1);
        node3.node.add_peer(addr2);
        the_block::sleep(Duration::from_millis(500)).await;

        // Mine blocks on node1
        let mut ts = 1u64;
        for _ in 0..3 {
            {
                let mut bc = node1.node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            node1.node.broadcast_chain();
            the_block::sleep(Duration::from_millis(100)).await;
        }

        let ok = wait_until_converged(
            &[&node1.node, &node2.node, &node3.node],
            Duration::from_secs(10 * timeout_factor()),
        )
        .await;
        assert!(ok, "convergence timed out");

        node1.shutdown();
        node2.shutdown();
        node3.shutdown();
        cleanup_env();
    });
}

#[testkit::tb_serial]
fn kill_node_recovers() {
    runtime::block_on(async {
        let _e = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        let node_count = 3usize;
        for _ in 0..node_count {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(addr, peers.clone());
            for n in &nodes {
                n.node.add_peer(addr);
                tn.node.add_peer(n.addr);
            }
            nodes.push(tn);
        }

        // Wait for handshakes
        the_block::sleep(Duration::from_millis(1500)).await;

        // Ensure full mesh connectivity
        for i in 0..nodes.len() {
            for j in 0..nodes.len() {
                if i != j {
                    nodes[i].node.add_peer(nodes[j].addr);
                }
            }
        }
        the_block::sleep(Duration::from_millis(500)).await;

        // Phase 1: Mine 6 blocks
        let mut ts = 1u64;
        for _ in 0..6 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(50)).await;
        }
        let max = Duration::from_secs(18 * timeout_factor());
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await,
            "initial convergence failed"
        );

        // Kill node 2
        nodes[2].flag.trigger();
        if let Some(handle) = nodes[2].handle.take() {
            let _ = handle.join();
        }

        // Wait for socket to be fully released
        the_block::sleep(Duration::from_millis(500)).await;
        for (i, n) in nodes.iter().enumerate() {
            if i != 2 {
                n.node.remove_peer(nodes[2].addr);
            }
        }

        // Phase 2: Mine 6 more blocks
        for _ in 0..6 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(50)).await;
        }
        let active: Vec<&Node> = nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2)
            .map(|(_, n)| &n.node)
            .collect();
        assert!(
            wait_until_converged(&active, max).await,
            "convergence after kill failed"
        );

        // Restart node 2
        let restart_bc = Blockchain::open(nodes[2].dir.path().to_str().unwrap()).unwrap();
        let prev_env = set_net_key_env(&nodes[2].net_key_path, &nodes[2].net_key_seed);
        // Collect peer addresses before creating new node
        let peer_addrs: Vec<SocketAddr> = nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2)
            .map(|(_, n)| n.addr)
            .collect();
        let node3 = Node::new(nodes[2].addr, peer_addrs, restart_bc);
        restore_net_key_env(prev_env);
        let flag = ShutdownFlag::new();
        let handle = node3.start_with_flag(&flag).expect("start gossip node");
        for (i, n) in nodes.iter().enumerate() {
            if i != 2 {
                n.node.add_peer(nodes[2].addr);
            }
        }
        node3.discover_peers();
        let addr = nodes[2].addr;
        let key_path = nodes[2].net_key_path.clone();
        let key_seed = nodes[2].net_key_seed.clone();
        let dir = std::mem::replace(&mut nodes[2].dir, tempdir().unwrap());
        nodes[2] = TestNode {
            addr,
            dir,
            node: node3,
            net_key_path: key_path,
            net_key_seed: key_seed,
            flag,
            handle: Some(handle),
        };

        // Wait longer for node restart and handshakes
        the_block::sleep(Duration::from_millis(1500)).await;

        // Broadcast from all nodes to help convergence
        for n in &nodes {
            n.node.broadcast_chain();
        }
        the_block::sleep(Duration::from_millis(500)).await;

        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await,
            "final convergence failed"
        );

        let h = nodes[0].node.blockchain().block_height;
        assert_eq!(h, 12, "Expected 12 blocks");
        for n in nodes.iter_mut() {
            n.shutdown();
        }
        cleanup_env();
    });
}

#[testkit::tb_serial]
fn partition_heals_to_majority() {
    runtime::block_on(async {
        let _e = init_env();
        let node_count = 3usize;
        let mut nodes: Vec<TestNode> = Vec::new();
        for _ in 0..node_count {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(addr, peers.clone());
            for n in &nodes {
                n.node.add_peer(addr);
                tn.node.add_peer(n.addr);
            }
            nodes.push(tn);
        }

        // Wait for handshakes
        the_block::sleep(Duration::from_millis(1500)).await;

        // Ensure full mesh connectivity
        for i in 0..nodes.len() {
            for j in 0..nodes.len() {
                if i != j {
                    nodes[i].node.add_peer(nodes[j].addr);
                }
            }
        }
        the_block::sleep(Duration::from_millis(500)).await;

        // Mine initial blocks before partition
        let mut ts = 1u64;
        for _ in 0..3 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(100)).await;
        }

        // Wait for initial convergence before creating partition
        let max = Duration::from_secs(10 * timeout_factor());
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await,
            "initial convergence before partition failed"
        );

        // Isolate node 3
        let iso = node_count - 1;
        nodes[iso].node.clear_peers();
        for (i, n) in nodes.iter().enumerate() {
            if i != iso {
                n.node.remove_peer(nodes[iso].addr);
            }
        }

        // Main partition mines 6 blocks
        for _ in 0..6 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(50)).await;
        }
        // Isolated node mines 2 blocks (shorter chain)
        {
            let mut bc = nodes[iso].node.blockchain();
            bc.mine_block_at("isolated", ts).unwrap();
            ts += 1;
            bc.mine_block_at("isolated", ts).unwrap();
        }

        // Capture reorg metric baseline before healing partition
        #[cfg(feature = "telemetry")]
        let reorg_baseline = the_block::telemetry::FORK_REORG_TOTAL
            .ensure_handle_for_label_values(&["2"])
            .expect(telemetry::LABEL_REGISTRATION_ERR)
            .get();

        // Heal partition
        for (i, n) in nodes.iter().enumerate() {
            if i != iso {
                n.node.add_peer(nodes[iso].addr);
                nodes[iso].node.add_peer(n.addr);
            }
        }
        for n in &nodes {
            n.node.discover_peers();
        }

        // Give time for handshakes to complete after re-connecting
        the_block::sleep(Duration::from_millis(800)).await;

        // Ensure all nodes have complete peer lists after re-connecting
        for i in 0..nodes.len() {
            for j in 0..nodes.len() {
                if i != j {
                    nodes[i].node.add_peer(nodes[j].addr);
                }
            }
        }
        the_block::sleep(Duration::from_millis(500)).await;

        // Trigger chain exchange after partition merge
        for n in &nodes {
            n.node.broadcast_chain();
        }
        // Explicitly request tips a few times to accelerate convergence
        // Actively pull the tip from the known leader to the isolated node
        let leader_addr = nodes[0].addr;
        for _ in 0..3 {
            let iso_height = nodes[iso].node.blockchain().block_height;
            nodes[iso].node.request_chain_from(leader_addr, iso_height);
            the_block::sleep(Duration::from_millis(400)).await;
            for n in &nodes {
                n.node.broadcast_chain();
            }
        }

        let max = Duration::from_secs(30 * timeout_factor());
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await,
            "partition heal failed"
        );

        let h = nodes[0].node.blockchain().block_height;
        assert_eq!(
            h, 9,
            "Expected majority chain (9 blocks: 3 pre-partition + 6 during) to win"
        );

        #[cfg(feature = "telemetry")]
        {
            // Isolated node mined 2 blocks, so reorg depth is 2
            let c = the_block::telemetry::FORK_REORG_TOTAL
                .ensure_handle_for_label_values(&["2"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get();
            assert!(
                c > reorg_baseline,
                "Expected fork reorg metric at depth 2 to increase (before: {}, after: {})",
                reorg_baseline,
                c
            );
        }

        for n in nodes.iter_mut() {
            n.shutdown();
        }
        cleanup_env();
    });
}
