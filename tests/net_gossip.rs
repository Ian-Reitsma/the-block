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
    net::{self, Handshake, Message, Node, Payload, LOCAL_FEATURES, PROTOCOL_VERSION},
    sign_tx, Block, Blockchain, RawTxPayload, TokenAmount,
};
use tokio::time::Instant;

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
    std::env::set_var("TB_NET_KEY_PATH", dir.path().join("net_key"));
    dir
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

/// Spin up three nodes that exchange transactions and blocks, ensuring
/// they converge to the same chain height even after a temporary fork.
#[tokio::test]
#[serial]
async fn gossip_converges_to_longest_chain() {
    let _dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();
    let addr3 = free_addr();

    let node1 = Node::new(addr1, vec![addr2, addr3], Blockchain::default());
    let node2 = Node::new(addr2, vec![addr1, addr3], Blockchain::default());
    let node3 = Node::new(addr3, vec![addr1, addr2], Blockchain::default());

    let _h1 = node1.start();
    let _h2 = node2.start();
    let _h3 = node3.start();

    node1.discover_peers();
    node2.discover_peers();
    node3.discover_peers();
    // Allow extra time for the peer table to propagate across threads so
    // subsequent broadcasts reach all nodes deterministically. The gossip
    // test is occasionally flaky on slower CI runners, so wait a full second
    // before starting the exchange.
    tokio::time::sleep(Duration::from_secs(1)).await;

    // genesis block from node1
    let mut ts = 1;
    {
        let mut bc = node1.blockchain();
        bc.mine_block_at("miner1", ts).unwrap();
        ts += 1;
    }
    node1.broadcast_chain();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // broadcast a transaction from miner1 to miner2
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "miner1".into(),
        to: "miner2".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    node1.broadcast_tx(tx);

    // each secondary node mines a block at height 2 without broadcasting
    {
        let mut bc = node2.blockchain();
        bc.mine_block_at("miner2", ts).unwrap();
    }
    {
        let mut bc = node3.blockchain();
        bc.mine_block_at("miner3", ts).unwrap();
    }

    // node3 advertises its fork first, node2 follows
    node3.broadcast_chain();
    node2.broadcast_chain();

    // node2 extends its fork to become the longest chain
    {
        let mut bc = node2.blockchain();
        bc.mine_block_at("miner2", ts).unwrap();
    }
    node2.broadcast_chain();

    assert!(wait_until_converged(&[&node1, &node2, &node3], Duration::from_secs(20)).await);

    let h1 = node1.blockchain().block_height;
    let h2 = node2.blockchain().block_height;
    let h3 = node3.blockchain().block_height;
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1, 3);
    #[cfg(feature = "telemetry")]
    assert!(the_block::telemetry::GOSSIP_CONVERGENCE_SECONDS.get_sample_count() > 0);
}

/// Start two nodes, then introduce a third with a longer fork to ensure
/// the network adopts the longest chain after reconnection.
#[tokio::test]
#[serial]
async fn partition_rejoins_longest_chain() {
    let _dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();
    let addr3 = free_addr();

    let node1 = Node::new(addr1, vec![addr2], Blockchain::default());
    let node2 = Node::new(addr2, vec![addr1], Blockchain::default());

    let _h1 = node1.start();
    let _h2 = node2.start();

    node1.discover_peers();
    node2.discover_peers();

    let mut ts = 1;
    {
        let mut bc = node1.blockchain();
        bc.mine_block_at("miner1", ts).unwrap();
        ts += 1;
        bc.mine_block_at("miner1", ts).unwrap();
        ts += 1;
    }
    node1.broadcast_chain();

    // Third node mines a longer chain while isolated
    let node3 = Node::new(addr3, vec![addr1, addr2], Blockchain::default());
    let _h3 = node3.start();
    {
        let mut bc = node3.blockchain();
        bc.mine_block_at("miner3", ts).unwrap();
        ts += 1;
        bc.mine_block_at("miner3", ts).unwrap();
        ts += 1;
        bc.mine_block_at("miner3", ts).unwrap();
    }
    node3.discover_peers();
    node3.broadcast_chain();

    assert!(wait_until_converged(&[&node1, &node2, &node3], Duration::from_secs(20)).await);

    let h1 = node1.blockchain().block_height;
    let h2 = node2.blockchain().block_height;
    let h3 = node3.blockchain().block_height;
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1, 3);
}

/// Invalid transactions broadcast over the network are ignored.
#[test]
#[serial]
fn invalid_gossip_tx_rejected() {
    let _dir = init_env();
    let addr = free_addr();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let hs = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: LOCAL_FEATURES,
    };
    send(addr, &kp, Payload::Handshake(hs));
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "unknown".into(),
        to: "miner".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    send(addr, &kp, Payload::Tx(tx));

    assert!(node.blockchain().mempool_consumer.is_empty());
}

/// Invalid blocks are ignored and do not crash peers.
#[test]
#[serial]
fn invalid_gossip_block_rejected() {
    let _dir = init_env();
    let addr = free_addr();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let hs = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: LOCAL_FEATURES,
    };
    send(addr, &kp, Payload::Handshake(hs));

    let block = Block {
        index: 99,
        previous_hash: "bad".into(),
        timestamp_millis: 0,
        transactions: Vec::new(),
        difficulty: 0,
        nonce: 0,
        hash: "bad".into(),
        coinbase_consumer: TokenAmount::new(0),
        coinbase_industrial: TokenAmount::new(0),
        fee_checksum: String::new(),
        state_root: String::new(),
    };
    send(addr, &kp, Payload::Block(block));

    assert!(node.blockchain().chain.is_empty());
}

/// Blocks signed with unknown keys are discarded.
#[test]
#[serial]
fn forged_identity_rejected() {
    let _dir = init_env();
    let addr = free_addr();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();

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
        nonce: 0,
        hash: String::new(),
        coinbase_consumer: TokenAmount::new(0),
        coinbase_industrial: TokenAmount::new(0),
        fee_checksum: String::new(),
        state_root: String::new(),
    };
    send(addr, &kp, Payload::Block(block));

    assert!(node.blockchain().chain.is_empty());
}

/// Peers advertising an unsupported protocol version are ignored.
#[test]
#[serial]
fn handshake_version_mismatch_rejected() {
    let _dir = init_env();
    let addr = free_addr();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();

    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let bad = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION + 1,
        features: LOCAL_FEATURES,
    };
    send(addr, &kp, Payload::Handshake(bad));

    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "x".into(),
        to: "y".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk.to_vec(), payload).unwrap();
    send(addr, &kp, Payload::Tx(tx));

    assert!(node.blockchain().mempool_consumer.is_empty());
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
    let bad = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: 0,
    };
    send(addr, &kp, Payload::Handshake(bad));

    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "x".into(),
        to: "y".into(),
        amount_consumer: 1,
        amount_industrial: 1,
        fee: 1,
        fee_selector: 0,
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
    let node1 = Node::new(addr1, vec![], Blockchain::default());
    let node2 = Node::new(addr2, vec![], Blockchain::default());
    let _h2 = node2.start();
    let cfg = dir.path().join("seeds.txt");
    fs::write(&cfg, format!("{}\n", addr2)).unwrap();
    node1.discover_peers_from_file(&cfg);
    assert!(node1.peer_addrs().contains(&addr2));
}

#[test]
#[serial]
fn peer_rate_limit_and_ban() {
    std::env::set_var("TB_P2P_MAX_PER_SEC", "3");
    std::env::set_var("TB_P2P_BAN_SECS", "60");
    let _dir = init_env();
    let addr = free_addr();
    let mut bc = Blockchain::default();
    bc.add_account("alice".into(), 100, 0).unwrap();
    bc.add_account("bob".into(), 0, 0).unwrap();
    let node = Node::new(addr, vec![], bc);
    let _h = node.start();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    send(
        addr,
        &sk,
        Payload::Handshake(Handshake {
            node_id: sk.verifying_key().to_bytes(),
            protocol_version: PROTOCOL_VERSION,
            features: LOCAL_FEATURES,
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
        fee_selector: 0,
        nonce: 1,
        memo: Vec::new(),
    };
    let tx = sign_tx(sk_tx.to_vec(), payload).unwrap();
    send(addr, &sk, Payload::Tx(tx));
    assert!(node.blockchain().mempool_consumer.is_empty());
    std::env::remove_var("TB_P2P_MAX_PER_SEC");
    std::env::remove_var("TB_P2P_BAN_SECS");
    std::env::remove_var("TB_NET_KEY_PATH");
}

#[tokio::test]
#[serial]
async fn partition_state_replay() {
    let _dir = init_env();
    let addr1 = free_addr();
    let addr2 = free_addr();

    let mut bc1 = Blockchain::default();
    let mut bc2 = Blockchain::default();
    bc1.add_account("alice".into(), 5000, 0).unwrap();
    bc1.add_account("bob".into(), 0, 0).unwrap();
    bc2.add_account("alice".into(), 5000, 0).unwrap();
    bc2.add_account("bob".into(), 0, 0).unwrap();

    let node1 = Node::new(addr1, vec![addr2], bc1);
    let node2 = Node::new(addr2, vec![addr1], bc2);

    let _h1 = node1.start();
    let _h2 = node2.start();

    let mut ts = 1;
    let (sk, _pk) = generate_keypair();
    let payload = RawTxPayload {
        from_: "alice".into(),
        to: "bob".into(),
        amount_consumer: 5,
        amount_industrial: 0,
        fee: 1000,
        fee_selector: 0,
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

    assert!(wait_until_converged(&[&node1, &node2], Duration::from_secs(20)).await);

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
    std::env::remove_var("TB_NET_KEY_PATH");
}
