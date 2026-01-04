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

async fn wait_until_converged(nodes: &[&Node], max: Duration) -> bool {
    let start = Instant::now();
    let mut tick: u64 = 0;
    loop {
        let first = nodes[0].blockchain().block_height;
        if nodes.iter().all(|n| n.blockchain().block_height == first) {
            return true;
        }
        if start.elapsed() > max {
            return false;
        }
        tick += 1;
        if tick % 250 == 0 {
            eprintln!(
                "wait_until_converged: elapsed={:?} heights={:?}",
                start.elapsed(),
                nodes
                    .iter()
                    .map(|n| n.blockchain().block_height)
                    .collect::<Vec<_>>()
            );
        }
        the_block::sleep(Duration::from_millis(20)).await;
    }
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
fn partitions_merge_consistent_fork_choice() {
    runtime::block_on(async {
        let _env = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        for id in 0..5 {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let tn = TestNode::new(id, addr, &peers);
            eprintln!("test node{id} gossip_addr={addr}");
            for p in &peers {
                tn.node.add_peer(*p);
            }
            nodes.push(tn);
        }

        // Give more time for handshakes to complete and peer lists to populate
        the_block::sleep(Duration::from_millis(1500)).await;

        // Ensure all nodes have full peer lists by explicitly adding all peer pairs
        for i in 0..nodes.len() {
            for j in 0..nodes.len() {
                if i != j {
                    nodes[i].node.add_peer(nodes[j].addr);
                }
            }
        }
        the_block::sleep(Duration::from_millis(500)).await;

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

        let heights_after_mine = nodes
            .iter()
            .map(|n| n.node.blockchain().block_height)
            .collect::<Vec<_>>();
        eprintln!(
            "initial mine complete heights={heights_after_mine:?} peers={:?}",
            nodes
                .iter()
                .map(|n| n.node.peer_addrs().len())
                .collect::<Vec<_>>()
        );

        let converged = wait_until_converged(
            &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
            Duration::from_secs(12),
        )
        .await;
        if !converged {
            let heights = nodes
                .iter()
                .map(|n| n.node.blockchain().block_height)
                .collect::<Vec<_>>();
            let peers = nodes
                .iter()
                .map(|n| n.node.peer_addrs().len())
                .collect::<Vec<_>>();
            panic!("initial convergence failed: heights={heights:?} peers={peers:?}");
        }
        eprintln!("initial convergence ok heights={:?}", heights_after_mine);

        for node in nodes.iter().take(3) {
            for peer in nodes.iter().skip(3) {
                node.node.remove_peer(peer.addr);
                peer.node.remove_peer(node.addr);
            }
        }

        eprintln!(
            "partitions set heights={:?}",
            nodes
                .iter()
                .map(|n| n.node.blockchain().block_height)
                .collect::<Vec<_>>()
        );

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

        eprintln!(
            "post-partition mining heights={:?}",
            nodes
                .iter()
                .map(|n| n.node.blockchain().block_height)
                .collect::<Vec<_>>()
        );

        for node in nodes.iter().take(3) {
            for peer in nodes.iter().skip(3) {
                node.node.add_peer(peer.addr);
                peer.node.add_peer(node.addr);
            }
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

        // Trigger chain exchange after partition merge by broadcasting from all nodes
        for node in &nodes {
            node.node.broadcast_chain();
        }
        the_block::sleep(Duration::from_millis(1200)).await;

        eprintln!(
            "post-merge broadcast heights={:?}",
            nodes
                .iter()
                .map(|n| n.node.blockchain().block_height)
                .collect::<Vec<_>>()
        );

        let converged = wait_until_converged(
            &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
            Duration::from_secs(20),
        )
        .await;
        if !converged {
            let heights = nodes
                .iter()
                .map(|n| n.node.blockchain().block_height)
                .collect::<Vec<_>>();
            let peers = nodes
                .iter()
                .map(|n| n.node.peer_addrs().len())
                .collect::<Vec<_>>();
            panic!("post-merge convergence failed: heights={heights:?} peers={peers:?}");
        }

        let height = nodes[3].node.blockchain().block_height;
        assert!(nodes
            .iter()
            .all(|n| n.node.blockchain().block_height == height));

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
