use clap::Subcommand;
use storage::StorageContract;

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
    Challenge { object_id: String, block: u64 },
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
                object_id,
                provider_id,
                original_bytes: bytes,
                shares,
                price_per_block: price,
                start_block: 0,
                retention_blocks: retention,
            };
            println!("{}", serde_json::to_string(&contract).unwrap());
        }
        StorageCmd::Challenge { object_id, block } => {
            println!("challenge {} at {}", object_id, block);
        }
    }
}
