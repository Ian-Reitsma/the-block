use ed25519_dalek::SigningKey;
use rand_core::{OsRng, RngCore};
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;
use the_block::{
    generate_keypair,
    net::{Handshake, Message, Node, Payload, PROTOCOL_VERSION},
    sign_tx, Block, Blockchain, RawTxPayload, TokenAmount,
};

fn send(addr: SocketAddr, sk: &SigningKey, body: Payload) {
    let msg = Message::new(body, sk);
    let mut stream = TcpStream::connect(addr).unwrap();
    let bytes = bincode::serialize(&msg).unwrap();
    stream.write_all(&bytes).unwrap();
}

/// Spin up three nodes that exchange transactions and blocks, ensuring
/// they converge to the same chain height even after a temporary fork.
#[test]
fn gossip_converges_to_longest_chain() {
    // fixed localhost ports for deterministic tests
    let addr1: SocketAddr = "127.0.0.1:7001".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:7002".parse().unwrap();
    let addr3: SocketAddr = "127.0.0.1:7003".parse().unwrap();

    let node1 = Node::new(addr1, vec![addr2, addr3], Blockchain::default());
    let node2 = Node::new(addr2, vec![addr1, addr3], Blockchain::default());
    let node3 = Node::new(addr3, vec![addr1, addr2], Blockchain::default());

    let _h1 = node1.start();
    let _h2 = node2.start();
    let _h3 = node3.start();

    node1.discover_peers();
    node2.discover_peers();
    node3.discover_peers();
    thread::sleep(Duration::from_millis(100));

    // genesis block from node1
    {
        let mut bc = node1.blockchain();
        bc.mine_block("miner1").unwrap();
    }
    node1.broadcast_chain();
    thread::sleep(Duration::from_millis(100));

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
        bc.mine_block("miner2").unwrap();
    }
    {
        let mut bc = node3.blockchain();
        bc.mine_block("miner3").unwrap();
    }

    // node3 advertises its fork first, node2 follows
    node3.broadcast_chain();
    node2.broadcast_chain();

    // node2 extends its fork to become the longest chain
    {
        let mut bc = node2.blockchain();
        bc.mine_block("miner2").unwrap();
    }
    node2.broadcast_chain();

    // wait up to 2s for all nodes to converge on the longest chain
    for _ in 0..20 {
        let h1 = node1.blockchain().block_height;
        let h2 = node2.blockchain().block_height;
        let h3 = node3.blockchain().block_height;
        if h1 == h2 && h2 == h3 && h1 == 3 {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let h1 = node1.blockchain().block_height;
    let h2 = node2.blockchain().block_height;
    let h3 = node3.blockchain().block_height;
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1, 3);
}

/// Start two nodes, then introduce a third with a longer fork to ensure
/// the network adopts the longest chain after reconnection.
#[test]
fn partition_rejoins_longest_chain() {
    let addr1: SocketAddr = "127.0.0.1:7101".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:7102".parse().unwrap();
    let addr3: SocketAddr = "127.0.0.1:7103".parse().unwrap();

    let node1 = Node::new(addr1, vec![addr2], Blockchain::default());
    let node2 = Node::new(addr2, vec![addr1], Blockchain::default());

    let _h1 = node1.start();
    let _h2 = node2.start();

    node1.discover_peers();
    node2.discover_peers();

    {
        let mut bc = node1.blockchain();
        bc.mine_block("miner1").unwrap();
        bc.mine_block("miner1").unwrap();
    }
    node1.broadcast_chain();
    thread::sleep(Duration::from_millis(100));

    // Third node mines a longer chain while isolated then broadcasts it
    let node3 = Node::new(addr3, vec![addr1, addr2], Blockchain::default());
    let _h3 = node3.start();
    {
        let mut bc = node3.blockchain();
        bc.mine_block("miner3").unwrap();
        bc.mine_block("miner3").unwrap();
        bc.mine_block("miner3").unwrap();
    }
    node3.broadcast_chain();
    node3.discover_peers();

    thread::sleep(Duration::from_millis(200));

    assert_eq!(node1.blockchain().block_height, 3);
    assert_eq!(node2.blockchain().block_height, 3);
    assert_eq!(node3.blockchain().block_height, 3);
}

/// Invalid transactions broadcast over the network are ignored.
#[test]
fn invalid_gossip_tx_rejected() {
    let addr: SocketAddr = "127.0.0.1:7201".parse().unwrap();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let hs = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: Vec::new(),
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

    thread::sleep(Duration::from_millis(100));

    assert!(node.blockchain().mempool.is_empty());
}

/// Invalid blocks are ignored and do not crash peers.
#[test]
fn invalid_gossip_block_rejected() {
    let addr: SocketAddr = "127.0.0.1:7202".parse().unwrap();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let hs = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION,
        features: Vec::new(),
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
        snapshot_root: String::new(),
    };
    send(addr, &kp, Payload::Block(block));

    thread::sleep(Duration::from_millis(100));

    assert!(node.blockchain().chain.is_empty());
}

/// Blocks signed with unknown keys are discarded.
#[test]
fn forged_identity_rejected() {
    let addr: SocketAddr = "127.0.0.1:7301".parse().unwrap();
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
        snapshot_root: String::new(),
    };
    send(addr, &kp, Payload::Block(block));

    thread::sleep(Duration::from_millis(100));
    assert!(node.blockchain().chain.is_empty());
}

/// Peers advertising an unsupported protocol version are ignored.
#[test]
fn handshake_version_mismatch_rejected() {
    let addr: SocketAddr = "127.0.0.1:7302".parse().unwrap();
    let node = Node::new(addr, vec![], Blockchain::default());
    let _h = node.start();

    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let kp = SigningKey::from_bytes(&seed);
    let bad = Handshake {
        node_id: kp.verifying_key().to_bytes(),
        protocol_version: PROTOCOL_VERSION + 1,
        features: Vec::new(),
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

    thread::sleep(Duration::from_millis(100));
    assert!(node.blockchain().mempool.is_empty());
}
