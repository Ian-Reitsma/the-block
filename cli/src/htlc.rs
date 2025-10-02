#![forbid(unsafe_code)]

use clap::Subcommand;
use crypto_suite::hashing::sha3::Sha3_256;
use hex::{decode, encode};
use ripemd::{Digest as RipemdDigest, Ripemd160};
use the_block::vm::contracts::htlc::{HashAlgo, Htlc};

#[derive(Subcommand)]
pub enum HtlcCmd {
    /// Create an HTLC from a preimage and timeout
    Create {
        preimage: String,
        #[arg(long, default_value_t = 0)]
        timeout: u64,
        #[arg(long, default_value = "sha3")]
        algo: String,
    },
    /// Redeem an existing HTLC with a preimage
    Redeem {
        hash: String,
        preimage: String,
        #[arg(long, default_value_t = 0)]
        timeout: u64,
        #[arg(long, default_value = "sha3")]
        algo: String,
    },
}

pub fn handle(cmd: HtlcCmd) {
    match cmd {
        HtlcCmd::Create {
            preimage,
            timeout,
            algo,
        } => {
            let bytes = preimage.into_bytes();
            let (hash, algo) = match algo.as_str() {
                "ripemd" => {
                    let mut h = Ripemd160::new();
                    RipemdDigest::update(&mut h, &bytes);
                    (RipemdDigest::finalize(h).to_vec(), HashAlgo::Ripemd160)
                }
                _ => {
                    let mut h = Sha3_256::new();
                    h.update(&bytes);
                    (h.finalize().to_vec(), HashAlgo::Sha3)
                }
            };
            let htlc = Htlc::new(hash, algo, timeout);
            #[cfg(feature = "telemetry")]
            {
                the_block::telemetry::HTLC_CREATED_TOTAL.inc();
            }
            println!("{}", encode(htlc.hash));
        }
        HtlcCmd::Redeem {
            hash,
            preimage,
            timeout,
            algo,
        } => {
            let hash_bytes = decode(hash).expect("invalid hash");
            let algo = match algo.as_str() {
                "ripemd" => HashAlgo::Ripemd160,
                _ => HashAlgo::Sha3,
            };
            let mut htlc = Htlc::new(hash_bytes, algo, timeout);
            let ok = htlc.redeem(preimage.as_bytes(), timeout.saturating_sub(1));
            println!("{}", ok);
        }
    }
}
