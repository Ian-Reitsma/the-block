mod util;
use ed25519_dalek::SigningKey;
use rand_core::{OsRng, RngCore};
use serial_test::serial;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;
use tempfile::tempdir;
use the_block::{
    generate_keypair,
    net::{self, Message, Node, Payload, LOCAL_FEATURES, PROTOCOL_VERSION},
    p2p::handshake::{Hello, Transport},
    sign_tx, Block, Blockchain, RawTxPayload, ShutdownFlag, TokenAmount,
};
use tokio::time::Instant;
use util::fork::inject_fork;

fn send(addr: SocketAddr, sk: &SigningKey, body: Payload) {
    let msg = Message::new(body, sk);
    let mut stream = TcpStream::connect(addr).unwrap();
    let bytes = bincode::serialize(&msg).unwrap();
    stream.write_all(&bytes).unwrap();
}

fn free_addr() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn init_env() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    net::ban_store::init(dir.path().join("ban_db").to_str().unwrap());
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key_default"));
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join("peers_default"));
    std::env::set_var("TB_PEER_SEED", "1");
    dir
}

fn make_node(
    dir: &tempfile::TempDir,
    idx: usize,
    addr: SocketAddr,
    peers: Vec<SocketAddr>,
    bc: Blockchain,
) -> Node {
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join(format!("net_key_{idx}")));
    std::env::set_var("TB_NET_KEY_SEED", format!("seed{idx}"));
    std::env::set_var("TB_PEER_DB_PATH", dir.path().join(format!("peers_{idx}")));
    Node::new(addr, peers, bc)
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

async fn wait_until_peers(nodes: &[&Node], expected: usize, max: Duration) -> bool {
    let start = Instant::now();
    loop {
        if nodes.iter().all(|n| n.peer_addrs().len() == expected) {
            return true;
        }
        if start.elapsed() > max {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn broadcast_until(node: &Node, group: &[&Node]) {
    let deadline = Instant::now() + Duration::from_secs(30 * timeout_factor());
    loop {
        node.broadcast_chain();
        if wait_until_converged(group, Duration::from_secs(3)).await {
            break;
        }
        if Instant::now() > deadline {
            panic!("gossip convergence failed");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Spin up three nodes that exchange transactions and blocks, ensuring
/// they converge to the same chain height even after a temporary fork.
#[tokio::test]
#[serial]
#[ignore]
async fn gossip_converges_to_longest_chain() {
    std::env::set_var("TB_GOSSIP_FANOUT", "all");
    let dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();
    let addr3 = free_addr();

    let node1 = make_node(&dir, 1, addr1, vec![addr2, addr3], Blockchain::default());
    let node2 = make_node(&dir, 2, addr2, vec![addr1, addr3], Blockchain::default());
    let node3 = make_node(&dir, 3, addr3, vec![addr1, addr2], Blockchain::default());

    let flag1 = ShutdownFlag::new();
    let flag2 = ShutdownFlag::new();
    let flag3 = ShutdownFlag::new();
    let jh1 = node1.start_with_flag(&flag1);
    let jh2 = node2.start_with_flag(&flag2);
    let jh3 = node3.start_with_flag(&flag3);

    // Wait for listeners to bind before peer discovery.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut discovered = false;
    for _ in 0..5 {
        node1.discover_peers();
        node2.discover_peers();
        node3.discover_peers();
        if wait_until_peers(
            &[&node1, &node2, &node3],
            2,
            Duration::from_secs(2 * timeout_factor()),
        )
        .await
        {
            discovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(discovered, "peer discovery failed");

    // allow handshakes to settle
    tokio::time::sleep(Duration::from_millis(200)).await;

    // genesis block from node1
    let mut ts = 1;
    inject_fork(&node1, "miner1", ts, 1);
    ts += 1;
    broadcast_until(&node1, &[&node1, &node2, &node3]).await;

    // each secondary node mines a block at height 2 without broadcasting
    inject_fork(&node2, "miner2", ts, 1);
    inject_fork(&node3, "miner3", ts, 1);
    ts += 1;

    // node3 advertises its fork first, node2 follows
    broadcast_until(&node3, &[&node1, &node2, &node3]).await;
    broadcast_until(&node2, &[&node1, &node2, &node3]).await;

    // node2 extends its fork to become the longest chain
    inject_fork(&node2, "miner2", ts, 1);
    broadcast_until(&node2, &[&node1, &node2, &node3]).await;

    let h1 = node1.blockchain().block_height;
    let h2 = node2.blockchain().block_height;
    let h3 = node3.blockchain().block_height;
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1, 3);
    #[cfg(feature = "telemetry")]
    assert!(the_block::telemetry::GOSSIP_CONVERGENCE_SECONDS.get_sample_count() > 0);

    flag1.trigger();
    flag2.trigger();
    flag3.trigger();
    jh1.join().unwrap();
    jh2.join().unwrap();
    jh3.join().unwrap();
    std::env::remove_var("TB_GOSSIP_FANOUT");
}

/// Start two nodes, then introduce a third with a longer fork to ensure
/// the network adopts the longest chain after reconnection.
#[tokio::test]
#[serial]
#[ignore]
async fn partition_rejoins_longest_chain() {
    let dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();
    let addr3 = free_addr();

    let node1 = make_node(&dir, 1, addr1, vec![addr2], Blockchain::default());
    let node2 = make_node(&dir, 2, addr2, vec![addr1], Blockchain::default());

    let flag1 = ShutdownFlag::new();
    let flag2 = ShutdownFlag::new();
    let jh1 = node1.start_with_flag(&flag1);
    let jh2 = node2.start_with_flag(&flag2);

    node1.discover_peers();
    node2.discover_peers();

    let mut ts = 1;
    inject_fork(&node1, "miner1", ts, 1);
    ts += 1;
    inject_fork(&node1, "miner1", ts, 1);
    ts += 1;
    node1.broadcast_chain();

    // Third node mines a longer chain while isolated
    let node3 = make_node(&dir, 3, addr3, vec![addr1, addr2], Blockchain::default());
    let flag3 = ShutdownFlag::new();
    let jh3 = node3.start_with_flag(&flag3);
    inject_fork(&node3, "miner3", ts, 3);
    node3.discover_peers();
    node3.broadcast_chain();

    assert!(wait_until_converged(&[&node1, &node2, &node3], Duration::from_secs(15)).await);

    let h1 = node1.blockchain().block_height;
    let h2 = node2.blockchain().block_height;
    let h3 = node3.blockchain().block_height;
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1, 3);

    flag1.trigger();
    flag2.trigger();
    flag3.trigger();
    jh1.join().unwrap();
    jh2.join().unwrap();
    jh3.join().unwrap();
}

/// Invalid transactions broadcast over the network are ignored.
#[test]
#[serial]
fn invalid_gossip_tx_rejected() {
    let dir = init_env();
    let addr = free_addr();
    let node = make_node(&dir, 1, addr, vec![], Blockchain::default());
    let flag = ShutdownFlag::new();
    let jh = node.start_with_flag(&flag);
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: LOCAL_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
    };
    send(addr, &kp, Payload::Handshake(hello));
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "unknown".into(),
        to: "miner".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        pct_ct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    send(addr, &kp, Payload::Tx(tx));

    assert!(node.blockchain().mempool_consumer.is_empty());
    flag.trigger();
    jh.join().unwrap();
}

/// Invalid blocks are ignored and do not crash peers.
#[test]
#[serial]
fn invalid_gossip_block_rejected() {
    let dir = init_env();
    let addr = free_addr();
    let node = make_node(&dir, 1, addr, vec![], Blockchain::default());
    let flag = ShutdownFlag::new();
    let jh = node.start_with_flag(&flag);
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let hello = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: LOCAL_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
    };
    send(addr, &kp, Payload::Handshake(hello));

    let block = Block {
        index: 99,
        previous_hash: "bad".into(),
        timestamp_millis: 0,
        transactions: Vec::new(),
        difficulty: 0,
        retune_hint: 0,
        nonce: 0,
        hash: "bad".into(),
        coinbase_consumer: TokenAmount::new(0),
        coinbase_industrial: TokenAmount::new(0),
        storage_sub_ct: TokenAmount::new(0),
        read_sub_ct: TokenAmount::new(0),
        compute_sub_ct: TokenAmount::new(0),
        storage_sub_it: TokenAmount::new(0),
        read_sub_it: TokenAmount::new(0),
        compute_sub_it: TokenAmount::new(0),
        read_root: [0u8; 32],
        fee_checksum: String::new(),
        state_root: String::new(),
        base_fee: 1,
        l2_roots: Vec::new(),
        l2_sizes: Vec::new(),
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: Vec::new(),
    };
    send(addr, &kp, Payload::Block(0, block));

    assert!(node.blockchain().chain.is_empty());
    flag.trigger();
    jh.join().unwrap();
}

/// Blocks signed with unknown keys are discarded.
#[test]
#[serial]
fn forged_identity_rejected() {
    let dir = init_env();
    let addr = free_addr();
    let node = make_node(&dir, 1, addr, vec![], Blockchain::default());
    let flag = ShutdownFlag::new();
    let jh = node.start_with_flag(&flag);

    // Forge a block with an unauthorized key and no handshake
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let block = Block {
        index: 0,
        previous_hash: "0".repeat(64),
        timestamp_millis: 0,
        transactions: Vec::new(),
        difficulty: 0,
        retune_hint: 0,
        nonce: 0,
        hash: String::new(),
        coinbase_consumer: TokenAmount::new(0),
        coinbase_industrial: TokenAmount::new(0),
        storage_sub_ct: TokenAmount::new(0),
        read_sub_ct: TokenAmount::new(0),
        compute_sub_ct: TokenAmount::new(0),
        storage_sub_it: TokenAmount::new(0),
        read_sub_it: TokenAmount::new(0),
        compute_sub_it: TokenAmount::new(0),
        read_root: [0u8; 32],
        fee_checksum: String::new(),
        state_root: String::new(),
        base_fee: 1,
        l2_roots: Vec::new(),
        l2_sizes: Vec::new(),
        vdf_commit: [0u8; 32],
        vdf_output: [0u8; 32],
        vdf_proof: Vec::new(),
    };
    send(addr, &kp, Payload::Block(0, block));

    assert!(node.blockchain().chain.is_empty());
    flag.trigger();
    jh.join().unwrap();
}

/// Peers advertising an unsupported protocol version are ignored.
#[test]
#[serial]
fn handshake_version_mismatch_rejected() {
    let dir = init_env();
    let addr = free_addr();
    let node = make_node(&dir, 1, addr, vec![], Blockchain::default());
    let flag = ShutdownFlag::new();
    let jh = node.start_with_flag(&flag);

    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let bad = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION + 1,
        feature_bits: LOCAL_FEATURES,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
    };
    send(addr, &kp, Payload::Handshake(bad));

    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "x".into(),
        to: "y".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        pct_ct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    send(addr, &kp, Payload::Tx(tx));

    assert!(node.blockchain().mempool_consumer.is_empty());
    flag.trigger();
    jh.join().unwrap();
}

/// Peers missing required feature bits are ignored.
#[test]
#[serial]
fn handshake_feature_mismatch_rejected() {
    let _dir = init_env();
    let addr = free_addr();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();

    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let bad = Hello {
        network_id: [0u8; 4],
        proto_version: PROTOCOL_VERSION,
        feature_bits: 0,
        agent: "test".into(),
        nonce: 0,
        transport: Transport::Tcp,
        quic_addr: None,
        quic_cert: None,
    };
    send(addr, &kp, Payload::Handshake(bad));

    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "x".into(),
        to: "y".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        pct_ct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    send(addr, &kp, Payload::Tx(tx));

    assert!(node.blockchain().mempool_consumer.is_empty());
}

/// Nodes can load seed peers from a config file.
#[test]
#[serial]
fn discover_peers_from_file_loads_seeds() {
    let dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();
    let node1 = make_node(&dir, 1, addr1, vec![], Blockchain::default());
    let node2 = make_node(&dir, 2, addr2, vec![], Blockchain::default());
    let flag2 = ShutdownFlag::new();
    let jh2 = node2.start_with_flag(&flag2);
    let cfg = dir.path().join("seeds.txt");
    fs::write(&cfg, format!("{}\n", addr2)).unwrap();
    node1.discover_peers_from_file(&cfg);
    assert!(node1.peer_addrs().contains(&addr2));
    flag2.trigger();
    jh2.join().unwrap();
}

#[test]
#[serial]
fn peer_rate_limit_and_ban() {
    std::env::set_var("TB_P2P_MAX_PER_SEC", "3");
    the_block::net::set_p2p_max_per_sec(3);
    std::env::set_var("TB_P2P_BAN_SECS", "60");
    let dir = init_env();
    let addr = free_addr();
    let mut bc = Blockchain::default();
    bc.add_account("alice".into(), 100, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let node = make_node(&dir, 1, addr, vec![], bc);
    let flag = ShutdownFlag::new();
    let jh = node.start_with_flag(&flag);
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    send(
        addr,
        &sk,
        Payload::Handshake(Hello {
            network_id: [0u8; 4],
            proto_version: PROTOCOL_VERSION,
            feature_bits: LOCAL_FEATURES,
            agent: "test".into(),
            nonce: 0,
            transport: Transport::Tcp,
            quic_addr: None,
            quic_cert: None,
        }),
    );
    for _ in 0..4 {
        send(addr, &sk, Payload::Hello(vec![]));
    }
    let (sk_tx, _pk_tx) = generate_keypair();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 1,
        amount_industrial: 0,
        fee: 1000,
        pct_ct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk_tx.to_vec(), payload).unwrap();
    send(addr, &sk, Payload::Tx(tx));
    assert!(node.blockchain().mempool_consumer.is_empty());
    std::env::remove_var("TB_P2P_MAX_PER_SEC");
    std::env::remove_var("TB_P2P_BAN_SECS");
    the_block::net::set_p2p_max_per_sec(100);
    flag.trigger();
    jh.join().unwrap();
}

#[tokio::test]
#[serial]
#[ignore]
async fn partition_state_replay() {
    let dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();

    let mut bc1 = Blockchain::default();
    let mut bc2 = Blockchain::default();
    bc1.add_account("alice".into(), 5000, 0).unwrap();
    bc1.add_account("bob".into(), 0, 0).unwrap();
    bc2.add_account("alice".into(), 5000, 0).unwrap();
    bc2.add_account("bob".into(), 0, 0).unwrap();

    let node1 = make_node(&dir, 1, addr1, vec![addr2], bc1);
    let node2 = make_node(&dir, 2, addr2, vec![addr1], bc2);

    let flag1 = ShutdownFlag::new();
    let flag2 = ShutdownFlag::new();
    let jh1 = node1.start_with_flag(&flag1);
    let jh2 = node2.start_with_flag(&flag2);

    let mut ts = 1;
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 5,
        amount_industrial: 0,
        fee: 1000,
        pct_ct: 100,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    {
        let mut bc = node1.blockchain();
        bc.submit_transaction(tx).unwrap();
        bc.mine_block_at("alice", ts).unwrap();
    }

    {
        let mut bc = node2.blockchain();
        bc.mine_block_at("alice", ts).unwrap();
        ts += 1;
        bc.mine_block_at("alice", ts).unwrap();
    }

    node1.discover_peers();
    node2.discover_peers();
    node1.broadcast_chain();
    node2.broadcast_chain();

    assert!(wait_until_converged(&[&node1, &node2], Duration::from_secs(15)).await);

    assert_eq!(node1.blockchain().block_height, 2);
    assert_eq!(node2.blockchain().block_height, 2);
    let bal1 = node1
        .blockchain()
        .accounts
        .get("bob")
        .map(|a| a.balance.consumer)
        .unwrap_or(0);
    let bal2 = node2
        .blockchain()
        .accounts
        .get("bob")
        .map(|a| a.balance.consumer)
        .unwrap_or(0);
    assert_eq!(bal1, 0);
    assert_eq!(bal2, 0);
    flag1.trigger();
    flag2.trigger();
    jh1.join().unwrap();
    jh2.join().unwrap();
}
