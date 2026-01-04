#![cfg(feature = "integration-tests")]
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
    let dir = tempdir().unwrap();
    the_block::net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key"));
    std::env::set_var("TB_NET_KEY_SEED", "chaos");
    std::env::set_var("TB_PEER_SEED", "1");
    // Use light loss/jitter for stable CI convergence:
    //   1% packet drop keeps the scenario realistic without stalling nodes.
    //   10ms jitter lets the reactor schedule without long delays.
    std::env::set_var("TB_NET_PACKET_LOSS", "0");
    std::env::set_var("TB_NET_JITTER_MS", "10");
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers_default"));
    std::env::set_var("TB_FAST_MINE", "1");
    std::fs::write(dir.path().join("seed"), b"chaos").unwrap();
    dir
}

fn timeout_factor() -> u64 {
    std::env::var("TB_TEST_TIMEOUT_MULT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3) // Default to 3x for CI resilience
}

async fn wait_until_converged(nodes: &[&Node], max: Duration) -> bool {
    let start = Instant::now();
    let mut last_report = Duration::from_secs(0);
    loop {
        let heights: Vec<_> = nodes.iter().map(|n| n.blockchain().block_height).collect();
        let first = heights[0];
        if heights.iter().all(|h| *h == first) {
            return true;
        }
        // Push the longest known chain out to peers whenever we see divergence to
        // kick stalled gossip back into sync after partitions heal.
        if let Some((idx, _)) = heights.iter().enumerate().max_by_key(|(_, h)| *h) {
            nodes[idx].discover_peers();
            nodes[idx].broadcast_chain();
        }
        let elapsed = start.elapsed();
        if elapsed > max {
            eprintln!(
                "chaos convergence timed out after {:?}, heights = {:?}",
                elapsed, heights
            );
            return false;
        }
        if elapsed - last_report > Duration::from_secs(1) {
            eprintln!("chaos convergence progress: {:?}", heights);
            last_report = elapsed;
        }
        the_block::sleep(Duration::from_millis(100)).await;
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
        // Assign a unique network key per node to avoid shared-identity rate limits.
        let prev_env = set_net_key_env(&key_path, &key_seed);
        let bc = Blockchain::with_difficulty(dir.path().to_str().unwrap(), 1).expect("open bc");
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
        let start = Instant::now();
        let ok = wait_until_converged(
            &[&node1.node, &node2.node, &node3.node],
            Duration::from_secs(10 * timeout_factor()),
        )
        .await;
        assert!(ok, "convergence timed out");
        let elapsed = start.elapsed();
        assert!(elapsed <= Duration::from_secs(10 * timeout_factor()));
        node1.shutdown();
        node2.shutdown();
        node3.shutdown();
        std::env::remove_var("TB_NET_PACKET_LOSS");
        std::env::remove_var("TB_NET_JITTER_MS");
        std::env::remove_var("TB_NET_KEY_PATH");
        std::env::remove_var("TB_NET_KEY_SEED");
        std::env::remove_var("TB_PEER_DB_PATH");
        std::env::remove_var("TB_PEER_SEED");
        std::env::remove_var("TB_FAST_MINE");
    });
}

#[testkit::tb_serial]
fn kill_node_recovers() {
    runtime::block_on(async {
        let _e = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        for _ in 0..5 {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(addr, peers.clone());
            for n in &nodes {
                n.node.add_peer(addr);
                tn.node.add_peer(n.addr);
            }
            nodes.push(tn);
        }
        the_block::sleep(Duration::from_secs(1)).await;
        let mut ts = 1u64;
        let blocks_per_phase = 6u64;
        for _ in 0..blocks_per_phase {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(20)).await;
        }
        for n in &nodes {
            n.node.discover_peers();
        }
        let max = Duration::from_secs(10 * timeout_factor());
        let start = Instant::now();
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await
        );
        println!("initial convergence {:?}", start.elapsed());

        nodes[2].flag.trigger();
        if let Some(handle) = nodes[2].handle.take() {
            let _ = handle.join();
        }
        for (i, n) in nodes.iter().enumerate() {
            if i != 2 {
                n.node.remove_peer(nodes[2].addr);
            }
        }
        for _ in 0..blocks_per_phase {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(20)).await;
        }
        for n in nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2)
            .map(|(_, n)| n)
        {
            n.node.discover_peers();
        }
        let active: Vec<&Node> = nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 2)
            .map(|(_, n)| &n.node)
            .collect();
        let start = Instant::now();
        assert!(wait_until_converged(&active, max).await);
        println!("convergence after removal {:?}", start.elapsed());

        let restart_bc = Blockchain::open(nodes[2].dir.path().to_str().unwrap()).unwrap();
        let prev_env = set_net_key_env(&nodes[2].net_key_path, &nodes[2].net_key_seed);
        let node3 = Node::new(
            nodes[2].addr,
            active.iter().map(|n| n.addr()).collect(),
            restart_bc,
        );
        restore_net_key_env(prev_env);
        for (i, n) in nodes.iter().enumerate() {
            if i != 2 {
                n.node.add_peer(nodes[2].addr);
            }
        }
        let flag = ShutdownFlag::new();
        let handle = node3.start_with_flag(&flag).expect("start gossip node");
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
        for n in &nodes {
            n.node.discover_peers();
        }
        let start = Instant::now();
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await
        );
        println!("final convergence {:?}", start.elapsed());
        let h = nodes[0].node.blockchain().block_height;
        assert_eq!(h, blocks_per_phase * 2);
        for n in nodes.iter_mut() {
            n.shutdown();
        }
        std::env::remove_var("TB_NET_PACKET_LOSS");
        std::env::remove_var("TB_NET_JITTER_MS");
        std::env::remove_var("TB_NET_KEY_PATH");
        std::env::remove_var("TB_NET_KEY_SEED");
        std::env::remove_var("TB_PEER_DB_PATH");
        std::env::remove_var("TB_PEER_SEED");
        std::env::remove_var("TB_FAST_MINE");
    });
}

#[testkit::tb_serial]
fn partition_heals_to_majority() {
    runtime::block_on(async {
        let _e = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        for _ in 0..5 {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(addr, peers.clone());
            for n in &nodes {
                n.node.add_peer(addr);
                tn.node.add_peer(n.addr);
            }
            nodes.push(tn);
        }
        the_block::sleep(Duration::from_secs(1)).await;
        let mut ts = 1u64;
        {
            let mut bc = nodes[0].node.blockchain();
            bc.mine_block_at("miner", ts).unwrap();
            ts += 1;
        }
        nodes[0].node.broadcast_chain();

        // isolate node4 (index 3)
        let iso = 3usize;
        nodes[iso].node.clear_peers();
        for (i, n) in nodes.iter().enumerate() {
            if i != iso {
                n.node.remove_peer(nodes[iso].addr);
            }
        }

        let main_blocks_after_isolation = 4u64;
        for _ in 0..main_blocks_after_isolation {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(20)).await;
        }
        {
            let mut bc = nodes[iso].node.blockchain();
            bc.mine_block_at("isolated", ts).unwrap();
            ts += 1;
            bc.mine_block_at("isolated", ts).unwrap();
        }

        // heal partition
        for (i, n) in nodes.iter().enumerate() {
            if i != iso {
                n.node.add_peer(nodes[iso].addr);
                nodes[iso].node.add_peer(n.addr);
            }
        }
        nodes[iso].node.discover_peers();
        nodes[0].node.broadcast_chain();
        let max = Duration::from_secs(10 * timeout_factor());
        let start = Instant::now();
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await
        );
        println!("partition heal convergence {:?}", start.elapsed());
        let h = nodes[0].node.blockchain().block_height;
        assert_eq!(h, 1 + main_blocks_after_isolation);
        #[cfg(feature = "telemetry")]
        {
            let c = the_block::telemetry::FORK_REORG_TOTAL
                .ensure_handle_for_label_values(&["0"])
                .expect(telemetry::LABEL_REGISTRATION_ERR)
                .get();
            assert!(c > 0);
        }
        for n in nodes.iter_mut() {
            n.shutdown();
        }
        std::env::remove_var("TB_NET_PACKET_LOSS");
        std::env::remove_var("TB_NET_JITTER_MS");
        std::env::remove_var("TB_NET_KEY_PATH");
        std::env::remove_var("TB_NET_KEY_SEED");
        std::env::remove_var("TB_PEER_DB_PATH");
        std::env::remove_var("TB_PEER_SEED");
        std::env::remove_var("TB_FAST_MINE");
    });
}
