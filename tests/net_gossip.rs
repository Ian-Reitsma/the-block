use std::net::SocketAddr;
use std::thread;
use std::time::Duration;
use the_block::{generate_keypair, net::Node, sign_tx, Blockchain, RawTxPayload};

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

    node1.hello();
    node2.hello();
    node3.hello();

    // genesis block from node1
    {
        let mut bc = node1.blockchain();
        bc.mine_block("miner1").unwrap();
    }
    node1.broadcast_chain();

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

    // allow gossip to propagate
    thread::sleep(Duration::from_millis(200));

    let h1 = node1.blockchain().block_height;
    let h2 = node2.blockchain().block_height;
    let h3 = node3.blockchain().block_height;
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1, 3);
}
