#![cfg(feature = "integration-tests")]
use std::net::SocketAddr;
use std::time::{Duration, Instant};
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
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key"));
    std::env::set_var("TB_NET_KEY_SEED", "chaos");
    std::env::set_var("TB_PEER_SEED", "1");
    std::env::set_var("TB_NET_PACKET_LOSS", "0.15");
    std::env::set_var("TB_NET_JITTER_MS", "200");
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers_default"));
    std::fs::write(dir.path().join("seed"), b"chaos").unwrap();
    dir
}

fn timeout_factor() -> u64 {
    std::env::var("TB_TEST_TIMEOUT_MULT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

async fn wait_until_converged(nodes: &[&Node], max: Duration) -> bool {
    let start = Instant::now();
    loop {
        let first = nodes[0].blockchain().block_height;
        if nodes.iter().all(|n| n.blockchain().block_height == first) {
            return true;
        }
        if start.elapsed() > max {
            return false;
        }
        the_block::sleep(Duration::from_millis(20)).await;
    }
}

struct TestNode {
    addr: SocketAddr,
    dir: sys::tempfile::TempDir,
    node: Node,
    flag: ShutdownFlag,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestNode {
    fn new(addr: SocketAddr, peers: Vec<SocketAddr>) -> Self {
        let dir = tempdir().unwrap();
        let bc = Blockchain::open(dir.path().to_str().unwrap()).expect("open bc");
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));
        let node = Node::new(addr, peers, bc);
        let flag = ShutdownFlag::new();
        let handle = node.start_with_flag(&flag);
        node.discover_peers();
        Self {
            addr,
            dir,
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
    });
}

#[testkit::tb_serial]
#[ignore]
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
        for _ in 0..20 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(50)).await;
        }
        let max = Duration::from_secs(5 * timeout_factor());
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
        for _ in 0..20 {
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
        let start = Instant::now();
        assert!(wait_until_converged(&active, max).await);
        println!("convergence after removal {:?}", start.elapsed());

        let restart_bc = Blockchain::open(nodes[2].dir.path().to_str().unwrap()).unwrap();
        let node3 = Node::new(
            nodes[2].addr,
            active.iter().map(|n| n.addr()).collect(),
            restart_bc,
        );
        for (i, n) in nodes.iter().enumerate() {
            if i != 2 {
                n.node.add_peer(nodes[2].addr);
            }
        }
        let flag = ShutdownFlag::new();
        let handle = node3.start_with_flag(&flag);
        node3.discover_peers();
        let dir = std::mem::replace(&mut nodes[2].dir, tempdir().unwrap());
        nodes[2] = TestNode {
            addr: nodes[2].addr,
            dir,
            node: node3,
            flag,
            handle: Some(handle),
        };
        let start = Instant::now();
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await
        );
        println!("final convergence {:?}", start.elapsed());
        let h = nodes[0].node.blockchain().block_height;
        assert_eq!(h, 40);
        for n in nodes.iter_mut() {
            n.shutdown();
        }
        std::env::remove_var("TB_NET_PACKET_LOSS");
        std::env::remove_var("TB_NET_JITTER_MS");
    });
}

#[testkit::tb_serial]
#[ignore]
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

        for _ in 0..10 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(50)).await;
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
        let max = Duration::from_secs(5 * timeout_factor());
        let start = Instant::now();
        assert!(
            wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await
        );
        println!("partition heal convergence {:?}", start.elapsed());
        let h = nodes[0].node.blockchain().block_height;
        assert_eq!(h, 12);
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
    });
}
