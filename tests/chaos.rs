use serial_test::serial;
use std::net::SocketAddr;
use std::time::Duration;
use tempfile::tempdir;
use the_block::{net::Node, Blockchain, ShutdownFlag};
use tokio::time::Instant;

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn init_env() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    the_block::net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key"));
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
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct TestNode {
    addr: SocketAddr,
    dir: tempfile::TempDir,
    node: Node,
    flag: ShutdownFlag,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestNode {
    fn new(addr: SocketAddr, peers: Vec<SocketAddr>) -> Self {
        let dir = tempdir().unwrap();
        let bc = Blockchain::open(dir.path().to_str().unwrap()).expect("open bc");
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

#[tokio::test]
#[serial]
#[ignore]
async fn kill_node_recovers() {
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
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut ts = 1u64;
    for _ in 0..20 {
        {
            let mut bc = nodes[0].node.blockchain();
            bc.mine_block_at("miner", ts).unwrap();
            ts += 1;
        }
        nodes[0].node.broadcast_chain();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let max = Duration::from_secs(5 * timeout_factor());
    assert!(wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await);

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
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let active: Vec<&Node> = nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 2)
        .map(|(_, n)| &n.node)
        .collect();
    assert!(wait_until_converged(&active, max).await);

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
    assert!(wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await);
    let h = nodes[0].node.blockchain().block_height;
    assert_eq!(h, 40);
    for n in nodes.iter_mut() {
        n.shutdown();
    }
}

#[tokio::test]
#[serial]
#[ignore]
async fn partition_heals_to_majority() {
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
    tokio::time::sleep(Duration::from_secs(1)).await;
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
        tokio::time::sleep(Duration::from_millis(50)).await;
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
    assert!(wait_until_converged(&nodes.iter().map(|n| &n.node).collect::<Vec<_>>(), max).await);
    let h = nodes[0].node.blockchain().block_height;
    assert_eq!(h, 12);
    #[cfg(feature = "telemetry")]
    {
        let c = the_block::telemetry::FORK_REORG_TOTAL
            .with_label_values(&["0"])
            .get();
        assert!(c > 0);
    }
    for n in nodes.iter_mut() {
        n.shutdown();
    }
}
