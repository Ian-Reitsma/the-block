use clap::Subcommand;
use storage::{StorageContract, StorageOffer};
use the_block::{
    generate_keypair, rpc,
    transaction::{sign_tx, RawTxPayload},
};

#[derive(Subcommand)]
pub enum StorageCmd {
    /// Upload data to storage market
    Upload {
        object_id: String,
        provider_id: String,
        bytes: u64,
        shares: u16,
        price: u64,
        retention: u64,
    },
    /// Challenge a storage provider
    Challenge {
        object_id: String,
        chunk: u64,
        block: u64,
    },
}

pub fn handle(cmd: StorageCmd) {
    match cmd {
        StorageCmd::Upload {
            object_id,
            provider_id,
            bytes,
            shares,
            price,
            retention,
        } => {
            let contract = StorageContract {
                object_id: object_id.clone(),
                provider_id: provider_id.clone(),
                original_bytes: bytes,
                shares,
                price_per_block: price,
                start_block: 0,
                retention_blocks: retention,
                next_payment_block: 1,
                accrued: 0,
            };
            let total = price * retention;
            let payload = RawTxPayload {
                from_: "wallet".into(),
                to: provider_id.clone(),
                amount_consumer: total,
                amount_industrial: 0,
                fee: 0,
                pct_ct: 100,
                nonce: 0,
                memo: Vec::new(),
            };
            let (sk, _pk) = generate_keypair();
            let _signed = sign_tx(&sk, &payload).expect("signing");
            let offer = StorageOffer::new(provider_id, bytes, price, retention);
            let resp = rpc::storage::upload(contract, vec![offer]);
            println!("{}", resp);
            println!("reserved {} CT", total);
        }
        StorageCmd::Challenge {
            object_id,
            chunk,
            block,
        } => {
            use blake3::Hasher;
            let mut h = Hasher::new();
            h.update(object_id.as_bytes());
            h.update(&chunk.to_le_bytes());
            let mut proof = [0u8; 32];
            proof.copy_from_slice(h.finalize().as_bytes());
            let resp = rpc::storage::challenge(&object_id, chunk, proof, block);
            println!("{}", resp);
        }
    }
}
