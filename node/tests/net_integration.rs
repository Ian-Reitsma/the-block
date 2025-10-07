#![cfg(feature = "integration-tests")]
use std::net::SocketAddr;
use std::time::Duration;
use std::time::Instant;
use tempfile::tempdir;
use the_block::{net::Node, Blockchain, ShutdownFlag};

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
    std::env::set_var("TB_NET_KEY_SEED", "net_integration");
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));
    std::env::set_var("TB_PEER_SEED", "42");
    dir
}

fn wait_until_converged(nodes: &[&Node], max: Duration) -> bool {
    runtime::block_on(async {
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
    })
}

struct TestNode {
    addr: SocketAddr,
    dir: tempfile::TempDir,
    node: Node,
    flag: ShutdownFlag,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestNode {
    fn new(addr: SocketAddr, peers: &[SocketAddr]) -> Self {
        let dir = tempdir().unwrap();
        let bc = Blockchain::open(dir.path().to_str().unwrap()).expect("open bc");
        std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers"));
        let node = Node::new(addr, peers.to_vec(), bc);
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
fn partitions_merge_consistent_fork_choice() {
    runtime::block_on(async {
        let _env = init_env();
        let mut nodes: Vec<TestNode> = Vec::new();
        for _ in 0..5 {
            let addr = free_addr();
            let peers: Vec<SocketAddr> = nodes.iter().map(|n| n.addr).collect();
            let mut tn = TestNode::new(addr, &peers);
            for p in &peers {
                tn.node.add_peer(*p);
            }
            nodes.push(tn);
        }

        the_block::sleep(Duration::from_millis(100)).await;
        let mut ts = 1u64;
        for _ in 0..3 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("miner", ts).unwrap();
                ts += 1;
            }
            nodes[0].node.broadcast_chain();
            the_block::sleep(Duration::from_millis(50)).await;
        }

        assert!(
            wait_until_converged(
                &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
                Duration::from_secs(5)
            )
            .await
        );

        for i in 0..3 {
            for j in 3..5 {
                nodes[i].node.remove_peer(nodes[j].addr);
                nodes[j].node.remove_peer(nodes[i].addr);
            }
        }

        for _ in 0..2 {
            {
                let mut bc = nodes[0].node.blockchain();
                bc.mine_block_at("minerA", ts).unwrap();
                ts += 1;
            }
            for i in 0..3 {
                nodes[i].node.broadcast_chain();
            }
            the_block::sleep(Duration::from_millis(50)).await;
        }

        for _ in 0..5 {
            {
                let mut bc = nodes[3].node.blockchain();
                bc.mine_block_at("minerB", ts).unwrap();
                ts += 1;
            }
            for j in 3..5 {
                nodes[j].node.broadcast_chain();
            }
            the_block::sleep(Duration::from_millis(50)).await;
        }

        for i in 0..3 {
            for j in 3..5 {
                nodes[i].node.add_peer(nodes[j].addr);
                nodes[j].node.add_peer(nodes[i].addr);
            }
        }

        assert!(
            wait_until_converged(
                &nodes.iter().map(|n| &n.node).collect::<Vec<_>>(),
                Duration::from_secs(10)
            )
            .await
        );

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
    });
}
