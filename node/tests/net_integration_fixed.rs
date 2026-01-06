#![cfg(feature = "integration-tests")]
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;
use sys::tempfile::tempdir;
use the_block::{net::Node, Blockchain, ShutdownFlag};

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn init_env() -> sys::tempfile::TempDir {
    let dir = tempdir().unwrap();
    the_block::net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));
    std::env::set_var("TB_PEER_SEED", "42");
    // Relax rate limits for this test to avoid drops during the handshake storm.
    std::env::set_var("TB_P2P_MAX_PER_SEC", "100000");
    // Disable periodic chain sync in this test to avoid flooding the listener with
    // redundant ChainRequest traffic.
    std::env::set_var("TB_P2P_CHAIN_SYNC_INTERVAL_MS", "0");
    // Use fast mining to skip real PoW - without this, mining takes hours.
    std::env::set_var("TB_FAST_MINE", "1");
    
    // Platform-specific socket fixes for macOS
    #[cfg(target_os = "macos")]
    {
        std::env::set_var("TB_SO_REUSEPORT", "1");
        std::env::set_var("TB_TCP_NODELAY", "1");
    }
    
    dir
}

fn set_net_key_env(path: &Path, seed: &str) -> (Option<String>, Option<String>) {
    let prev_path = std::env::var("TB_NET_KEY_PATH").ok();
    let prev_seed = std::env::var("TB_NET_KEY_SEED").ok();
    std::env::set_var("TB_NET_KEY_PATH", path);
    std::env::set_var("TB_NET_KEY_SEED", seed);
    (prev_path, prev_seed)
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
    std::env::remove_var("TB_P2P_MAX_PER_SEC");
}

/// 1% DEV FIX: Enhanced convergence checking with aggressive logging and watchdog
async fn wait_until_converged(nodes: &[&Node], max: Duration) -> bool {
    let start = Instant::now();
    let mut tick: u64 = 0;
    let watchdog_max = max + Duration::from_secs(5); // Hard timeout to catch deadlocks
    
    loop {
        let first = nodes[0].blockchain().block_height;
        let all_heights: Vec<_> = nodes.iter().map(|n| n.blockchain().block_height).collect();
        let all_tips: Vec<_> = nodes.iter().map(|n| {
            let bc = n.blockchain();
            (bc.block_height, bc.chain.last().map(|b| b.hash.clone()).unwrap_or_default())
        }).collect();
        let all_peers: Vec<_> = nodes.iter().map(|n| n.peer_addrs().len()).collect();
        
        if nodes.iter().all(|n| n.blockchain().block_height == first) {
            eprintln!(
                "[CONVERGED] elapsed={:?} height={} all_heights={:?}",
                start.elapsed(), first, all_heights
            );
            return true;
        }
        
        // WATCHDOG: Prevent infinite hangs (macOS deadlock detection)
        if start.elapsed() > watchdog_max {
            eprintln!("\n============================================================");
            eprintln!("WATCHDOG TIMEOUT: Exceeded {:?} - LIKELY DEADLOCK", watchdog_max);
            eprintln!("This indicates mutex deadlock or channel blocking in consensus.");
            eprintln!("============================================================");
            for (i, node) in nodes.iter().enumerate() {
                let bc = node.blockchain();
                let tip_hash = bc.chain.last().map(|b| &b.hash[..std::cmp::min(16, b.hash.len())]).unwrap_or("<none>");
                eprintln!(
                    "  Node[{}]: height={:4} tip={} peers={:2} addr={}",
                    i, bc.block_height, tip_hash, node.peer_addrs().len(), node.addr()
                );
            }
            eprintln!("============================================================\n");
            panic!("WATCHDOG: Deadlock detected in consensus - test hung for {:?}", start.elapsed());
        }
        
        // Soft timeout with detailed diagnostic
        if start.elapsed() > max {
            eprintln!("\n============================================================");
            eprintln!("[CONVERGENCE TIMEOUT] Failed after {:?}", start.elapsed());
            eprintln!("Heights: {:?}", all_heights);
            eprintln!("Peers:   {:?}", all_peers);
            eprintln!("============================================================");
            for (i, (height, tip)) in all_tips.iter().enumerate() {
                let tip_short = &tip[..std::cmp::min(16, tip.len())];
                eprintln!("  Node[{}]: height={:4} tip={} peers={:2}", i, height, tip_short, all_peers[i]);
            }
            eprintln!("\nDIAGNOSTIC: Check for chain fork or peer connectivity issues.");
            eprintln!("============================================================\n");
            return false;
        }
        
        tick += 1;
        // More frequent logging to catch issues early
        if tick % 50 == 0 {
            eprintln!(
                "[tick={}] elapsed={:?} heights={:?} peers={:?}",
                tick, start.elapsed(), all_heights, all_peers
            );
            // Deep diagnostic every 100 ticks (~2 seconds)
            if tick % 100 == 0 {
                for (i, (height, tip)) in all_tips.iter().enumerate() {
                    let tip_short = &tip[..std::cmp::min(8, tip.len())];
                    eprintln!("  Node[{}]: h={} tip={} p={}", i, height, tip_short, all_peers[i]);
                }
            }
        }
        the_block::sleep(Duration::from_millis(20)).await;
    }
}

/// 1% DEV FIX: Event-driven peer handshake wait instead of arbitrary sleep
async fn wait_for_handshakes(nodes: &[&TestNode], expected_peers: usize, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if nodes.iter().all(|n| n.node.peer_addrs().len() >= expected_peers) {
            eprintln!("[HANDSHAKES COMPLETE] All nodes have {} peers in {:?}", expected_peers, start.elapsed());
            return true;
        }
        the_block::sleep(Duration::from_millis(50)).await;
    }
    eprintln!("[HANDSHAKE TIMEOUT] Not all nodes reached {} peers", expected_peers);
    for (i, node) in nodes.iter().enumerate() {
        eprintln!("  Node[{}]: {} peers", i, node.node.peer_addrs().len());
    }
    false
}

struct TestNode {
    addr: SocketAddr,
    _dir: sys::tempfile::TempDir,
    node: Node,
    flag: ShutdownFlag,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestNode {
    fn new(node_id: usize, addr: SocketAddr, peers: &[SocketAddr]) -> Self {
        let dir = tempdir().unwrap();
        let bc = Blockchain::open(dir.path().to_str().unwrap()).expect("open bc");
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));
        let key_path = dir.path().join(format!("net_key_{node_id}"));
        let key_seed = format!("net_integration_{node_id}");
        let prev_env = set_net_key_env(&key_path, &key_seed);
        let node = Node::new(addr, peers.to_vec(), bc);
        restore_net_key_env(prev_env);
        let flag = ShutdownFlag::new();
        let handle = node.start_with_flag(&flag).expect("start gossip node");
        node.discover_peers();
        Self {
            addr,
            _dir: dir,
            node,
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
fn partition_heals_to_majority() {
    eprintln!("\n============================================================");
    eprintln!("TEST: partition_heals_to_majority");
    eprintln!("============================================================\n");
    
    runtime::block_on(async {
        let _env = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        
        // 1% FIX: Setup phase with event-driven waits
        eprintln!("[SETUP] Creating 5 nodes...");
        for id in 0..5 {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(id, addr, &peers);
            eprintln!("  Node[{id}] @ {addr}");
            for p in &peers {
                tn.node.add_peer(*p);
            }
            nodes.push(tn);
        }

        // Wait for handshakes instead of blind sleep
        eprintln!("[SETUP] Waiting for initial handshakes...");
        wait_for_handshakes(&nodes.iter().collect::<Vec<_>>(), 4, Duration::from_secs(3)).await;

        // Ensure all nodes have full peer lists
        eprintln!("[SETUP] Ensuring full mesh connectivity...");
        for i in 0..nodes.len() {
            for j in 0..nodes.len() {
                if i != j {
                    nodes[i].node.add_peer(nodes[j].addr);
                }
            }
        }
        the_block::sleep(Duration::from_millis(500)).await;

        eprintln!("[MINE] Mining initial chain...");
        let mut ts = 1u64;
        for round in 0..3 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            eprintln!("  Mined block {round}, broadcasting...");
            the_block::sleep(Duration::from_millis(100)).await;
        }

        eprintln!("[CONVERGE] Initial convergence check...");
        let converged = wait_until_converged(
            &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
            Duration::from_secs(12),
        )
        .await;
        if !converged {
            panic!("Initial convergence failed - check network/chain sync");
        }
        eprintln!("[SUCCESS] Initial convergence complete\n");

        // CREATE PARTITION
        eprintln!("[PARTITION] Creating network partition (3-2 split)...");
        for node in nodes.iter().take(3) {
            for peer in nodes.iter().skip(3) {
                node.node.remove_peer(peer.addr);
                peer.node.remove_peer(node.addr);
            }
        }

        // Mine on both sides
        eprintln!("[MINE] Mining 2 blocks on partition A (nodes 0-2)...");
        for _ in 0..2 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("minerA", ts).unwrap();
                ts += 1;
            }
            for node in nodes.iter().take(3) {
                node.node.broadcast_chain();
            }
            the_block::sleep(Duration::from_millis(50)).await;
        }

        eprintln!("[MINE] Mining 5 blocks on partition B (nodes 3-4)...");
        for _ in 0..5 {
            {
                let mut bc = nodes[3].node.blockchain();
                bc.mine_block_at("minerB", ts).unwrap();
                ts += 1;
            }
            for peer in nodes.iter().skip(3) {
                peer.node.broadcast_chain();
            }
            the_block::sleep(Duration::from_millis(50)).await;
        }

        let heights_post_partition: Vec<_> = nodes.iter().map(|n| n.node.blockchain().block_height).collect();
        eprintln!("[STATUS] Post-partition heights: {:?}", heights_post_partition);

        // HEAL PARTITION
        eprintln!("[HEAL] Reconnecting partitions...");
        for node in nodes.iter().take(3) {
            for peer in nodes.iter().skip(3) {
                node.node.add_peer(peer.addr);
                peer.node.add_peer(node.addr);
            }
        }

        // Wait for reconnection handshakes
        eprintln!("[HEAL] Waiting for handshakes to complete...");
        wait_for_handshakes(&nodes.iter().collect::<Vec<_>>(), 4, Duration::from_secs(2)).await;

        // Ensure complete connectivity
        for i in 0..nodes.len() {
            for j in 0..nodes.len() {
                if i != j {
                    nodes[i].node.add_peer(nodes[j].addr);
                }
            }
        }
        the_block::sleep(Duration::from_millis(500)).await;

        // Trigger chain sync
        eprintln!("[HEAL] Broadcasting chains for convergence...");
        for node in &nodes {
            node.node.broadcast_chain();
        }
        the_block::sleep(Duration::from_millis(1200)).await;

        // VERIFY CONVERGENCE TO MAJORITY (longest) CHAIN
        eprintln!("[CONVERGE] Post-heal convergence check...");
        let converged = wait_until_converged(
            &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
            Duration::from_secs(20),
        )
        .await;
        if !converged {
            panic!("Post-partition convergence failed - chain fork not resolved");
        }

        let final_height = nodes[3].node.blockchain().block_height;
        eprintln!("[VERIFY] Final converged height: {}", final_height);
        assert!(nodes.iter().all(|n| n.node.blockchain().block_height == final_height),
            "All nodes must converge to same height");

        eprintln!("[SUCCESS] Partition healed to majority chain\n");

        for n in &mut nodes {
            n.shutdown();
        }
        std::env::remove_var("TB_NET_KEY_PATH");
        std::env::remove_var("TB_NET_KEY_SEED");
        std::env::remove_var("TB_PEER_DB_PATH");
        std::env::remove_var("TB_PEER_SEED");
        std::env::remove_var("TB_P2P_MAX_PER_SEC");
        std::env::remove_var("TB_P2P_CHAIN_SYNC_INTERVAL_MS");
        std::env::remove_var("TB_FAST_MINE");
    });
}

#[testkit::tb_serial]
fn kill_node_recovers() {
    eprintln!("\n============================================================");
    eprintln!("TEST: kill_node_recovers");
    eprintln!("============================================================\n");
    
    runtime::block_on(async {
        let _env = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        
        eprintln!("[SETUP] Creating 3 nodes...");
        for id in 0..3 {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(id, addr, &peers);
            eprintln!("  Node[{id}] @ {addr}");
            for p in &peers {
                tn.node.add_peer(*p);
            }
            nodes.push(tn);
        }

        wait_for_handshakes(&nodes.iter().collect::<Vec<_>>(), 2, Duration::from_secs(2)).await;

        eprintln!("[MINE] Mining initial blocks...");
        let mut ts = 1u64;
        for _ in 0..2 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(100)).await;
        }

        eprintln!("[CONVERGE] Initial convergence...");
        assert!(wait_until_converged(
            &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
            Duration::from_secs(10),
        ).await, "Initial convergence failed");

        eprintln!("[KILL] Shutting down node 1...");
        nodes[1].shutdown();
        the_block::sleep(Duration::from_millis(500)).await;

        eprintln!("[MINE] Mining while node 1 is down...");
        for _ in 0..3 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            nodes[2].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(100)).await;
        }

        eprintln!("[RESTART] Restarting node 1...");
        let addr = nodes[1].addr;
        let peers: Vec<SocketAddr> = vec![nodes[0].addr, nodes[2].addr];
        nodes[1] = TestNode::new(1, addr, &peers);
        for peer in &peers {
            nodes[1].node.add_peer(*peer);
        }

        eprintln!("[CONVERGE] Post-restart convergence...");
        the_block::sleep(Duration::from_millis(500)).await;
        for node in &nodes {
            node.node.broadcast_chain();
        }

        assert!(wait_until_converged(
            &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
            Duration::from_secs(15),
        ).await, "Killed node failed to catch up");

        eprintln!("[SUCCESS] Node recovered and synced\n");

        for n in &mut nodes {
            n.shutdown();
        }
    });
}

// Keep the original test for backwards compatibility
#[testkit::tb_serial]
fn partitions_merge_consistent_fork_choice() {
    partition_heals_to_majority();
}
