use the_block::{block_binary, receipts::StorageReceipt, Block, Receipt};

#[test]
fn replay_roundtrip_block_with_receipts() {
    let receipt = Receipt::Storage(StorageReceipt {
        contract_id: "contract_1".into(),
        provider: "provider_1".into(),
        bytes: 1024,
        price: 10,
        block_height: 1,
        provider_escrow: 50,
        provider_signature: vec![0u8; 64],
        signature_nonce: 0,
    });

    let block = Block {
        index: 1,
        receipts: vec![receipt],
        ..Default::default()
    };

    let encoded = block_binary::encode_block(&block).expect("encode block");
    let decoded = block_binary::decode_block(&encoded).expect("decode block");

    assert_eq!(block, decoded);
}
